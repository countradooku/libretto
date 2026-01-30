//! Plugin discovery from vendor directory.
//!
//! This module handles scanning the vendor directory for Composer plugins,
//! parsing their metadata from composer.json files.

use crate::api::PluginMetadata;
use crate::config::PluginConfig;
use crate::error::Result;
use dashmap::DashMap;
use moka::sync::Cache;
use rayon::prelude::*;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

/// Composer package types that indicate a plugin.
const PLUGIN_PACKAGE_TYPES: &[&str] = &["composer-plugin", "libretto-plugin"];

/// Metadata cache TTL.
const CACHE_TTL: Duration = Duration::from_secs(3600);

/// Plugin discovery service.
#[derive(Debug)]
pub struct PluginDiscovery {
    /// Metadata cache.
    cache: Cache<PathBuf, Arc<PluginMetadata>>,
    /// Discovered plugins by path.
    discovered: DashMap<PathBuf, PluginMetadata>,
}

impl PluginDiscovery {
    /// Create a new plugin discovery service.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(1000)
                .time_to_live(CACHE_TTL)
                .build(),
            discovered: DashMap::new(),
        }
    }

    /// Scan the vendor directory for plugins.
    ///
    /// # Errors
    /// Returns error if scanning fails.
    pub async fn scan_vendor(&self, vendor_path: &Path) -> Result<Vec<PluginMetadata>> {
        if !vendor_path.exists() {
            return Ok(Vec::new());
        }

        info!(path = %vendor_path.display(), "scanning vendor directory for plugins");

        // Find all composer.json files in vendor
        let composer_files: Vec<PathBuf> = WalkDir::new(vendor_path)
            .max_depth(3) // vendor/vendor-name/package-name/composer.json
            .into_iter()
            .filter_map(std::result::Result::ok)
            .filter(|e| e.file_name() == "composer.json")
            .map(|e| e.path().to_path_buf())
            .collect();

        debug!(count = composer_files.len(), "found composer.json files");

        // Parse in parallel
        let plugins: Vec<PluginMetadata> = composer_files
            .par_iter()
            .filter_map(|path| {
                match self.parse_composer_json(path) {
                    Ok(Some(metadata)) => {
                        debug!(plugin = %metadata.name, "discovered plugin");
                        self.discovered.insert(path.clone(), metadata.clone());
                        Some(metadata)
                    }
                    Ok(None) => None, // Not a plugin
                    Err(e) => {
                        warn!(path = %path.display(), error = %e, "failed to parse composer.json");
                        None
                    }
                }
            })
            .collect();

        info!(count = plugins.len(), "discovered plugins");
        Ok(plugins)
    }

    /// Parse a composer.json file and extract plugin metadata.
    fn parse_composer_json(&self, path: &Path) -> Result<Option<PluginMetadata>> {
        // Check cache first
        if let Some(cached) = self.cache.get(path) {
            return Ok(Some((*cached).clone()));
        }

        let content = std::fs::read_to_string(path)?;
        let composer: ComposerJson = sonic_rs::from_str(&content)?;

        // Check if this is a plugin package
        let package_type = composer.package_type.as_deref().unwrap_or("library");
        if !PLUGIN_PACKAGE_TYPES.contains(&package_type) {
            return Ok(None);
        }

        // Extract plugin metadata
        let metadata = self.extract_metadata(&composer, path)?;

        // Cache the result
        self.cache
            .insert(path.to_path_buf(), Arc::new(metadata.clone()));

        Ok(Some(metadata))
    }

    /// Extract plugin metadata from composer.json.
    fn extract_metadata(&self, composer: &ComposerJson, path: &Path) -> Result<PluginMetadata> {
        let extra = composer.extra.as_ref();

        // Get plugin class from extra.class
        let class = extra.and_then(|e| e.class.clone());

        // Get plugin API version
        let plugin_api_version = extra.and_then(|e| e.plugin_api_version.clone());

        // Get plugin configuration
        let config = extra.and_then(|e| {
            e.config.as_ref().map(|c| PluginConfig {
                options: c.clone(),
                enabled: true,
                timeout: None,
                sandbox: None,
            })
        });

        // Parse capabilities from extra
        let capabilities = extra
            .and_then(|e| e.capabilities.as_ref())
            .map(|caps| {
                caps.iter()
                    .filter_map(|c| match c.as_str() {
                        "install" => Some(crate::api::PluginCapability::Install),
                        "resolve" => Some(crate::api::PluginCapability::Resolve),
                        "command" => Some(crate::api::PluginCapability::Command),
                        "event" => Some(crate::api::PluginCapability::Event),
                        "repository" => Some(crate::api::PluginCapability::Repository),
                        "autoload" => Some(crate::api::PluginCapability::Autoload),
                        "download" => Some(crate::api::PluginCapability::Download),
                        "source" => Some(crate::api::PluginCapability::Source),
                        "script" => Some(crate::api::PluginCapability::Script),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Parse authors
        let authors = composer
            .authors
            .as_ref()
            .map(|authors| {
                authors
                    .iter()
                    .map(|a| crate::api::Author {
                        name: a.name.clone().unwrap_or_default(),
                        email: a.email.clone(),
                        homepage: a.homepage.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        // Get plugin directory (parent of composer.json)
        let plugin_path = path.parent().map(Path::to_path_buf);

        Ok(PluginMetadata {
            name: composer.name.clone().unwrap_or_else(|| "unknown".into()),
            version: composer.version.clone().unwrap_or_else(|| "0.0.0".into()),
            description: composer.description.clone().unwrap_or_default(),
            plugin_api_version,
            class,
            capabilities,
            path: plugin_path,
            config,
            authors,
            license: composer.license.clone(),
            homepage: composer.homepage.clone(),
            require_libretto: extra.and_then(|e| e.require_libretto.clone()),
        })
    }

    /// Get a discovered plugin by path.
    #[must_use]
    pub fn get(&self, path: &Path) -> Option<PluginMetadata> {
        self.discovered.get(path).map(|p| p.clone())
    }

    /// Get all discovered plugins.
    #[must_use]
    pub fn all(&self) -> Vec<PluginMetadata> {
        self.discovered.iter().map(|p| p.value().clone()).collect()
    }

    /// Clear the discovery cache.
    pub fn clear_cache(&self) {
        self.cache.invalidate_all();
        self.discovered.clear();
    }

    /// Check if a plugin is discovered.
    #[must_use]
    pub fn is_discovered(&self, name: &str) -> bool {
        self.discovered.iter().any(|p| p.value().name == name)
    }

    /// Find a plugin by name.
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<PluginMetadata> {
        self.discovered
            .iter()
            .find(|p| p.value().name == name)
            .map(|p| p.value().clone())
    }

    /// Scan a single package directory.
    ///
    /// # Errors
    /// Returns error if scanning fails.
    pub fn scan_package(&self, package_path: &Path) -> Result<Option<PluginMetadata>> {
        let composer_path = package_path.join("composer.json");
        if !composer_path.exists() {
            return Ok(None);
        }

        self.parse_composer_json(&composer_path)
    }
}

impl Default for PluginDiscovery {
    fn default() -> Self {
        Self::new()
    }
}

/// Composer.json structure (subset).
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ComposerJson {
    /// Package name.
    name: Option<String>,
    /// Package version.
    version: Option<String>,
    /// Description.
    description: Option<String>,
    /// Package type.
    #[serde(rename = "type")]
    package_type: Option<String>,
    /// License.
    license: Option<String>,
    /// Homepage.
    homepage: Option<String>,
    /// Authors.
    authors: Option<Vec<ComposerAuthor>>,
    /// Extra configuration.
    extra: Option<ComposerExtra>,
    /// Require (dependencies).
    #[serde(default)]
    require: std::collections::HashMap<String, String>,
}

/// Composer.json author structure.
#[derive(Debug, Deserialize)]
struct ComposerAuthor {
    name: Option<String>,
    email: Option<String>,
    homepage: Option<String>,
}

/// Composer.json extra section.
#[derive(Debug, Deserialize)]
struct ComposerExtra {
    /// Plugin class name.
    class: Option<String>,
    /// Plugin API version requirement.
    #[serde(rename = "plugin-api-version")]
    plugin_api_version: Option<String>,
    /// Plugin capabilities.
    capabilities: Option<Vec<String>>,
    /// Plugin configuration.
    #[serde(default)]
    config: Option<std::collections::HashMap<String, serde_json::Value>>,
    /// Required Libretto version.
    #[serde(rename = "require-libretto")]
    require_libretto: Option<String>,
}

/// Installed packages from composer.lock or installed.json.
#[derive(Debug, Deserialize)]
struct InstalledPackages {
    packages: Vec<ComposerJson>,
    #[serde(rename = "packages-dev")]
    packages_dev: Option<Vec<ComposerJson>>,
}

impl PluginDiscovery {
    /// Scan installed.json for faster discovery.
    ///
    /// # Errors
    /// Returns error if scanning fails.
    pub async fn scan_installed_json(&self, vendor_path: &Path) -> Result<Vec<PluginMetadata>> {
        let installed_path = vendor_path.join("composer").join("installed.json");

        if !installed_path.exists() {
            // Fall back to scanning vendor directory
            return self.scan_vendor(vendor_path).await;
        }

        info!(path = %installed_path.display(), "scanning installed.json for plugins");

        let content = std::fs::read_to_string(&installed_path)?;
        let installed: InstalledPackages = sonic_rs::from_str(&content)?;

        let mut plugins = Vec::new();

        // Process all packages
        let all_packages: Vec<_> = installed
            .packages
            .into_iter()
            .chain(installed.packages_dev.unwrap_or_default())
            .collect();

        for composer in all_packages {
            let package_type = composer.package_type.as_deref().unwrap_or("library");
            if !PLUGIN_PACKAGE_TYPES.contains(&package_type) {
                continue;
            }

            // Construct path to the package
            let name = composer.name.as_deref().unwrap_or("unknown");
            let package_path = vendor_path.join(name).join("composer.json");

            match self.extract_metadata(&composer, &package_path) {
                Ok(metadata) => {
                    debug!(plugin = %metadata.name, "discovered plugin from installed.json");
                    self.discovered.insert(package_path, metadata.clone());
                    plugins.push(metadata);
                }
                Err(e) => {
                    warn!(package = %name, error = %e, "failed to extract plugin metadata");
                }
            }
        }

        info!(
            count = plugins.len(),
            "discovered plugins from installed.json"
        );
        Ok(plugins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn discovery_creation() {
        let discovery = PluginDiscovery::new();
        assert!(discovery.all().is_empty());
    }

    #[tokio::test]
    async fn scan_empty_directory() {
        let temp = TempDir::new().unwrap();
        let discovery = PluginDiscovery::new();

        let plugins = discovery.scan_vendor(temp.path()).await.unwrap();
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn scan_nonexistent_directory() {
        let discovery = PluginDiscovery::new();

        let plugins = discovery
            .scan_vendor(Path::new("/nonexistent/path"))
            .await
            .unwrap();
        assert!(plugins.is_empty());
    }

    #[tokio::test]
    async fn scan_with_plugin() {
        let temp = TempDir::new().unwrap();

        // Create a plugin structure
        let plugin_dir = temp.path().join("vendor-name").join("plugin-name");
        std::fs::create_dir_all(&plugin_dir).unwrap();

        let composer_json = r#"{
            "name": "vendor-name/plugin-name",
            "version": "1.0.0",
            "type": "composer-plugin",
            "description": "A test plugin",
            "extra": {
                "class": "VendorName\\PluginName\\Plugin",
                "plugin-api-version": "^2.0"
            }
        }"#;

        std::fs::write(plugin_dir.join("composer.json"), composer_json).unwrap();

        let discovery = PluginDiscovery::new();
        let plugins = discovery.scan_vendor(temp.path()).await.unwrap();

        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "vendor-name/plugin-name");
        assert_eq!(plugins[0].version, "1.0.0");
        assert_eq!(
            plugins[0].class,
            Some("VendorName\\PluginName\\Plugin".into())
        );
    }

    #[test]
    fn parse_composer_json_non_plugin() {
        let temp = TempDir::new().unwrap();

        let composer_json = r#"{
            "name": "vendor/library",
            "version": "1.0.0",
            "type": "library"
        }"#;

        std::fs::write(temp.path().join("composer.json"), composer_json).unwrap();

        let discovery = PluginDiscovery::new();
        let result = discovery
            .parse_composer_json(&temp.path().join("composer.json"))
            .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn cache_functionality() {
        let temp = TempDir::new().unwrap();

        let composer_json = r#"{
            "name": "test/plugin",
            "type": "composer-plugin",
            "extra": {
                "class": "Test\\Plugin"
            }
        }"#;

        std::fs::write(temp.path().join("composer.json"), composer_json).unwrap();

        let discovery = PluginDiscovery::new();

        // First call should parse
        let result1 = discovery
            .parse_composer_json(&temp.path().join("composer.json"))
            .unwrap();
        assert!(result1.is_some());

        // Second call should use cache
        let result2 = discovery
            .parse_composer_json(&temp.path().join("composer.json"))
            .unwrap();
        assert!(result2.is_some());

        assert_eq!(result1.unwrap().name, result2.unwrap().name);
    }

    #[test]
    fn find_by_name() {
        let discovery = PluginDiscovery::new();

        let metadata = PluginMetadata {
            name: "test/plugin".into(),
            version: "1.0.0".into(),
            ..Default::default()
        };

        discovery
            .discovered
            .insert(PathBuf::from("/test"), metadata);

        let found = discovery.find_by_name("test/plugin");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "test/plugin");

        let not_found = discovery.find_by_name("other/plugin");
        assert!(not_found.is_none());
    }
}
