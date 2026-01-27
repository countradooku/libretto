//! Repository types and configurations.

use libretto_core::Package;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use url::Url;

/// Repository configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// Repository URL.
    #[serde(default)]
    pub url: Option<Url>,
    /// Repository type.
    #[serde(rename = "type")]
    pub repo_type: RepositoryType,
    /// Optional authentication.
    #[serde(default)]
    pub auth: Option<AuthConfig>,
    /// Repository options.
    #[serde(default, flatten)]
    pub options: RepositoryOptions,
}

/// Repository type.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryType {
    /// Composer repository (packagist-compatible).
    #[default]
    Composer,
    /// VCS repository (git, svn, hg).
    Vcs,
    /// Path repository (local directory).
    Path,
    /// Package repository (inline package definition).
    Package,
    /// Artifact repository (directory with ZIP/tar files).
    Artifact,
}

impl std::fmt::Display for RepositoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Composer => write!(f, "composer"),
            Self::Vcs => write!(f, "vcs"),
            Self::Path => write!(f, "path"),
            Self::Package => write!(f, "package"),
            Self::Artifact => write!(f, "artifact"),
        }
    }
}

/// Repository-specific options.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepositoryOptions {
    /// For Composer: whether to allow insecure (non-HTTPS) connections.
    #[serde(default, rename = "allow_ssl_downgrade")]
    pub allow_ssl_downgrade: bool,

    /// For Composer: canonical repository (packages from here take precedence).
    #[serde(default)]
    pub canonical: Option<bool>,

    /// For Composer: exclude packages matching these patterns.
    #[serde(default)]
    pub exclude: Vec<String>,

    /// For Composer: only include packages matching these patterns.
    #[serde(default)]
    pub only: Vec<String>,

    /// For VCS: specify branch to use.
    #[serde(default)]
    pub branch: Option<String>,

    /// For VCS: specify tag to use.
    #[serde(default)]
    pub tag: Option<String>,

    /// For VCS: disable SSH for Git.
    #[serde(default, rename = "no-api")]
    pub no_api: bool,

    /// For VCS: use shallow clone.
    #[serde(default)]
    pub shallow: bool,

    /// For Path: relative or absolute path.
    #[serde(default)]
    pub path: Option<PathBuf>,

    /// For Path: symlink mode (true = symlink, false = copy).
    #[serde(default)]
    pub symlink: Option<bool>,

    /// For Package: inline package definition.
    #[serde(default)]
    pub package: Option<InlinePackage>,

    /// For Artifact: directory containing archives.
    #[serde(default)]
    pub artifact_dir: Option<PathBuf>,
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthConfig {
    /// HTTP Basic authentication.
    #[serde(rename = "http-basic")]
    HttpBasic {
        /// Username.
        username: String,
        /// Password.
        password: String,
    },
    /// Bearer token authentication.
    Bearer {
        /// Token.
        token: String,
    },
    /// SSH key authentication.
    #[serde(rename = "ssh")]
    Ssh {
        /// Path to private key.
        #[serde(default)]
        key_path: Option<PathBuf>,
        /// Key passphrase.
        #[serde(default)]
        passphrase: Option<String>,
    },
    /// GitHub OAuth token.
    #[serde(rename = "github-oauth")]
    GitHubOAuth {
        /// OAuth token.
        token: String,
    },
    /// GitLab token.
    #[serde(rename = "gitlab-token")]
    GitLabToken {
        /// Personal access token.
        token: String,
    },
    /// Bitbucket app password.
    #[serde(rename = "bitbucket-oauth")]
    BitbucketOAuth {
        /// Consumer key.
        consumer_key: String,
        /// Consumer secret.
        consumer_secret: String,
    },
}

