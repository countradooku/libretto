//! High-performance plugin system for Libretto.
//!
//! This crate provides a comprehensive plugin system supporting both native Rust plugins
//! and PHP plugins (for Composer compatibility). Features include:
//!
//! - **Native Rust Plugins**: Dynamic library loading via `libloading` with stable C ABI
//! - **PHP Plugin Support**: IPC via Unix sockets (Unix) or named pipes (Windows)
//! - **Plugin Discovery**: Automatic scanning of vendor directory for composer plugins
//! - **Composer-Compatible Hooks**: Full support for all Composer plugin events
//! - **Plugin Sandboxing**: File system restrictions, network monitoring, timeouts
//! - **Event Bus**: Plugin-to-plugin communication via crossbeam channels
//! - **Hot Reloading**: Development mode support for native plugins
//!
//! # Performance Targets
//!
//! - Plugin invocation overhead: <10ms
//! - Support for 20+ simultaneous plugins
//! - Lazy loading for minimal startup impact
//!
//! # Example
//!
//! ```rust,ignore
//! use libretto_plugin_system::{PluginManager, PluginEvent, EventContext};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let mut manager = PluginManager::new();
//!
//!     // Discover plugins in vendor directory
//!     manager.discover("./vendor").await?;
//!
//!     // Emit an event to all plugins
//!     let results = manager.emit(PluginEvent::PreInstall, &EventContext::default()).await?;
//!
//!     Ok(())
//! }
//! ```

#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

mod api;
mod config;
mod discovery;
mod error;
mod event_bus;
mod hooks;
mod lifecycle;
mod native;
mod php;
mod sandbox;

pub use api::{
    EventContext, EventResult, Plugin, PluginApi, PluginCapability, PluginInfo, PluginMetadata,
    PluginType,
};
pub use config::{PluginConfig, PluginManagerConfig};
pub use discovery::PluginDiscovery;
pub use error::{PluginError, Result};
pub use event_bus::{EventBus, EventMessage, EventSubscription, MessagePayload};
pub use hooks::{Hook, HookPriority, HookRegistry};
pub use lifecycle::{PluginLifecycle, PluginState};
pub use native::{NativePlugin, NativePluginLoader};
pub use php::{PhpPlugin, PhpPluginBridge};
pub use sandbox::{Sandbox, SandboxConfig, SandboxViolation};

use dashmap::DashMap;
use moka::sync::Cache;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, instrument, warn};

/// Default plugin timeout (30 seconds).
pub const DEFAULT_PLUGIN_TIMEOUT: Duration = Duration::from_secs(30);

/// Default maximum concurrent plugins.
pub const DEFAULT_MAX_CONCURRENT_PLUGINS: usize = 20;

/// Plugin API version for compatibility checking.
pub const PLUGIN_API_VERSION: &str = "2.6.0";

/// Global plugin metadata cache for fast lookups.
static METADATA_CACHE: std::sync::LazyLock<Cache<PathBuf, Arc<PluginMetadata>>> =
    std::sync::LazyLock::new(|| {
        Cache::builder()
            .max_capacity(1000)
            .time_to_live(Duration::from_secs(3600))
            .build()
    });

/// Plugin handle for managing loaded plugins.
#[derive(Debug, Clone)]
pub struct PluginHandle {
    /// Plugin identifier.
    pub id: String,
    /// Plugin metadata.
    pub metadata: Arc<PluginMetadata>,
    /// Current state.
    pub state: Arc<RwLock<PluginState>>,
    /// Plugin type (native or PHP).
    pub plugin_type: PluginType,
    /// Priority for hook ordering.
    pub priority: i32,
    /// Whether the plugin is enabled.
    pub enabled: Arc<RwLock<bool>>,
}

