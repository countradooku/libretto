//! Plugin API definitions.
//!
//! This module defines the stable plugin API for both native Rust plugins and PHP plugins.
//! The API uses `extern "C"` for ABI stability.

use crate::error::Result;
use crate::hooks::Hook;
use libretto_core::PackageId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Plugin type identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginType {
    /// Native Rust plugin (.so/.dll/.dylib).
    Native,
    /// PHP plugin (Composer-compatible).
    Php,
}

/// Plugin capability flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginCapability {
    /// Can modify installation behavior.
    Install,
    /// Can modify dependency resolution.
    Resolve,
    /// Can provide custom commands.
    Command,
    /// Can hook into events.
    Event,
    /// Can provide custom repository types.
    Repository,
    /// Can modify autoloader generation.
    Autoload,
    /// Can modify downloads.
    Download,
    /// Can modify package sources.
    Source,
    /// Can modify scripts.
    Script,
}

impl PluginCapability {
    /// Get the hooks supported by this capability.
    #[must_use]
    pub fn supported_hooks(&self) -> Option<Vec<Hook>> {
        match self {
            Self::Install => Some(vec![
                Hook::PreInstallCmd,
                Hook::PostInstallCmd,
                Hook::PrePackageInstall,
                Hook::PostPackageInstall,
                Hook::PrePackageUninstall,
                Hook::PostPackageUninstall,
            ]),
            Self::Resolve => Some(vec![
                Hook::PreDependenciesSolving,
                Hook::PostDependenciesSolving,
            ]),
            Self::Event => Some(vec![
                Hook::PreInstallCmd,
                Hook::PostInstallCmd,
                Hook::PreUpdateCmd,
                Hook::PostUpdateCmd,
                Hook::PreAutoloadDump,
                Hook::PostAutoloadDump,
            ]),
            Self::Autoload => Some(vec![Hook::PreAutoloadDump, Hook::PostAutoloadDump]),
            Self::Download => Some(vec![Hook::PreFileDownload]),
            Self::Command => None, // Custom commands don't use standard hooks
            Self::Repository => None,
            Self::Source => Some(vec![Hook::PrePackageInstall, Hook::PostPackageInstall]),
            Self::Script => Some(vec![
                Hook::PreInstallCmd,
                Hook::PostInstallCmd,
                Hook::PreUpdateCmd,
                Hook::PostUpdateCmd,
            ]),
        }
    }

    /// Get all capabilities.
    #[must_use]
    pub fn all() -> Vec<Self> {
        vec![
            Self::Install,
            Self::Resolve,
            Self::Command,
            Self::Event,
            Self::Repository,
            Self::Autoload,
            Self::Download,
            Self::Source,
            Self::Script,
        ]
    }
}

/// Plugin metadata parsed from composer.json or plugin manifest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name (vendor/name format).
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Required plugin API version (e.g., "^2.0").
    #[serde(default, rename = "plugin-api-version")]
    pub plugin_api_version: Option<String>,
    /// PHP plugin class name (for Composer compatibility).
    #[serde(default)]
    pub class: Option<String>,
    /// Plugin capabilities.
    #[serde(default)]
    pub capabilities: Vec<PluginCapability>,
    /// Path to the plugin (directory or library file).
    #[serde(skip)]
    pub path: Option<PathBuf>,
    /// Plugin configuration from composer.json "extra".
    #[serde(skip)]
    pub config: Option<crate::config::PluginConfig>,
    /// Authors.
    #[serde(default)]
    pub authors: Vec<Author>,
    /// License.
    #[serde(default)]
    pub license: Option<String>,
    /// Homepage URL.
    #[serde(default)]
    pub homepage: Option<String>,
    /// Required Libretto version.
    #[serde(default, rename = "require-libretto")]
    pub require_libretto: Option<String>,
}

impl PluginMetadata {
    /// Create metadata from plugin info.
    #[must_use]
    pub fn from_info(info: &PluginInfo) -> Self {
        Self {
            name: info.name.clone(),
            version: info.version.clone(),
            description: info.description.clone(),
            plugin_api_version: info.api_version.clone(),
            capabilities: info.capabilities.clone(),
            ..Default::default()
        }
    }
}