/// Inline package definition for "package" repository type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlinePackage {
    /// Package name.
    pub name: String,
    /// Package version.
    pub version: String,
    /// Distribution info.
    #[serde(default)]
    pub dist: Option<InlineDist>,
    /// Source info.
    #[serde(default)]
    pub source: Option<InlineSource>,
    /// Required dependencies.
    #[serde(default)]
    pub require: HashMap<String, String>,
    /// Package type.
    #[serde(default, rename = "type")]
    pub package_type: Option<String>,
    /// Autoload configuration.
    #[serde(default)]
    pub autoload: HashMap<String, sonic_rs::Value>,
    /// Other metadata.
    #[serde(default, flatten)]
    pub extra: HashMap<String, sonic_rs::Value>,
}

/// Inline distribution info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineDist {
    /// Download URL.
    pub url: String,
    /// Archive type (zip, tar).
    #[serde(rename = "type")]
    pub archive_type: String,
    /// SHA checksum.
    #[serde(default)]
    pub shasum: Option<String>,
    /// Reference.
    #[serde(default)]
    pub reference: Option<String>,
}

/// Inline source info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineSource {
    /// Repository URL.
    pub url: String,
    /// Source type (git, svn, hg).
    #[serde(rename = "type")]
    pub source_type: String,
    /// Reference (branch, tag, commit).
    pub reference: String,
}

/// Package search result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageSearchResult {
    /// Package name.
    pub name: String,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Download count.
    #[serde(default)]
    pub downloads: u64,
    /// Favorites/stars count.
    #[serde(default)]
    pub favers: u64,
    /// Repository URL.
    #[serde(default)]
    pub repository: Option<String>,
    /// Whether abandoned.
    #[serde(default)]
    pub abandoned: bool,
    /// Replacement package if abandoned.
    #[serde(default)]
    pub replacement: Option<String>,
}

/// Package version information for resolution.
#[derive(Debug, Clone)]
pub struct PackageVersion {
    /// Package.
    pub package: Package,
    /// Whether this is a development version.
    pub is_dev: bool,
    /// Stability (stable, RC, beta, alpha, dev).
    pub stability: Stability,
    /// Original version string.
    pub version_string: String,
}

/// Package stability level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stability {
    /// Development version.
    Dev,
    /// Alpha version.
    Alpha,
    /// Beta version.
    Beta,
    /// Release candidate.
    Rc,
    /// Stable release.
    Stable,
}

impl Default for Stability {
    fn default() -> Self {
        Self::Stable
    }
}

impl Stability {
    /// Parse stability from version string.
    #[must_use]
    pub fn from_version(version: &str) -> Self {
        let lower = version.to_lowercase();
        if lower.contains("-dev") || lower.starts_with("dev-") {
            Self::Dev
        } else if lower.contains("-alpha") || lower.contains("alpha") {
            Self::Alpha
        } else if lower.contains("-beta") || lower.contains("beta") {
            Self::Beta
        } else if lower.contains("-rc") || lower.contains("rc") {
            Self::Rc
        } else if lower.matches('a').count() == 1 && lower.chars().any(|c| c.is_ascii_digit()) {
            // Compact notation like "1.0.0a1"
            Self::Alpha
        } else if lower.matches('b').count() == 1 && lower.chars().any(|c| c.is_ascii_digit()) {
            // Compact notation like "1.0.0b2"
            Self::Beta
        } else {
            Self::Stable
        }
    }

    /// Parse stability from constraint suffix.
    #[must_use]
    pub fn from_constraint_suffix(suffix: &str) -> Option<Self> {
        match suffix.to_lowercase().as_str() {
            "@dev" => Some(Self::Dev),
            "@alpha" => Some(Self::Alpha),
            "@beta" => Some(Self::Beta),
            "@rc" => Some(Self::Rc),
            "@stable" => Some(Self::Stable),
            _ => None,
        }
    }
}

impl std::fmt::Display for Stability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Dev => write!(f, "dev"),
            Self::Alpha => write!(f, "alpha"),
            Self::Beta => write!(f, "beta"),
            Self::Rc => write!(f, "RC"),
            Self::Stable => write!(f, "stable"),
        }
    }
}

