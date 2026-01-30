//! Plugin configuration types.
//!
//! This module provides configuration structures for the plugin system,
//! including per-plugin configuration from composer.json "extra" section.

use crate::sandbox::SandboxConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Plugin manager configuration.
#[derive(Debug, Clone)]
pub struct PluginManagerConfig {
    /// Enable plugins globally.
    pub plugins_enabled: bool,

    /// Enable hot reload for native plugins (development mode).
    pub hot_reload: bool,

    /// Default timeout for plugin operations.
    pub default_timeout: Duration,

    /// Maximum concurrent plugin operations.
    pub max_concurrent: usize,

    /// Sandbox configuration.
    pub sandbox: SandboxConfig,

    /// Event bus capacity.
    pub event_bus_capacity: usize,

    /// PHP plugin timeout.
    pub php_timeout: Duration,

    /// Enable lazy loading.
    pub lazy_loading: bool,

    /// Plugin loading order (explicit ordering).
    pub load_order: Vec<String>,

    /// Disabled plugins.
    pub disabled_plugins: Vec<String>,
}

impl Default for PluginManagerConfig {
    fn default() -> Self {
        Self {
            plugins_enabled: true,
            hot_reload: false,
            default_timeout: Duration::from_secs(30),
            max_concurrent: 20,
            sandbox: SandboxConfig::default(),
            event_bus_capacity: 1000,
            php_timeout: Duration::from_secs(60),
            lazy_loading: true,
            load_order: Vec::new(),
            disabled_plugins: Vec::new(),
        }
    }
}

impl PluginManagerConfig {
    /// Create a new configuration with defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable hot reload (for development).
    #[must_use]
    pub const fn with_hot_reload(mut self, enabled: bool) -> Self {
        self.hot_reload = enabled;
        self
    }

    /// Set default timeout.
    #[must_use]
    pub const fn with_timeout(mut self, timeout: Duration) -> Self {
        self.default_timeout = timeout;
        self
    }

    /// Set maximum concurrent operations.
    #[must_use]
    pub const fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max;
        self
    }

    /// Disable plugins.
    #[must_use]
    pub fn with_disabled_plugins(mut self, plugins: Vec<String>) -> Self {
        self.disabled_plugins = plugins;
        self
    }

    /// Set sandbox configuration.
    #[must_use]
    pub fn with_sandbox(mut self, sandbox: SandboxConfig) -> Self {
        self.sandbox = sandbox;
        self
    }

    /// Enable lazy loading.
    #[must_use]
    pub const fn with_lazy_loading(mut self, enabled: bool) -> Self {
        self.lazy_loading = enabled;
        self
    }

    /// Check if a plugin is disabled.
    #[must_use]
    pub fn is_plugin_disabled(&self, plugin_id: &str) -> bool {
        self.disabled_plugins.iter().any(|p| p == plugin_id)
    }

    /// Disable all plugins (--no-plugins).
    pub const fn disable_all(&mut self) {
        self.plugins_enabled = false;
    }
}

/// Per-plugin configuration from composer.json "extra".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginConfig {
    /// Plugin-specific options.
    #[serde(default)]
    pub options: HashMap<String, serde_json::Value>,

    /// Whether the plugin is enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// Custom timeout for this plugin.
    #[serde(default, with = "option_duration_serde")]
    pub timeout: Option<Duration>,

    /// Custom sandbox settings.
    #[serde(default)]
    pub sandbox: Option<PluginSandboxConfig>,
}

const fn default_enabled() -> bool {
    true
}