/// Author information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Author {
    /// Author name.
    pub name: String,
    /// Author email.
    #[serde(default)]
    pub email: Option<String>,
    /// Author homepage.
    #[serde(default)]
    pub homepage: Option<String>,
}

/// Plugin information returned by plugins at runtime.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Description.
    pub description: String,
    /// API version this plugin targets.
    pub api_version: Option<String>,
    /// Plugin capabilities.
    pub capabilities: Vec<PluginCapability>,
    /// Hook priority (lower = earlier execution).
    pub priority: Option<i32>,
}

impl PluginInfo {
    /// Create a new plugin info builder.
    #[must_use]
    pub fn builder(name: impl Into<String>, version: impl Into<String>) -> PluginInfoBuilder {
        PluginInfoBuilder::new(name, version)
    }
}

/// Builder for `PluginInfo`.
#[derive(Debug)]
pub struct PluginInfoBuilder {
    name: String,
    version: String,
    description: String,
    api_version: Option<String>,
    capabilities: Vec<PluginCapability>,
    priority: Option<i32>,
}

impl PluginInfoBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: String::new(),
            api_version: None,
            capabilities: Vec::new(),
            priority: None,
        }
    }

    /// Set description.
    #[must_use]
    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set API version.
    #[must_use]
    pub fn api_version(mut self, version: impl Into<String>) -> Self {
        self.api_version = Some(version.into());
        self
    }

    /// Add a capability.
    #[must_use]
    pub fn capability(mut self, capability: PluginCapability) -> Self {
        self.capabilities.push(capability);
        self
    }

    /// Set capabilities.
    #[must_use]
    pub fn capabilities(mut self, capabilities: Vec<PluginCapability>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set priority.
    #[must_use]
    pub const fn priority(mut self, priority: i32) -> Self {
        self.priority = Some(priority);
        self
    }

    /// Build the plugin info.
    #[must_use]
    pub fn build(self) -> PluginInfo {
        PluginInfo {
            name: self.name,
            version: self.version,
            description: self.description,
            api_version: self.api_version,
            capabilities: self.capabilities,
            priority: self.priority,
        }
    }
}

/// Context passed to plugin event handlers.
#[derive(Debug, Clone, Default)]
pub struct EventContext {
    /// Packages being processed.
    pub packages: Vec<PackageId>,
    /// Current operation (install, update, etc.).
    pub operation: Option<String>,
    /// Project root directory.
    pub project_root: Option<PathBuf>,
    /// Vendor directory.
    pub vendor_dir: Option<PathBuf>,
    /// Additional data as key-value pairs.
    pub data: HashMap<String, String>,
    /// Command arguments (for command events).
    pub args: Vec<String>,
    /// Whether this is a dev operation.
    pub dev_mode: bool,
    /// Whether verbose output is enabled.
    pub verbose: bool,
    /// Download URLs (for pre-file-download).
    pub urls: Vec<String>,
    /// Package versions (for package events).
    pub versions: HashMap<String, String>,
}

impl EventContext {
    /// Create a new event context.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Set packages.
    #[must_use]
    pub fn with_packages(mut self, packages: Vec<PackageId>) -> Self {
        self.packages = packages;
        self
    }

    /// Set operation.
    #[must_use]
    pub fn with_operation(mut self, operation: impl Into<String>) -> Self {
        self.operation = Some(operation.into());
        self
    }

    /// Set project root.
    #[must_use]
    pub fn with_project_root(mut self, path: impl Into<PathBuf>) -> Self {
        self.project_root = Some(path.into());
        self
    }

    /// Set vendor directory.
    #[must_use]
    pub fn with_vendor_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.vendor_dir = Some(path.into());
        self
    }

    /// Add data.
    #[must_use]
    pub fn with_data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.data.insert(key.into(), value.into());
        self
    }

    /// Set dev mode.
    #[must_use]
    pub const fn with_dev_mode(mut self, dev_mode: bool) -> Self {
        self.dev_mode = dev_mode;
        self
    }

    /// Set verbose mode.
    #[must_use]
    pub const fn with_verbose(mut self, verbose: bool) -> Self {
        self.verbose = verbose;
        self
    }

    /// Add URL for download events.
    #[must_use]
    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.urls.push(url.into());
        self
    }

    /// Get a data value.
    #[must_use]
    pub fn get_data(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }
}

