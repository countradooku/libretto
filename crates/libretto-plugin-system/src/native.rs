//! Native Rust plugin loading via `libloading`.
//!
//! This module handles loading and managing native plugins compiled as dynamic
//! libraries (.so on Linux, .dylib on macOS, .dll on Windows).

#![allow(unsafe_code)]

use crate::api::{EventContext, EventResult, Plugin, PluginCapability, PluginInfo, ffi};
use crate::error::{PluginError, Result};
use crate::hooks::Hook;
use dashmap::DashMap;
use libloading::{Library, Symbol};
use parking_lot::RwLock;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Get the dynamic library extension for the current platform.
#[must_use]
pub const fn library_extension() -> &'static str {
    #[cfg(target_os = "linux")]
    {
        "so"
    }
    #[cfg(target_os = "macos")]
    {
        "dylib"
    }
    #[cfg(target_os = "windows")]
    {
        "dll"
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
    {
        "so"
    }
}

/// Native plugin loader.
#[derive(Debug)]
pub struct NativePluginLoader {
    /// Loaded libraries (kept alive to prevent unloading).
    libraries: DashMap<PathBuf, Arc<Library>>,
    /// Hot reload enabled flag.
    hot_reload: bool,
    /// File modification times for hot reload detection.
    file_times: DashMap<PathBuf, std::time::SystemTime>,
}

impl NativePluginLoader {
    /// Create a new native plugin loader.
    #[must_use]
    pub fn new(hot_reload: bool) -> Self {
        Self {
            libraries: DashMap::new(),
            hot_reload,
            file_times: DashMap::new(),
        }
    }

    /// Load a native plugin from a dynamic library.
    ///
    /// # Errors
    /// Returns error if loading fails.
    pub fn load(&self, path: &Path) -> Result<Box<dyn Plugin>> {
        let path = self.resolve_library_path(path)?;

        info!(path = %path.display(), "loading native plugin");

        // Check if already loaded
        if let Some(lib) = self.libraries.get(&path) {
            debug!(path = %path.display(), "library already loaded, reusing");
            return self.create_plugin_from_library(&lib);
        }

        // Load the library
        // SAFETY: We trust the plugin library to be well-formed.
        // The library path has been validated above.
        let library = unsafe { Library::new(&path) }
            .map_err(|e| PluginError::library_load(&path, e.to_string()))?;

        // Verify ABI version
        self.verify_abi_version(&library, &path)?;

        let library = Arc::new(library);

        // Store modification time for hot reload
        if self.hot_reload
            && let Ok(metadata) = std::fs::metadata(&path)
            && let Ok(mtime) = metadata.modified()
        {
            self.file_times.insert(path.clone(), mtime);
        }

        // Create plugin instance
        let plugin = self.create_plugin_from_library(&library)?;

        // Store library reference
        self.libraries.insert(path, library);

        Ok(plugin)
    }

    /// Unload a plugin library.
    pub fn unload(&self, path: &Path) -> Result<()> {
        let path = self.resolve_library_path(path)?;

        if self.libraries.remove(&path).is_some() {
            self.file_times.remove(&path);
            info!(path = %path.display(), "native plugin unloaded");
        }

        Ok(())
    }

    /// Check if a library needs to be reloaded (for hot reload).
    #[must_use]
    pub fn needs_reload(&self, path: &Path) -> bool {
        if !self.hot_reload {
            return false;
        }

        let Ok(path) = self.resolve_library_path(path) else {
            return false;
        };

        let Some(stored_time) = self.file_times.get(&path) else {
            return false;
        };

        let Ok(metadata) = std::fs::metadata(&path) else {
            return false;
        };

        let Ok(current_time) = metadata.modified() else {
            return false;
        };

        current_time > *stored_time
    }

    /// Reload a plugin (hot reload).
    ///
    /// # Errors
    /// Returns error if reloading fails.
    pub fn reload(&self, path: &Path) -> Result<Box<dyn Plugin>> {
        let path = self.resolve_library_path(path)?;

        // Unload first
        self.unload(&path)?;

        // Load again
        self.load(&path)
    }

    /// Resolve the full library path.
    fn resolve_library_path(&self, path: &Path) -> Result<PathBuf> {
        // If path is a directory, look for the library file
        if path.is_dir() {
            let ext = library_extension();

            // Look for common library names
            for name in &["plugin", "libplugin", "lib"] {
                let lib_path = path.join(format!("{name}.{ext}"));
                if lib_path.exists() {
                    return Ok(lib_path);
                }
            }

            // Look for any library file
            if let Ok(entries) = std::fs::read_dir(path) {
                for entry in entries.flatten() {
                    let entry_path = entry.path();
                    if entry_path.extension() == Some(OsStr::new(ext)) {
                        return Ok(entry_path);
                    }
                }
            }

            return Err(PluginError::library_load(
                path,
                format!("no library file found in directory (expected .{ext})"),
            ));
        }

        // Verify the file exists
        if !path.exists() {
            return Err(PluginError::library_load(path, "file does not exist"));
        }

        Ok(path.to_path_buf())
    }