impl PluginConfig {
    /// Create a new plugin config.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Get an option value.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&serde_json::Value> {
        self.options.get(key)
    }

    /// Get an option as a string.
    #[must_use]
    pub fn get_str(&self, key: &str) -> Option<&str> {
        self.options.get(key).and_then(|v| v.as_str())
    }

    /// Get an option as a bool.
    #[must_use]
    pub fn get_bool(&self, key: &str) -> Option<bool> {
        self.options.get(key).and_then(serde_json::Value::as_bool)
    }

    /// Get an option as an i64.
    #[must_use]
    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.options.get(key).and_then(serde_json::Value::as_i64)
    }

    /// Get an option as a string array.
    #[must_use]
    pub fn get_str_array(&self, key: &str) -> Option<Vec<&str>> {
        self.options.get(key).and_then(|v| {
            v.as_array()
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        })
    }

    /// Set an option.
    pub fn set(&mut self, key: impl Into<String>, value: serde_json::Value) {
        self.options.insert(key.into(), value);
    }

    /// Merge with another config (other takes precedence).
    pub fn merge(&mut self, other: &Self) {
        for (key, value) in &other.options {
            self.options.insert(key.clone(), value.clone());
        }

        if other.timeout.is_some() {
            self.timeout = other.timeout;
        }

        if other.sandbox.is_some() {
            self.sandbox = other.sandbox.clone();
        }
    }
}

/// Per-plugin sandbox configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginSandboxConfig {
    /// Disable sandbox for this plugin.
    #[serde(default)]
    pub disable: bool,

    /// Additional allowed read paths.
    #[serde(default)]
    pub allowed_read_paths: Vec<String>,

    /// Additional allowed write paths.
    #[serde(default)]
    pub allowed_write_paths: Vec<String>,

    /// Additional allowed network hosts.
    #[serde(default)]
    pub allowed_hosts: Vec<String>,

    /// Custom memory limit.
    #[serde(default)]
    pub memory_limit: Option<usize>,
}

/// Configuration loaded from composer.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[allow(dead_code)]
pub struct ComposerPluginConfig {
    /// Global plugin configuration.
    #[serde(default, rename = "plugin-config")]
    pub global: HashMap<String, serde_json::Value>,

    /// Per-plugin configuration.
    #[serde(default)]
    pub plugins: HashMap<String, PluginConfig>,

    /// Disabled plugins.
    #[serde(default, rename = "disable-plugins")]
    pub disabled: Vec<String>,

    /// Allowed plugins (if set, only these are allowed).
    #[serde(default, rename = "allow-plugins")]
    pub allowed: Option<HashMap<String, bool>>,
}

#[allow(dead_code)]
impl ComposerPluginConfig {
    /// Load from a composer.json "config" section.
    ///
    /// # Errors
    /// Returns error if parsing fails.
    pub fn from_json(json: &serde_json::Value) -> Result<Self, sonic_rs::Error> {
        // Handle composer.json format
        let config = json.get("config").unwrap_or(json);
        sonic_rs::from_str(&config.to_string())
    }

    /// Check if a plugin is allowed.
    #[must_use]
    pub fn is_plugin_allowed(&self, plugin_id: &str) -> bool {
        // If allowed list is set, check it
        if let Some(ref allowed) = self.allowed {
            return allowed.get(plugin_id).copied().unwrap_or(false);
        }

        // Otherwise check disabled list
        !self.disabled.contains(&plugin_id.to_string())
    }

    /// Get configuration for a specific plugin.
    #[must_use]
    pub fn get_plugin_config(&self, plugin_id: &str) -> Option<&PluginConfig> {
        self.plugins.get(plugin_id)
    }
}

/// Duration serialization for optional Duration fields.
mod option_duration_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(duration: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        duration.map(|d| d.as_secs()).serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let secs = Option::<u64>::deserialize(deserializer)?;
        Ok(secs.map(Duration::from_secs))
    }
}

/// Environment-based configuration.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct EnvironmentConfig {
    /// `LIBRETTO_PLUGINS_DISABLED`
    pub plugins_disabled: bool,
    /// `LIBRETTO_PLUGIN_TIMEOUT`
    pub timeout: Option<Duration>,
    /// `LIBRETTO_PLUGIN_MAX_CONCURRENT`
    pub max_concurrent: Option<usize>,
    /// `LIBRETTO_HOT_RELOAD`
    pub hot_reload: bool,
}