/// Result returned by plugin event handlers.
#[derive(Debug, Clone, Default)]
pub struct EventResult {
    /// Whether to continue processing other handlers.
    pub continue_processing: bool,
    /// Messages to display.
    pub messages: Vec<String>,
    /// Modified data (e.g., modified download URL).
    pub modified_data: HashMap<String, String>,
    /// Error message if processing failed.
    pub error: Option<String>,
    /// Warnings generated during processing.
    pub warnings: Vec<String>,
}

impl EventResult {
    /// Create a success result.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            continue_processing: true,
            messages: Vec::new(),
            modified_data: HashMap::new(),
            error: None,
            warnings: Vec::new(),
        }
    }

    /// Create a result with a message.
    #[must_use]
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            continue_processing: true,
            messages: vec![message.into()],
            modified_data: HashMap::new(),
            error: None,
            warnings: Vec::new(),
        }
    }

    /// Create a stop result (prevents further processing).
    #[must_use]
    pub fn stop() -> Self {
        Self {
            continue_processing: false,
            messages: Vec::new(),
            modified_data: HashMap::new(),
            error: None,
            warnings: Vec::new(),
        }
    }

    /// Create an error result.
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            continue_processing: false,
            messages: Vec::new(),
            modified_data: HashMap::new(),
            error: Some(message.into()),
            warnings: Vec::new(),
        }
    }

    /// Add a message.
    #[must_use]
    pub fn message(mut self, message: impl Into<String>) -> Self {
        self.messages.push(message.into());
        self
    }

    /// Add a warning.
    #[must_use]
    pub fn warning(mut self, warning: impl Into<String>) -> Self {
        self.warnings.push(warning.into());
        self
    }

    /// Add modified data.
    #[must_use]
    pub fn data(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.modified_data.insert(key.into(), value.into());
        self
    }

    /// Stop processing.
    #[must_use]
    pub const fn stop_processing(mut self) -> Self {
        self.continue_processing = false;
        self
    }

    /// Check if result is an error.
    #[must_use]
    pub const fn is_error(&self) -> bool {
        self.error.is_some()
    }

    /// Check if result has warnings.
    #[must_use]
    pub const fn has_warnings(&self) -> bool {
        !self.warnings.is_empty()
    }
}

/// Main plugin trait for native Rust plugins.
///
/// Plugins implement this trait to provide functionality.
/// The trait is object-safe and uses async methods.
#[async_trait::async_trait]
pub trait Plugin: Send + Sync {
    /// Get plugin information.
    fn info(&self) -> PluginInfo;

    /// Get plugin capabilities.
    fn capabilities(&self) -> Vec<PluginCapability>;

    /// Called when plugin is activated.
    async fn activate(&self) -> Result<()> {
        Ok(())
    }

    /// Called when plugin is deactivated.
    async fn deactivate(&self) -> Result<()> {
        Ok(())
    }

    /// Called when plugin is being uninstalled.
    async fn uninstall(&self) -> Result<()> {
        Ok(())
    }

    /// Handle an event.
    async fn on_event(&self, event: Hook, context: &EventContext) -> Result<EventResult>;

    /// Get custom commands provided by this plugin.
    fn commands(&self) -> Vec<PluginCommand> {
        Vec::new()
    }