    /// Verify the plugin's ABI version matches.
    fn verify_abi_version(&self, library: &Library, path: &Path) -> Result<()> {
        // SAFETY: We're calling a C function that returns a u32.
        let version_fn: std::result::Result<Symbol<ffi::PluginAbiVersionFn>, _> =
            unsafe { library.get(b"libretto_plugin_abi_version\0") };

        match version_fn {
            Ok(func) => {
                // SAFETY: The function pointer is valid from the loaded library.
                let version = unsafe { func() };
                if version != ffi::PLUGIN_ABI_VERSION {
                    return Err(PluginError::ApiVersionMismatch {
                        required: ffi::PLUGIN_ABI_VERSION.to_string(),
                        found: version.to_string(),
                    });
                }
            }
            Err(_) => {
                // If the version function is not found, assume it's a legacy plugin
                warn!(
                    path = %path.display(),
                    "plugin does not export ABI version function, assuming compatible"
                );
            }
        }

        Ok(())
    }

    /// Create a plugin instance from a loaded library.
    fn create_plugin_from_library(&self, library: &Library) -> Result<Box<dyn Plugin>> {
        // Get the plugin info function
        // SAFETY: We're loading a symbol from a library we just opened.
        let info_fn: Symbol<ffi::PluginInfoFn> = unsafe { library.get(b"libretto_plugin_info\0") }
            .map_err(|e| PluginError::symbol_not_found("library", e.to_string()))?;

        // SAFETY: We're calling a C function that returns FFI-safe data.
        let info_ffi = unsafe { info_fn() };
        // SAFETY: The FFI info struct contains valid pointers from the plugin.
        let info = unsafe { info_ffi.to_plugin_info() };

        // Get the create function
        // SAFETY: We're loading a symbol from a library we just opened.
        let create_fn: Symbol<ffi::PluginCreateFn> =
            unsafe { library.get(b"libretto_plugin_create\0") }
                .map_err(|e| PluginError::symbol_not_found("library", e.to_string()))?;

        // Create the plugin instance
        // SAFETY: We're calling the plugin's constructor function.
        let plugin_ptr = unsafe { create_fn() };

        if plugin_ptr.is_null() {
            return Err(PluginError::InitializationFailed(
                "plugin create function returned null".into(),
            ));
        }

        // Wrap in our native plugin wrapper
        Ok(Box::new(NativePlugin {
            info,
            library: Arc::clone(
                self.libraries
                    .iter()
                    .next()
                    .map(|e| e.value().clone())
                    .as_ref()
                    .ok_or_else(|| {
                        PluginError::InitializationFailed("library not stored".into())
                    })?,
            ),
            plugin_ptr,
            state: RwLock::new(NativePluginState::Created),
        }))
    }
}

/// State of a native plugin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NativePluginState {
    Created,
    Activated,
    Deactivated,
}

/// Wrapper for native plugin instances.
pub struct NativePlugin {
    info: PluginInfo,
    #[allow(dead_code)]
    library: Arc<Library>,
    plugin_ptr: *mut std::ffi::c_void,
    state: RwLock<NativePluginState>,
}

// SAFETY: The plugin pointer is managed and only accessed through safe interfaces.
unsafe impl Send for NativePlugin {}
unsafe impl Sync for NativePlugin {}

impl std::fmt::Debug for NativePlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NativePlugin")
            .field("info", &self.info)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl Drop for NativePlugin {
    fn drop(&mut self) {
        // Call destroy function if available
        // SAFETY: We're loading a symbol from our library.
        if let Ok(destroy_fn) = unsafe {
            self.library
                .get::<ffi::PluginDestroyFn>(b"libretto_plugin_destroy\0")
        } {
            // SAFETY: We're calling the plugin's destructor with the pointer we got from create.
            unsafe { destroy_fn(self.plugin_ptr) };
        }
    }
}

#[async_trait::async_trait]
impl Plugin for NativePlugin {
    fn info(&self) -> PluginInfo {
        self.info.clone()
    }

    fn capabilities(&self) -> Vec<PluginCapability> {
        self.info.capabilities.clone()
    }

    async fn activate(&self) -> Result<()> {
        *self.state.write() = NativePluginState::Activated;
        debug!(plugin = %self.info.name, "native plugin activated");
        Ok(())
    }

    async fn deactivate(&self) -> Result<()> {
        *self.state.write() = NativePluginState::Deactivated;
        debug!(plugin = %self.info.name, "native plugin deactivated");
        Ok(())
    }

    async fn uninstall(&self) -> Result<()> {
        debug!(plugin = %self.info.name, "native plugin uninstalled");
        Ok(())
    }