#[allow(dead_code)]
impl EnvironmentConfig {
    /// Load configuration from environment variables.
    #[must_use]
    pub fn from_env() -> Self {
        Self {
            plugins_disabled: std::env::var("LIBRETTO_PLUGINS_DISABLED")
                .is_ok_and(|v| v == "1" || v.to_lowercase() == "true"),
            timeout: std::env::var("LIBRETTO_PLUGIN_TIMEOUT")
                .ok()
                .and_then(|v| v.parse().ok())
                .map(Duration::from_secs),
            max_concurrent: std::env::var("LIBRETTO_PLUGIN_MAX_CONCURRENT")
                .ok()
                .and_then(|v| v.parse().ok()),
            hot_reload: std::env::var("LIBRETTO_HOT_RELOAD")
                .is_ok_and(|v| v == "1" || v.to_lowercase() == "true"),
        }
    }

    /// Apply to manager config.
    pub const fn apply_to(&self, config: &mut PluginManagerConfig) {
        if self.plugins_disabled {
            config.plugins_enabled = false;
        }

        if let Some(timeout) = self.timeout {
            config.default_timeout = timeout;
        }

        if let Some(max) = self.max_concurrent {
            config.max_concurrent = max;
        }

        if self.hot_reload {
            config.hot_reload = true;
        }
    }
}

impl Default for EnvironmentConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_manager_config_defaults() {
        let config = PluginManagerConfig::default();

        assert!(config.plugins_enabled);
        assert!(!config.hot_reload);
        assert_eq!(config.default_timeout, Duration::from_secs(30));
        assert_eq!(config.max_concurrent, 20);
    }

    #[test]
    fn plugin_manager_config_builder() {
        let config = PluginManagerConfig::new()
            .with_hot_reload(true)
            .with_timeout(Duration::from_secs(60))
            .with_max_concurrent(10);

        assert!(config.hot_reload);
        assert_eq!(config.default_timeout, Duration::from_secs(60));
        assert_eq!(config.max_concurrent, 10);
    }

    #[test]
    fn plugin_config_options() {
        let mut config = PluginConfig::new();

        config.set("string_opt", serde_json::json!("value"));
        config.set("bool_opt", serde_json::json!(true));
        config.set("int_opt", serde_json::json!(42));

        assert_eq!(config.get_str("string_opt"), Some("value"));
        assert_eq!(config.get_bool("bool_opt"), Some(true));
        assert_eq!(config.get_i64("int_opt"), Some(42));
    }

    #[test]
    fn plugin_config_merge() {
        let mut base = PluginConfig::new();
        base.set("opt1", serde_json::json!("base"));
        base.set("opt2", serde_json::json!("base"));

        let mut override_config = PluginConfig::new();
        override_config.set("opt2", serde_json::json!("override"));
        override_config.set("opt3", serde_json::json!("new"));

        base.merge(&override_config);

        assert_eq!(base.get_str("opt1"), Some("base"));
        assert_eq!(base.get_str("opt2"), Some("override"));
        assert_eq!(base.get_str("opt3"), Some("new"));
    }

    #[test]
    fn composer_plugin_config_allowed() {
        let config = ComposerPluginConfig {
            allowed: Some(HashMap::from([
                ("vendor/allowed".to_string(), true),
                ("vendor/denied".to_string(), false),
            ])),
            ..Default::default()
        };

        assert!(config.is_plugin_allowed("vendor/allowed"));
        assert!(!config.is_plugin_allowed("vendor/denied"));
        assert!(!config.is_plugin_allowed("vendor/unknown"));
    }

    #[test]
    fn composer_plugin_config_disabled() {
        let config = ComposerPluginConfig {
            disabled: vec!["vendor/disabled".to_string()],
            ..Default::default()
        };

        assert!(!config.is_plugin_allowed("vendor/disabled"));
        assert!(config.is_plugin_allowed("vendor/other"));
    }

    #[test]
    fn disabled_plugins_check() {
        let config = PluginManagerConfig::new()
            .with_disabled_plugins(vec!["plugin-a".to_string(), "plugin-b".to_string()]);

        assert!(config.is_plugin_disabled("plugin-a"));
        assert!(config.is_plugin_disabled("plugin-b"));
        assert!(!config.is_plugin_disabled("plugin-c"));
    }
}