    /// Get custom repository types provided by this plugin.
    fn repository_types(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Custom command definition.
#[derive(Debug, Clone)]
pub struct PluginCommand {
    /// Command name.
    pub name: String,
    /// Command description.
    pub description: String,
    /// Command aliases.
    pub aliases: Vec<String>,
    /// Command arguments definition.
    pub arguments: Vec<CommandArgument>,
    /// Command options definition.
    pub options: Vec<CommandOption>,
}

/// Command argument definition.
#[derive(Debug, Clone)]
pub struct CommandArgument {
    /// Argument name.
    pub name: String,
    /// Argument description.
    pub description: String,
    /// Whether the argument is required.
    pub required: bool,
    /// Default value.
    pub default: Option<String>,
}

/// Command option definition.
#[derive(Debug, Clone)]
pub struct CommandOption {
    /// Option name (long form).
    pub name: String,
    /// Short form (single character).
    pub short: Option<char>,
    /// Option description.
    pub description: String,
    /// Whether the option takes a value.
    pub takes_value: bool,
    /// Default value.
    pub default: Option<String>,
}

/// Stable C ABI for plugin entry points.
///
/// Native plugins must export these functions with `extern "C"`.
#[allow(unsafe_code)]
pub mod ffi {
    use super::{PluginCapability, PluginInfo};

    /// Plugin version for ABI compatibility checking.
    pub const PLUGIN_ABI_VERSION: u32 = 1;

    /// Plugin entry point function signature.
    pub type PluginCreateFn = unsafe extern "C" fn() -> *mut std::ffi::c_void;

    /// Plugin destruction function signature.
    pub type PluginDestroyFn = unsafe extern "C" fn(*mut std::ffi::c_void);

    /// Plugin ABI version function signature.
    pub type PluginAbiVersionFn = unsafe extern "C" fn() -> u32;

    /// Plugin info function signature.
    pub type PluginInfoFn = unsafe extern "C" fn() -> PluginInfoFFI;

    /// FFI-safe plugin info.
    #[repr(C)]
    #[derive(Debug)]
    pub struct PluginInfoFFI {
        /// Plugin name (null-terminated).
        pub name: *const std::ffi::c_char,
        /// Plugin version (null-terminated).
        pub version: *const std::ffi::c_char,
        /// Description (null-terminated).
        pub description: *const std::ffi::c_char,
        /// API version (null-terminated, may be null).
        pub api_version: *const std::ffi::c_char,
        /// Capability flags as a bitmask.
        pub capabilities: u32,
        /// Priority.
        pub priority: i32,
    }

    impl PluginInfoFFI {
        /// Convert to safe Rust `PluginInfo`.
        ///
        /// # Safety
        /// The pointers must be valid and null-terminated.
        #[must_use]
        pub unsafe fn to_plugin_info(&self) -> PluginInfo {
            let name = if self.name.is_null() {
                String::new()
            } else {
                // SAFETY: caller guarantees pointer is valid
                unsafe {
                    std::ffi::CStr::from_ptr(self.name)
                        .to_string_lossy()
                        .into_owned()
                }
            };

            let version = if self.version.is_null() {
                String::new()
            } else {
                // SAFETY: caller guarantees pointer is valid
                unsafe {
                    std::ffi::CStr::from_ptr(self.version)
                        .to_string_lossy()
                        .into_owned()
                }
            };

            let description = if self.description.is_null() {
                String::new()
            } else {
                // SAFETY: caller guarantees pointer is valid
                unsafe {
                    std::ffi::CStr::from_ptr(self.description)
                        .to_string_lossy()
                        .into_owned()
                }
            };

            let api_version = if self.api_version.is_null() {
                None
            } else {
                // SAFETY: caller guarantees pointer is valid
                Some(unsafe {
                    std::ffi::CStr::from_ptr(self.api_version)
                        .to_string_lossy()
                        .into_owned()
                })
            };

            let capabilities = capabilities_from_bitmask(self.capabilities);

            PluginInfo {
                name,
                version,
                description,
                api_version,
                capabilities,
                priority: Some(self.priority),
            }
        }
    }

    /// Convert capability bitmask to vector.
    #[must_use]
    pub fn capabilities_from_bitmask(mask: u32) -> Vec<PluginCapability> {
        let mut caps = Vec::new();
        if mask & 0x01 != 0 {
            caps.push(PluginCapability::Install);
        }
        if mask & 0x02 != 0 {
            caps.push(PluginCapability::Resolve);
        }
        if mask & 0x04 != 0 {
            caps.push(PluginCapability::Command);
        }
        if mask & 0x08 != 0 {
            caps.push(PluginCapability::Event);
        }
        if mask & 0x10 != 0 {
            caps.push(PluginCapability::Repository);
        }
        if mask & 0x20 != 0 {
            caps.push(PluginCapability::Autoload);
        }
        if mask & 0x40 != 0 {
            caps.push(PluginCapability::Download);
        }
        if mask & 0x80 != 0 {
            caps.push(PluginCapability::Source);
        }
        if mask & 0x100 != 0 {
            caps.push(PluginCapability::Script);
        }
        caps
    }

    /// Convert capabilities to bitmask.
    #[must_use]
    #[allow(dead_code)]
    pub fn capabilities_to_bitmask(caps: &[PluginCapability]) -> u32 {
        let mut mask = 0u32;
        for cap in caps {
            mask |= match cap {
                PluginCapability::Install => 0x01,
                PluginCapability::Resolve => 0x02,
                PluginCapability::Command => 0x04,
                PluginCapability::Event => 0x08,
                PluginCapability::Repository => 0x10,
                PluginCapability::Autoload => 0x20,
                PluginCapability::Download => 0x40,
                PluginCapability::Source => 0x80,
                PluginCapability::Script => 0x100,
            };
        }
        mask
    }
}

/// Plugin API for dependency injection.
///
/// This provides services that plugins can use.
#[derive(Debug, Clone)]
pub struct PluginApi {
    /// Project root directory.
    pub project_root: PathBuf,
    /// Vendor directory.
    pub vendor_dir: PathBuf,
    /// Composer home directory.
    pub composer_home: PathBuf,
    /// Event bus for plugin communication.
    event_bus: Arc<crate::event_bus::EventBus>,
}

impl PluginApi {
    /// Create a new plugin API instance.
    #[must_use]
    pub const fn new(
        project_root: PathBuf,
        vendor_dir: PathBuf,
        composer_home: PathBuf,
        event_bus: Arc<crate::event_bus::EventBus>,
    ) -> Self {
        Self {
            project_root,
            vendor_dir,
            composer_home,
            event_bus,
        }
    }

    /// Get the project root directory.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Get the vendor directory.
    #[must_use]
    pub fn vendor_dir(&self) -> &Path {
        &self.vendor_dir
    }

    /// Get the composer home directory.
    #[must_use]
    pub fn composer_home(&self) -> &Path {
        &self.composer_home
    }

    /// Send a message to other plugins via the event bus.
    pub fn send_message(&self, message: crate::event_bus::EventMessage) -> Result<()> {
        self.event_bus.publish(message)
    }

    /// Subscribe to messages from other plugins.
    #[must_use]
    pub fn subscribe(&self) -> crate::event_bus::EventSubscription {
        self.event_bus.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_info_builder() {
        let info = PluginInfo::builder("test/plugin", "1.0.0")
            .description("A test plugin")
            .capability(PluginCapability::Event)
            .priority(10)
            .build();

        assert_eq!(info.name, "test/plugin");
        assert_eq!(info.version, "1.0.0");
        assert_eq!(info.description, "A test plugin");
        assert_eq!(info.priority, Some(10));
        assert!(info.capabilities.contains(&PluginCapability::Event));
    }

    #[test]
    fn event_context_builder() {
        let ctx = EventContext::new()
            .with_operation("install")
            .with_dev_mode(true)
            .with_data("key", "value");

        assert_eq!(ctx.operation, Some("install".into()));
        assert!(ctx.dev_mode);
        assert_eq!(ctx.get_data("key"), Some(&"value".to_string()));
    }

    #[test]
    fn event_result_states() {
        let ok = EventResult::ok();
        assert!(ok.continue_processing);
        assert!(!ok.is_error());

        let stop = EventResult::stop();
        assert!(!stop.continue_processing);

        let err = EventResult::error("failed");
        assert!(err.is_error());
        assert!(!err.continue_processing);
    }

    #[test]
    fn capability_bitmask() {
        let caps = vec![PluginCapability::Install, PluginCapability::Event];
        let mask = ffi::capabilities_to_bitmask(&caps);
        let restored = ffi::capabilities_from_bitmask(mask);

        assert!(restored.contains(&PluginCapability::Install));
        assert!(restored.contains(&PluginCapability::Event));
        assert!(!restored.contains(&PluginCapability::Command));
    }

    #[test]
    fn capability_supported_hooks() {
        let install_hooks = PluginCapability::Install.supported_hooks();
        assert!(install_hooks.is_some());
        let hooks = install_hooks.unwrap();
        assert!(hooks.contains(&Hook::PreInstallCmd));
        assert!(hooks.contains(&Hook::PostInstallCmd));
    }
}