/// Repository priority for conflict resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RepositoryPriority {
    /// Lowest priority (last in search order).
    Low = 0,
    /// Normal priority (default).
    Normal = 50,
    /// High priority (searched first).
    High = 100,
    /// Canonical priority (takes precedence over all).
    Canonical = 200,
}

impl Default for RepositoryPriority {
    fn default() -> Self {
        Self::Normal
    }
}

/// Repository with priority and metadata.
#[derive(Debug, Clone)]
pub struct PrioritizedRepository {
    /// Repository configuration.
    pub config: RepositoryConfig,
    /// Priority.
    pub priority: RepositoryPriority,
    /// Display name.
    pub name: String,
    /// Whether enabled.
    pub enabled: bool,
}

impl PrioritizedRepository {
    /// Create new prioritized repository.
    #[must_use]
    pub fn new(config: RepositoryConfig, name: impl Into<String>) -> Self {
        let priority = if config.options.canonical == Some(true) {
            RepositoryPriority::Canonical
        } else {
            RepositoryPriority::Normal
        };

        Self {
            config,
            priority,
            name: name.into(),
            enabled: true,
        }
    }

    /// Set priority.
    #[must_use]
    pub fn with_priority(mut self, priority: RepositoryPriority) -> Self {
        self.priority = priority;
        self
    }

    /// Set enabled state.
    #[must_use]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repository_type_display() {
        assert_eq!(RepositoryType::Composer.to_string(), "composer");
        assert_eq!(RepositoryType::Vcs.to_string(), "vcs");
        assert_eq!(RepositoryType::Path.to_string(), "path");
    }

    #[test]
    fn test_stability_from_version() {
        assert_eq!(Stability::from_version("1.0.0"), Stability::Stable);
        assert_eq!(Stability::from_version("1.0.0-dev"), Stability::Dev);
        assert_eq!(Stability::from_version("dev-master"), Stability::Dev);
        assert_eq!(Stability::from_version("1.0.0-alpha1"), Stability::Alpha);
        assert_eq!(Stability::from_version("1.0.0-beta2"), Stability::Beta);
        assert_eq!(Stability::from_version("1.0.0-RC1"), Stability::Rc);
        // 'b' alone is matched for beta - this is correct behavior
        assert_eq!(Stability::from_version("1.0.0b2"), Stability::Beta);
    }

    #[test]
    fn test_stability_from_constraint() {
        assert_eq!(
            Stability::from_constraint_suffix("@dev"),
            Some(Stability::Dev)
        );
        assert_eq!(
            Stability::from_constraint_suffix("@stable"),
            Some(Stability::Stable)
        );
        assert_eq!(Stability::from_constraint_suffix("invalid"), None);
    }

    #[test]
    fn test_stability_ordering() {
        assert!(Stability::Dev < Stability::Alpha);
        assert!(Stability::Alpha < Stability::Beta);
        assert!(Stability::Beta < Stability::Rc);
        assert!(Stability::Rc < Stability::Stable);
    }

    #[test]
    fn test_repository_config_deserialize() {
        let json = r#"{
            "type": "composer",
            "url": "https://repo.packagist.org"
        }"#;

        let config: RepositoryConfig = sonic_rs::from_str(json).unwrap();
        assert_eq!(config.repo_type, RepositoryType::Composer);
        assert!(config.url.is_some());
    }

    #[test]
    fn test_auth_config() {
        let json = r#"{
            "type": "http-basic",
            "username": "user",
            "password": "pass"
        }"#;

        let auth: AuthConfig = sonic_rs::from_str(json).unwrap();
        match auth {
            AuthConfig::HttpBasic { username, password } => {
                assert_eq!(username, "user");
                assert_eq!(password, "pass");
            }
            _ => panic!("wrong auth type"),
        }
    }
}
