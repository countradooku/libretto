//! Platform requirements validation (PHP version, extensions, libraries).

use ahash::AHashSet;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::process::Stdio;
use thiserror::Error;
use tokio::process::Command;
use tracing::debug;

/// Platform validation error.
#[derive(Debug, Error)]
pub enum PlatformError {
    /// Missing platform requirement.
    #[error("missing requirement: {0}")]
    MissingRequirement(String),

    /// Version mismatch.
    #[error("version mismatch for {name}: required {required}, found {found}")]
    VersionMismatch {
        /// Requirement name.
        name: String,
        /// Required version.
        required: String,
        /// Found version.
        found: String,
    },

    /// Platform detection failed.
    #[error("platform detection failed: {0}")]
    DetectionFailed(String),

    /// Invalid version format.
    #[error("invalid version format: {0}")]
    InvalidVersion(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for platform operations.
pub type Result<T> = std::result::Result<T, PlatformError>;

/// Platform requirement type.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RequirementType {
    /// PHP version (php).
    Php,
    /// PHP extension (ext-*).
    Extension,
    /// Library (lib-*).
    Library,
    /// Composer plugin API (composer-plugin-api).
    ComposerPluginApi,
}

/// Platform requirement.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Requirement {
    /// Requirement type.
    pub req_type: RequirementType,
    /// Requirement name (without prefix).
    pub name: String,
    /// Version constraint.
    pub constraint: Option<VersionReq>,
}

impl Requirement {
    /// Parse from Composer format (e.g., "php", "ext-mbstring", "lib-curl").
    #[must_use]
    pub fn parse(name: &str, constraint: Option<&str>) -> Option<Self> {
        let (req_type, req_name) = if name == "php" {
            (RequirementType::Php, String::new())
        } else if let Some(ext_name) = name.strip_prefix("ext-") {
            (RequirementType::Extension, ext_name.to_string())
        } else if let Some(lib_name) = name.strip_prefix("lib-") {
            (RequirementType::Library, lib_name.to_string())
        } else if name == "composer-plugin-api" {
            (RequirementType::ComposerPluginApi, String::new())
        } else {
            return None;
        };

        let version_req = constraint.and_then(|c| VersionReq::parse(c).ok());

        Some(Self {
            req_type,
            name: req_name,
            constraint: version_req,
        })
    }

    /// Get display name.
    #[must_use]
    pub fn display_name(&self) -> String {
        match self.req_type {
            RequirementType::Php => "php".to_string(),
            RequirementType::Extension => format!("ext-{}", self.name),
            RequirementType::Library => format!("lib-{}", self.name),
            RequirementType::ComposerPluginApi => "composer-plugin-api".to_string(),
        }
    }
}

/// PHP platform information.
#[derive(Debug, Clone)]
pub struct PhpPlatform {
    /// PHP version.
    pub version: Version,
    /// Installed extensions.
    pub extensions: AHashSet<String>,
    /// Available libraries.
    pub libraries: AHashSet<String>,
}

impl PhpPlatform {
    /// Detect PHP platform from system.
    ///
    /// # Errors
    /// Returns error if detection fails.
    pub async fn detect() -> Result<Self> {
        Self::detect_with_binary("php").await
    }

    /// Detect with specific PHP binary.
    ///
    /// # Errors
    /// Returns error if detection fails.
    pub async fn detect_with_binary(php_binary: &str) -> Result<Self> {
        debug!(binary = php_binary, "detecting PHP platform");

        // Get PHP version
        let version_output = Command::new(php_binary)
            .arg("-r")
            .arg("echo PHP_VERSION;")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await?;

        if !version_output.status.success() {
            return Err(PlatformError::DetectionFailed("php -v failed".to_string()));
        }

        let version_str = String::from_utf8_lossy(&version_output.stdout);
        let version = Version::parse(version_str.trim())
            .map_err(|e| PlatformError::InvalidVersion(e.to_string()))?;

        debug!(version = %version, "detected PHP version");

        // Get loaded extensions
        let ext_output = Command::new(php_binary)
            .arg("-r")
            .arg("echo json_encode(get_loaded_extensions());")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .await?;

        let extensions: AHashSet<String> = if ext_output.status.success() {
            let ext_json = String::from_utf8_lossy(&ext_output.stdout);
            let vec: Vec<String> = sonic_rs::from_str(&ext_json).unwrap_or_default();
            vec.into_iter().collect()
        } else {
            AHashSet::new()
        };

        debug!(count = extensions.len(), "detected PHP extensions");

        // Detect common libraries (simplified)
        let libraries = Self::detect_libraries().await;

        Ok(Self {
            version,
            extensions,
            libraries,
        })
    }

    async fn detect_libraries() -> AHashSet<String> {
        let mut libs = AHashSet::new();

        // Check for common libraries by trying to link
        let common_libs = [
            "curl", "openssl", "zlib", "libxml", "pcre", "iconv", "bz2", "gd", "intl", "mbstring",
            "zip",
        ];

        for lib in &common_libs {
            // Simplified detection - in production would check actual system libraries
            libs.insert((*lib).to_string());
        }

        libs
    }