impl PluginHandle {
    /// Create a new plugin handle.
    #[must_use]
    pub fn new(metadata: PluginMetadata, plugin_type: PluginType) -> Self {
        let id = metadata.name.clone();
        Self {
            id,
            metadata: Arc::new(metadata),
            state: Arc::new(RwLock::new(PluginState::Unloaded)),
            plugin_type,
            priority: 0,
            enabled: Arc::new(RwLock::new(true)),
        }
    }

    /// Check if plugin is loaded.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        matches!(
            *self.state.read(),
            PluginState::Loaded | PluginState::Active
        )
    }

    /// Check if plugin is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        *self.enabled.read()
    }

    /// Enable the plugin.
    pub fn enable(&self) {
        *self.enabled.write() = true;
    }

    /// Disable the plugin.
    pub fn disable(&self) {
        *self.enabled.write() = false;
    }
}

/// Main plugin manager coordinating all plugin operations.
pub struct PluginManager {
    /// Loaded plugin handles.
    plugins: DashMap<String, PluginHandle>,
    /// Native plugin loader.
    native_loader: NativePluginLoader,
    /// PHP plugin bridge.
    php_bridge: PhpPluginBridge,
    /// Plugin discovery.
    discovery: PluginDiscovery,
    /// Hook registry.
    hook_registry: Arc<HookRegistry>,
    /// Event bus for plugin communication.
    event_bus: Arc<EventBus>,
    /// Sandbox configuration.
    sandbox_config: SandboxConfig,
    /// Manager configuration.
    config: PluginManagerConfig,
    /// Plugin configurations from composer.json "extra".
    plugin_configs: DashMap<String, PluginConfig>,
    /// Native plugin instances (kept alive).
    native_instances: DashMap<String, Arc<dyn Plugin>>,
    /// PHP plugin instances.
    php_instances: DashMap<String, Arc<PhpPlugin>>,
}

