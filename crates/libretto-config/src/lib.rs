//! Hierarchical configuration management for Libretto package manager.
//!
//! This crate provides comprehensive configuration management with:
//!
//! - **Hierarchical Configuration**: Sources merged in priority order:
//!   1. Built-in defaults
//!   2. System config (`/etc/libretto/config.json`)
//!   3. Global config (`~/.config/libretto/config.json`)
//!   4. Project config (`composer.json` config section)
//!   5. Environment variables (`COMPOSER_*`)
//!   6. CLI arguments
//!
//! - **Full Composer Compatibility**: All Composer configuration options supported
//! - **Authentication**: Secure credential management with optional keyring integration
//! - **Validation**: JSON schema validation with descriptive error messages
//! - **Caching**: Lazy loading with file watching for hot reloading
//! - **Performance**: <5ms parsing, zero-overhead cached access
//!
//! # Quick Start
//!
//! ```no_run
//! use libretto_config::{ConfigLoader, CachedConfigManager};
//!
//! // Simple usage with caching
//! let manager = CachedConfigManager::new(".");
//! let config = manager.get_config().expect("failed to load config");
//!
//! println!("Vendor dir: {:?}", config.vendor_dir);
//! println!("Cache dir: {:?}", config.cache_dir);
//!
//! // Direct loader usage
//! let loader = ConfigLoader::new(".");
//! let config = loader.resolve().expect("failed to resolve config");
//! ```
//!
//! # Configuration Sources
//!
//! ## System Configuration
//!
//! - Linux: `/etc/libretto/config.json`
//! - macOS: `/Library/Application Support/libretto/config.json`
//! - Windows: `C:\ProgramData\libretto\config.json`
//!
//! ## Global Configuration
//!
//! - Uses `COMPOSER_HOME` if set
//! - Otherwise uses XDG config directory (`~/.config/libretto/`)
//!
//! ## Environment Variables
//!
//! All standard Composer environment variables are supported:
//!
//! - `COMPOSER_HOME` - Global configuration directory
//! - `COMPOSER_CACHE_DIR` - Cache directory
//! - `COMPOSER_PROCESS_TIMEOUT` - Process timeout in seconds
//! - `COMPOSER_ALLOW_SUPERUSER` - Allow running as root
//! - `COMPOSER_AUTH` - Inline auth.json content
//! - `COMPOSER_DISABLE_NETWORK` - Offline mode
//! - `HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY` - Proxy configuration
//!
//! # Authentication
//!
//! Authentication credentials are loaded from `auth.json` files:
//!
//! ```json
//! {
//!     "github-oauth": {
//!         "github.com": "your-token"
//!     },
//!     "http-basic": {
//!         "repo.example.com": {
//!             "username": "user",
//!             "password": "pass"
//!         }
//!     }
//! }
//! ```
//!
//! With the `keyring` feature, credentials can be stored securely in the system keyring.

#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

pub mod auth;
pub mod cache;
pub mod env;
pub mod error;
pub mod loader;
pub mod types;
pub mod validate;

// Re-exports for convenience
pub use auth::{AuthConfig, Credential, CredentialStore};
pub use cache::{CacheStats, CachedConfigManager, ConfigCache, ConfigWatcher, global_cache};
pub use env::{ComposerEnvVar, EnvConfig, parse_byte_size, parse_duration_secs};
pub use error::{ConfigError, Result};
pub use loader::{CliOverrides, ConfigLoader, ConfigSource};
pub use types::{
    AllowPlugins, ArchiveFormat, AutoloadConfig, AutoloadPath, BinCompat, ComposerConfig,
    ComposerManifest, DiscardChanges, GitHubProtocol, PlatformCheck, PreferredInstall,
    PreferredInstallConfig, Repositories, RepositoryConfig, RepositoryDefinition, RepositoryType,
    ResolvedConfig, Scripts, ScriptsConfig, Stability, StoreAuths,
};
pub use validate::{Severity, ValidationIssue, ValidationResult, Validator};

/// Prelude module for common imports.
pub mod prelude {
    pub use crate::auth::{AuthConfig, Credential, CredentialStore};
    pub use crate::cache::{CachedConfigManager, global_cache};
    pub use crate::error::{ConfigError, Result};
    pub use crate::loader::{CliOverrides, ConfigLoader};
    pub use crate::types::{ComposerConfig, ComposerManifest, ResolvedConfig};
    pub use crate::validate::Validator;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resolved_config() {
        let config = ResolvedConfig::default();
        assert_eq!(config.process_timeout, 300);
        assert!(config.secure_http);
        assert!(!config.disable_tls);
        assert!(config.lock);
    }

    #[test]
    fn config_loader_creation() {
        let loader = ConfigLoader::new("/tmp/test");
        assert!(loader.project_manifest_path().ends_with("composer.json"));
    }

    #[test]
    fn env_var_names() {
        assert_eq!(ComposerEnvVar::Home.as_str(), "COMPOSER_HOME");
        assert_eq!(ComposerEnvVar::CacheDir.as_str(), "COMPOSER_CACHE_DIR");
    }

    #[test]
    fn parse_byte_size_test() {
        assert_eq!(parse_byte_size("1024").unwrap(), 1024);
        assert_eq!(parse_byte_size("1K").unwrap(), 1024);
        assert_eq!(parse_byte_size("1M").unwrap(), 1024 * 1024);
    }

    #[test]
    fn auth_config_default() {
        let auth = AuthConfig::default();
        assert!(auth.is_empty());
    }

    #[test]
    fn validator_creation() {
        let validator = Validator::new().strict(true);
        let manifest = ComposerManifest::default();
        let result = validator.validate_manifest(&manifest);
        // Empty manifest should not have errors
        assert!(!result.has_errors());
    }
}