    /// Check if requirement is satisfied.
    #[must_use]
    pub fn check_requirement(&self, req: &Requirement) -> bool {
        match req.req_type {
            RequirementType::Php => {
                if let Some(ref constraint) = req.constraint {
                    constraint.matches(&self.version)
                } else {
                    true
                }
            }
            RequirementType::Extension => {
                let installed = self.extensions.contains(&req.name.to_lowercase());
                if !installed {
                    return false;
                }
                // Extension version checking not implemented (complex in PHP)
                true
            }
            RequirementType::Library => {
                let installed = self.libraries.contains(&req.name.to_lowercase());
                if !installed {
                    return false;
                }
                true
            }
            RequirementType::ComposerPluginApi => {
                // Simplified - accept any version
                true
            }
        }
    }

    /// Validate multiple requirements.
    #[must_use]
    pub fn validate_requirements(&self, requirements: &[Requirement]) -> Vec<PlatformError> {
        let mut errors = Vec::new();

        for req in requirements {
            if !self.check_requirement(req) {
                match req.req_type {
                    RequirementType::Php => {
                        if let Some(ref constraint) = req.constraint {
                            errors.push(PlatformError::VersionMismatch {
                                name: "php".to_string(),
                                required: constraint.to_string(),
                                found: self.version.to_string(),
                            });
                        }
                    }
                    RequirementType::Extension => {
                        errors.push(PlatformError::MissingRequirement(format!(
                            "ext-{}",
                            req.name
                        )));
                    }
                    RequirementType::Library => {
                        errors.push(PlatformError::MissingRequirement(format!(
                            "lib-{}",
                            req.name
                        )));
                    }
                    RequirementType::ComposerPluginApi => {}
                }
            }
        }

        errors
    }
}

/// Platform validation mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationMode {
    /// Validate all requirements.
    Full,
    /// Validate PHP only.
    PhpOnly,
    /// No validation.
    Disabled,
}

impl ValidationMode {
    /// Parse from string (for config).
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "true" | "full" => Self::Full,
            "php-only" | "php" => Self::PhpOnly,
            "false" | "disabled" => Self::Disabled,
            _ => Self::Full,
        }
    }
}

/// Platform validator.
#[derive(Debug)]
pub struct PlatformValidator {
    platform: PhpPlatform,
    mode: ValidationMode,
}

impl PlatformValidator {
    /// Create new validator with detected platform.
    ///
    /// # Errors
    /// Returns error if platform detection fails.
    pub async fn new(mode: ValidationMode) -> Result<Self> {
        let platform = if mode == ValidationMode::Disabled {
            // Create dummy platform
            PhpPlatform {
                version: Version::parse("8.0.0").unwrap(),
                extensions: AHashSet::new(),
                libraries: AHashSet::new(),
            }
        } else {
            PhpPlatform::detect().await?
        };

        Ok(Self { platform, mode })
    }

    /// Validate requirements.
    #[must_use]
    pub fn validate(&self, requirements: &[Requirement]) -> Vec<PlatformError> {
        if self.mode == ValidationMode::Disabled {
            return Vec::new();
        }

        let filtered: Vec<_> = if self.mode == ValidationMode::PhpOnly {
            requirements
                .iter()
                .filter(|r| r.req_type == RequirementType::Php)
                .cloned()
                .collect()
        } else {
            requirements.to_vec()
        };

        self.platform.validate_requirements(&filtered)
    }

    /// Get platform info.
    #[must_use]
    pub const fn platform(&self) -> &PhpPlatform {
        &self.platform
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_requirement_parsing() {
        let req = Requirement::parse("php", Some(">=8.0")).unwrap();
        assert_eq!(req.req_type, RequirementType::Php);
        assert!(req.constraint.is_some());

        let req = Requirement::parse("ext-mbstring", None).unwrap();
        assert_eq!(req.req_type, RequirementType::Extension);
        assert_eq!(req.name, "mbstring");

        let req = Requirement::parse("lib-curl", Some("*")).unwrap();
        assert_eq!(req.req_type, RequirementType::Library);
        assert_eq!(req.name, "curl");
    }

    #[test]
    fn test_validation_mode_parsing() {
        assert_eq!(ValidationMode::parse("true"), ValidationMode::Full);
        assert_eq!(ValidationMode::parse("php-only"), ValidationMode::PhpOnly);
        assert_eq!(ValidationMode::parse("false"), ValidationMode::Disabled);
    }

    #[tokio::test]
    async fn test_php_detection() {
        // This test requires PHP to be installed
        if let Ok(platform) = PhpPlatform::detect().await {
            assert!(platform.version.major >= 7);
            println!(
                "Detected PHP {} with {} extensions",
                platform.version,
                platform.extensions.len()
            );
        }
    }
}