impl std::fmt::Debug for PluginManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PluginManager")
            .field("plugins_count", &self.plugins.len())
            .field("native_instances_count", &self.native_instances.len())
            .field("php_instances_count", &self.php_instances.len())
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl PluginManager {
    /// Create a new plugin manager with default configuration.
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(PluginManagerConfig::default())
    }

    /// Create a new plugin manager with custom configuration.
    #[must_use]
    pub fn with_config(config: PluginManagerConfig) -> Self {
        let event_bus = Arc::new(EventBus::new(config.event_bus_capacity));
        let hook_registry = Arc::new(HookRegistry::new());

        Self {
            plugins: DashMap::new(),
            native_loader: NativePluginLoader::new(config.hot_reload),
            php_bridge: PhpPluginBridge::new(config.php_timeout),
            discovery: PluginDiscovery::new(),
            hook_registry,
            event_bus,
            sandbox_config: config.sandbox.clone(),
            config,
            plugin_configs: DashMap::new(),
            native_instances: DashMap::new(),
            php_instances: DashMap::new(),
        }
    }

    /// Discover plugins in the vendor directory.
    ///
    /// # Errors
    /// Returns error if discovery fails.
    #[instrument(skip(self), fields(vendor_path = %vendor_path.as_ref().display()))]
    pub async fn discover(&mut self, vendor_path: impl AsRef<Path>) -> Result<Vec<PluginHandle>> {
        let vendor_path = vendor_path.as_ref();
        info!("discovering plugins in vendor directory");

        let discovered = self.discovery.scan_vendor(vendor_path).await?;

        let mut handles = Vec::with_capacity(discovered.len());

        for metadata in discovered {
            // Check API version compatibility
            if !self.is_api_compatible(&metadata) {
                warn!(
                    plugin = %metadata.name,
                    required = ?metadata.plugin_api_version,
                    "skipping incompatible plugin"
                );
                continue;
            }

            let plugin_type = if metadata.class.is_some() {
                PluginType::Php
            } else {
                PluginType::Native
            };

            let handle = PluginHandle::new(metadata.clone(), plugin_type);

            // Cache metadata for fast lookups
            if let Some(path) = &metadata.path {
                METADATA_CACHE.insert(path.clone(), Arc::new(metadata.clone()));
            }

            // Store plugin configuration if present
            if let Some(config) = &metadata.config {
                self.plugin_configs
                    .insert(metadata.name.clone(), config.clone());
            }

            self.plugins.insert(handle.id.clone(), handle.clone());
            handles.push(handle);
        }

        info!(count = handles.len(), "discovered plugins");
        Ok(handles)
    }

    /// Load a specific plugin by ID.
    ///
    /// # Errors
    /// Returns error if loading fails.
    #[instrument(skip(self))]
    pub async fn load(&self, plugin_id: &str) -> Result<()> {
        let handle = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        if !handle.is_enabled() {
            return Err(PluginError::Disabled(plugin_id.to_string()));
        }

        // Update state to loading
        *handle.state.write() = PluginState::Loading;

        let result = match handle.plugin_type {
            PluginType::Native => self.load_native_plugin(&handle).await,
            PluginType::Php => self.load_php_plugin(&handle).await,
        };

        match result {
            Ok(()) => {
                *handle.state.write() = PluginState::Loaded;
                info!(plugin = plugin_id, "plugin loaded successfully");
                Ok(())
            }
            Err(e) => {
                *handle.state.write() = PluginState::Error(e.to_string());
                Err(e)
            }
        }
    }

    /// Load all discovered plugins.
    ///
    /// # Errors
    /// Returns error if any critical plugin fails to load.
    pub async fn load_all(&self) -> Result<()> {
        let plugin_ids: Vec<String> = self.plugins.iter().map(|p| p.key().clone()).collect();

        for plugin_id in plugin_ids {
            if let Err(e) = self.load(&plugin_id).await {
                warn!(plugin = %plugin_id, error = %e, "failed to load plugin");
                // Continue loading other plugins
            }
        }

        Ok(())
    }

    /// Unload a specific plugin.
    ///
    /// # Errors
    /// Returns error if unloading fails.
    #[instrument(skip(self))]
    pub async fn unload(&self, plugin_id: &str) -> Result<()> {
        let handle = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        *handle.state.write() = PluginState::Unloading;

        // Call plugin's cleanup
        match handle.plugin_type {
            PluginType::Native => {
                if let Some((_, plugin)) = self.native_instances.remove(plugin_id) {
                    plugin.deactivate().await?;
                    plugin.uninstall().await?;
                }
            }
            PluginType::Php => {
                if let Some((_, plugin)) = self.php_instances.remove(plugin_id) {
                    plugin.stop().await?;
                }
            }
        }

        // Unregister hooks
        self.hook_registry.unregister_plugin(plugin_id);

        *handle.state.write() = PluginState::Unloaded;
        info!(plugin = plugin_id, "plugin unloaded");

        Ok(())
    }

    /// Emit an event to all loaded plugins.
    ///
    /// # Errors
    /// Returns error if event processing fails.
    #[instrument(skip(self, context))]
    pub async fn emit(&self, event: Hook, context: &EventContext) -> Result<Vec<EventResult>> {
        let start = std::time::Instant::now();

        // Get handlers sorted by priority
        let handlers = self.hook_registry.get_handlers(&event);

        let mut results = Vec::with_capacity(handlers.len());
        let sandbox = Sandbox::new(self.sandbox_config.clone());

        for handler in handlers {
            // Check if plugin is still enabled
            if let Some(handle) = self.plugins.get(&handler.plugin_id)
                && (!handle.is_enabled() || !handle.is_loaded())
            {
                continue;
            }

            // Execute with timeout and sandbox
            let result = sandbox
                .execute_with_timeout(
                    self.config.default_timeout,
                    self.invoke_handler(&handler, event, context),
                )
                .await;

            match result {
                Ok(event_result) => {
                    if !event_result.continue_processing {
                        results.push(event_result);
                        break;
                    }
                    results.push(event_result);
                }
                Err(e) => {
                    warn!(
                        plugin = %handler.plugin_id,
                        hook = ?event,
                        error = %e,
                        "plugin handler failed"
                    );
                    results.push(EventResult::error(e.to_string()));
                }
            }
        }

        let elapsed = start.elapsed();
        debug!(
            hook = ?event,
            handlers = results.len(),
            elapsed_ms = elapsed.as_millis(),
            "event processing complete"
        );

        Ok(results)
    }

    /// Register a native plugin manually.
    pub fn register_native(&self, plugin: Arc<dyn Plugin>) {
        let info = plugin.info();
        let metadata = PluginMetadata::from_info(&info);
        let handle = PluginHandle::new(metadata, PluginType::Native);

        // Register hooks
        for capability in plugin.capabilities() {
            if let Some(hooks) = capability.supported_hooks() {
                for hook in hooks {
                    self.hook_registry.register(
                        hook,
                        handle.id.clone(),
                        info.priority.unwrap_or(0),
                    );
                }
            }
        }

        self.native_instances.insert(handle.id.clone(), plugin);
        self.plugins.insert(handle.id.clone(), handle);
    }

    /// Get all loaded plugins.
    #[must_use]
    pub fn plugins(&self) -> Vec<PluginHandle> {
        self.plugins.iter().map(|p| p.value().clone()).collect()
    }

    /// Get a specific plugin by ID.
    #[must_use]
    pub fn get(&self, plugin_id: &str) -> Option<PluginHandle> {
        self.plugins.get(plugin_id).map(|p| p.value().clone())
    }

    /// Check if plugins are enabled globally.
    #[must_use]
    pub const fn plugins_enabled(&self) -> bool {
        self.config.plugins_enabled
    }

    /// Disable all plugins (--no-plugins flag).
    pub fn disable_all(&self) {
        for plugin in self.plugins.iter_mut() {
            plugin.disable();
        }
    }

    /// Enable all plugins.
    pub fn enable_all(&self) {
        for plugin in self.plugins.iter_mut() {
            plugin.enable();
        }
    }

    /// Get the event bus for plugin-to-plugin communication.
    #[must_use]
    pub fn event_bus(&self) -> Arc<EventBus> {
        Arc::clone(&self.event_bus)
    }

    /// Get plugin configuration.
    #[must_use]
    pub fn get_plugin_config(&self, plugin_id: &str) -> Option<PluginConfig> {
        self.plugin_configs.get(plugin_id).map(|c| c.clone())
    }

    /// Hot reload a native plugin (development mode only).
    ///
    /// # Errors
    /// Returns error if hot reload fails or is not enabled.
    #[instrument(skip(self))]
    pub async fn hot_reload(&self, plugin_id: &str) -> Result<()> {
        if !self.config.hot_reload {
            return Err(PluginError::HotReloadDisabled);
        }

        let handle = self
            .plugins
            .get(plugin_id)
            .ok_or_else(|| PluginError::NotFound(plugin_id.to_string()))?;

        if handle.plugin_type != PluginType::Native {
            return Err(PluginError::InvalidOperation(
                "hot reload only supported for native plugins".into(),
            ));
        }

        // Unload the plugin
        self.unload(plugin_id).await?;

        // Reload from disk
        self.load(plugin_id).await?;

        info!(plugin = plugin_id, "plugin hot reloaded");
        Ok(())
    }

    // Private helper methods

    async fn load_native_plugin(&self, handle: &PluginHandle) -> Result<()> {
        let path = handle
            .metadata
            .path
            .as_ref()
            .ok_or_else(|| PluginError::InvalidMetadata("missing plugin path".into()))?;

        let plugin = self.native_loader.load(path)?;

        // Initialize and activate
        plugin.activate().await?;

        // Register hooks
        for capability in plugin.capabilities() {
            if let Some(hooks) = capability.supported_hooks() {
                for hook in hooks {
                    self.hook_registry
                        .register(hook, handle.id.clone(), handle.priority);
                }
            }
        }

        self.native_instances
            .insert(handle.id.clone(), Arc::from(plugin));
        Ok(())
    }

    async fn load_php_plugin(&self, handle: &PluginHandle) -> Result<()> {
        let class = handle
            .metadata
            .class
            .as_ref()
            .ok_or_else(|| PluginError::InvalidMetadata("missing plugin class".into()))?;

        let path = handle
            .metadata
            .path
            .as_ref()
            .ok_or_else(|| PluginError::InvalidMetadata("missing plugin path".into()))?;

        let php_plugin = self.php_bridge.create_plugin(class, path).await?;

        // Start the PHP process
        php_plugin.start().await?;

        // Get capabilities and register hooks
        let capabilities = php_plugin.get_capabilities().await?;
        for capability in capabilities {
            if let Some(hooks) = capability.supported_hooks() {
                for hook in hooks {
                    self.hook_registry
                        .register(hook, handle.id.clone(), handle.priority);
                }
            }
        }

        self.php_instances
            .insert(handle.id.clone(), Arc::new(php_plugin));
        Ok(())
    }

    async fn invoke_handler(
        &self,
        handler: &hooks::HookHandler,
        event: Hook,
        context: &EventContext,
    ) -> Result<EventResult> {
        // Try native first
        if let Some(plugin) = self.native_instances.get(&handler.plugin_id) {
            return plugin.on_event(event, context).await;
        }

        // Try PHP
        if let Some(plugin) = self.php_instances.get(&handler.plugin_id) {
            return plugin.invoke(event, context).await;
        }

        Err(PluginError::NotLoaded(handler.plugin_id.clone()))
    }

    fn is_api_compatible(&self, metadata: &PluginMetadata) -> bool {
        let Some(required) = &metadata.plugin_api_version else {
            return true; // No version requirement
        };

        // Parse versions
        let Ok(required_ver) = semver::VersionReq::parse(required) else {
            warn!(
                plugin = %metadata.name,
                version = %required,
                "invalid plugin-api-version constraint"
            );
            return false;
        };

        let Ok(current_ver) = semver::Version::parse(PLUGIN_API_VERSION) else {
            return false;
        };

        required_ver.matches(&current_ver)
    }
}