    async fn on_event(&self, event: Hook, context: &EventContext) -> Result<EventResult> {
        if *self.state.read() != NativePluginState::Activated {
            return Ok(EventResult::ok());
        }

        // Try to find the event handler function
        let handler_name = format!("libretto_plugin_on_{}\0", event.as_str().replace('-', "_"));

        // SAFETY: We're trying to load an optional symbol.
        let handler: std::result::Result<
            Symbol<unsafe extern "C" fn(*mut std::ffi::c_void, *const std::ffi::c_void) -> i32>,
            _,
        > = unsafe { self.library.get(handler_name.as_bytes()) };

        match handler {
            Ok(func) => {
                // Serialize context for FFI
                let context_json = sonic_rs::to_string(context)
                    .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

                let context_cstr = std::ffi::CString::new(context_json)
                    .map_err(|e| PluginError::ExecutionFailed(e.to_string()))?;

                // SAFETY: We're calling the plugin's event handler with valid pointers.
                let result = unsafe {
                    func(
                        self.plugin_ptr,
                        context_cstr.as_ptr().cast::<std::ffi::c_void>(),
                    )
                };

                // Interpret result: 0 = continue, 1 = stop, negative = error
                if result < 0 {
                    Ok(EventResult::error(format!(
                        "plugin returned error code: {result}"
                    )))
                } else if result == 1 {
                    Ok(EventResult::stop())
                } else {
                    Ok(EventResult::ok())
                }
            }
            Err(_) => {
                // No handler for this event
                Ok(EventResult::ok())
            }
        }
    }
}

/// Macro to help define native plugins.
///
/// This generates the necessary FFI functions for a native plugin.
#[macro_export]
macro_rules! declare_native_plugin {
    ($plugin_type:ty, $info:expr) => {
        static PLUGIN_INFO: std::sync::OnceLock<$crate::api::PluginInfo> =
            std::sync::OnceLock::new();

        #[no_mangle]
        pub extern "C" fn libretto_plugin_abi_version() -> u32 {
            $crate::api::ffi::PLUGIN_ABI_VERSION
        }

        #[no_mangle]
        pub extern "C" fn libretto_plugin_info() -> $crate::api::ffi::PluginInfoFFI {
            let info = PLUGIN_INFO.get_or_init(|| $info);

            static NAME: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
            static VERSION: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
            static DESCRIPTION: std::sync::OnceLock<std::ffi::CString> = std::sync::OnceLock::new();
            static API_VERSION: std::sync::OnceLock<Option<std::ffi::CString>> =
                std::sync::OnceLock::new();

            let name =
                NAME.get_or_init(|| std::ffi::CString::new(info.name.as_str()).unwrap_or_default());
            let version = VERSION
                .get_or_init(|| std::ffi::CString::new(info.version.as_str()).unwrap_or_default());
            let description = DESCRIPTION.get_or_init(|| {
                std::ffi::CString::new(info.description.as_str()).unwrap_or_default()
            });
            let api_version = API_VERSION.get_or_init(|| {
                info.api_version
                    .as_ref()
                    .and_then(|v| std::ffi::CString::new(v.as_str()).ok())
            });

            $crate::api::ffi::PluginInfoFFI {
                name: name.as_ptr(),
                version: version.as_ptr(),
                description: description.as_ptr(),
                api_version: api_version
                    .as_ref()
                    .map_or(std::ptr::null(), |v| v.as_ptr()),
                capabilities: $crate::api::ffi::capabilities_to_bitmask(&info.capabilities),
                priority: info.priority.unwrap_or(0),
            }
        }

        #[no_mangle]
        pub extern "C" fn libretto_plugin_create() -> *mut std::ffi::c_void {
            let plugin = Box::new(<$plugin_type>::new());
            Box::into_raw(plugin).cast::<std::ffi::c_void>()
        }

        #[no_mangle]
        pub extern "C" fn libretto_plugin_destroy(ptr: *mut std::ffi::c_void) {
            if !ptr.is_null() {
                // SAFETY: We created this pointer in libretto_plugin_create
                unsafe {
                    drop(Box::from_raw(ptr.cast::<$plugin_type>()));
                }
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn library_extension_test() {
        let ext = library_extension();
        assert!(!ext.is_empty());
        #[cfg(target_os = "linux")]
        assert_eq!(ext, "so");
        #[cfg(target_os = "macos")]
        assert_eq!(ext, "dylib");
        #[cfg(target_os = "windows")]
        assert_eq!(ext, "dll");
    }

    #[test]
    fn loader_creation() {
        let loader = NativePluginLoader::new(false);
        assert!(!loader.hot_reload);
        assert!(loader.libraries.is_empty());
    }

    #[test]
    fn loader_with_hot_reload() {
        let loader = NativePluginLoader::new(true);
        assert!(loader.hot_reload);
    }

    #[test]
    fn resolve_nonexistent_path() {
        let loader = NativePluginLoader::new(false);
        let result = loader.resolve_library_path(Path::new("/nonexistent/path/plugin.so"));
        assert!(result.is_err());
    }
}
