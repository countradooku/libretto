//! Plugin system for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use libretto_core::PackageId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

/// Plugin capability flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginCapability {
    /// Can modify installation behavior.
    Install,
    /// Can modify resolution behavior.
    Resolve,
    /// Can provide custom commands.
    Command,
    /// Can hook into events.
    Event,
    /// Can provide custom repository types.
    Repository,
}

/// Plugin metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    /// Plugin name.
    pub name: String,
    /// Plugin version.
    pub version: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Required Libretto version.
    #[serde(default)]
    pub require_libretto: Option<String>,
    /// Plugin class name (for PHP plugins).
    #[serde(default)]
    pub class: Option<String>,
}

/// Event types that plugins can hook into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PluginEvent {
    /// Before resolution starts.
    PreResolve,
    /// After resolution completes.
    PostResolve,
    /// Before package installation.
    PreInstall,
    /// After package installation.
    PostInstall,
    /// Before package update.
    PreUpdate,
    /// After package update.
    PostUpdate,
    /// Before autoloader generation.
    PreAutoload,
    /// After autoloader generation.
    PostAutoload,
}

/// Plugin event handler result.
#[derive(Debug, Clone, Default)]
pub struct EventResult {
    /// Whether to continue processing.
    pub continue_processing: bool,
    /// Messages to display.
    pub messages: Vec<String>,
}

impl EventResult {
    /// Create success result.
    #[must_use]
    pub fn ok() -> Self {
        Self {
            continue_processing: true,
            messages: Vec::new(),
        }
    }

    /// Create result with message.
    #[must_use]
    pub fn with_message(message: impl Into<String>) -> Self {
        Self {
            continue_processing: true,
            messages: vec![message.into()],
        }
    }

    /// Create stop result.
    #[must_use]
    pub fn stop() -> Self {
        Self {
            continue_processing: false,
            messages: Vec::new(),
        }
    }
}

/// Plugin trait for native plugins.
pub trait Plugin: Send + Sync {
    /// Get plugin metadata.
    fn metadata(&self) -> &PluginMetadata;

    /// Get plugin capabilities.
    fn capabilities(&self) -> Vec<PluginCapability>;

    /// Handle an event.
    fn on_event(&self, event: PluginEvent, context: &EventContext) -> EventResult;
}

/// Context passed to plugin event handlers.
#[derive(Debug, Default)]
pub struct EventContext {
    /// Packages being processed.
    pub packages: Vec<PackageId>,
    /// Additional data.
    pub data: HashMap<String, String>,
}

/// Loaded plugin instance.
struct LoadedPlugin {
    metadata: PluginMetadata,
    instance: Box<dyn Plugin>,
}

impl std::fmt::Debug for LoadedPlugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LoadedPlugin")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}

/// Plugin manager.
#[derive(Debug, Default)]
pub struct PluginManager {
    plugins: Vec<LoadedPlugin>,
    search_paths: Vec<PathBuf>,
}

impl PluginManager {
    /// Create new plugin manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Add search path for plugins.
    pub fn add_search_path(&mut self, path: PathBuf) {
        self.search_paths.push(path);
    }

    /// Register a native plugin.
    pub fn register(&mut self, plugin: Box<dyn Plugin>) {
        let metadata = plugin.metadata().clone();
        info!(name = %metadata.name, "registered plugin");
        self.plugins.push(LoadedPlugin {
            metadata,
            instance: plugin,
        });
    }

    /// Get all loaded plugins.
    #[must_use]
    pub fn plugins(&self) -> Vec<&PluginMetadata> {
        self.plugins.iter().map(|p| &p.metadata).collect()
    }

    /// Get search paths.
    #[must_use]
    pub fn search_paths(&self) -> &[PathBuf] {
        &self.search_paths
    }

    /// Emit an event to all plugins.
    #[must_use]
    pub fn emit(&self, event: PluginEvent, context: &EventContext) -> Vec<EventResult> {
        self.plugins
            .iter()
            .map(|p| p.instance.on_event(event, context))
            .collect()
    }

    /// Check if any plugin has a capability.
    #[must_use]
    pub fn has_capability(&self, capability: PluginCapability) -> bool {
        self.plugins
            .iter()
            .any(|p| p.instance.capabilities().contains(&capability))
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
    fn event_result() {
        let result = EventResult::ok();
        assert!(result.continue_processing);

        let result = EventResult::stop();
        assert!(!result.continue_processing);
    }

    #[test]
    fn event_context_default() {
        let ctx = EventContext::default();
        assert!(ctx.packages.is_empty());
        assert!(ctx.data.is_empty());
    }
}