impl Default for PluginManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_manager_creation() {
        let manager = PluginManager::new();
        assert!(manager.plugins().is_empty());
    }

    #[test]
    fn plugin_handle_state() {
        let metadata = PluginMetadata {
            name: "test-plugin".into(),
            version: "1.0.0".into(),
            ..Default::default()
        };
        let handle = PluginHandle::new(metadata, PluginType::Native);

        assert!(!handle.is_loaded());
        assert!(handle.is_enabled());

        handle.disable();
        assert!(!handle.is_enabled());

        handle.enable();
        assert!(handle.is_enabled());
    }

    #[test]
    fn api_version_compatibility() {
        let manager = PluginManager::new();

        let compatible = PluginMetadata {
            name: "test".into(),
            version: "1.0.0".into(),
            plugin_api_version: Some("^2.0.0".into()),
            ..Default::default()
        };
        assert!(manager.is_api_compatible(&compatible));

        let incompatible = PluginMetadata {
            name: "test".into(),
            version: "1.0.0".into(),
            plugin_api_version: Some("^3.0.0".into()),
            ..Default::default()
        };
        assert!(!manager.is_api_compatible(&incompatible));
    }

    #[tokio::test]
    async fn event_emission() {
        let manager = PluginManager::new();
        let context = EventContext::default();

        let results = manager.emit(Hook::PreInstallCmd, &context).await.unwrap();
        assert!(results.is_empty()); // No plugins loaded
    }
}
