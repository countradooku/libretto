//! Package types and metadata.

use crate::version::VersionConstraint;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use url::Url;

/// Package identifier (vendor/name).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PackageId {
    vendor: String,
    name: String,
}

impl PackageId {
    /// Create new package ID.
    #[must_use]
    pub fn new(vendor: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            vendor: vendor.into(),
            name: name.into(),
        }
    }

    /// Parse from "vendor/name" string.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        let (vendor, name) = s.split_once('/')?;
        if vendor.is_empty() || name.is_empty() {
            return None;
        }
        Some(Self::new(vendor, name))
    }

    /// Get vendor.
    #[must_use]
    pub fn vendor(&self) -> &str {
        &self.vendor
    }

    /// Get name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get full name.
    #[must_use]
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.vendor, self.name)
    }
}

impl std::fmt::Display for PackageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.vendor, self.name)
    }
}

/// Package type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum PackageType {
    /// Library package.
    #[default]
    Library,
    /// Project package.
    Project,
    /// Metapackage.
    Metapackage,
    /// Composer plugin.
    ComposerPlugin,
}

/// Package source location.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PackageSource {
    /// Dist archive (zip/tar).
    Dist {
        /// Download URL.
        url: Url,
        /// Archive type.
        archive_type: String,
        /// Checksum.
        shasum: Option<String>,
    },
    /// Git source.
    Git {
        /// Repository URL.
        url: Url,
        /// Reference (branch/tag/commit).
        reference: String,
    },
}

/// Package dependency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dependency {
    /// Package ID.
    pub package: PackageId,
    /// Version constraint.
    pub constraint: VersionConstraint,
    /// Is dev dependency.
    #[serde(default)]
    pub dev: bool,
}

impl Dependency {
    /// Create new dependency.
    #[must_use]
    pub const fn new(package: PackageId, constraint: VersionConstraint) -> Self {
        Self {
            package,
            constraint,
            dev: false,
        }
    }

    /// Create dev dependency.
    #[must_use]
    pub const fn dev(package: PackageId, constraint: VersionConstraint) -> Self {
        Self {
            package,
            constraint,
            dev: true,
        }
    }
}

/// Complete package metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Package {
    /// Package identifier.
    pub id: PackageId,
    /// Version.
    pub version: Version,
    /// Package type.
    #[serde(default)]
    pub package_type: PackageType,
    /// Description.
    #[serde(default)]
    pub description: String,
    /// Dependencies.
    #[serde(default)]
    pub require: Vec<Dependency>,
    /// Dev dependencies.
    #[serde(default)]
    pub require_dev: Vec<Dependency>,
    /// Source location.
    pub source: Option<PackageSource>,
    /// Dist location.
    pub dist: Option<PackageSource>,
    /// Autoload configuration.
    #[serde(default)]
    pub autoload: HashMap<String, HashMap<String, String>>,
    /// Authors.
    #[serde(default)]
    pub authors: Vec<Author>,
    /// License.
    #[serde(default)]
    pub license: Vec<String>,
}

impl Package {
    /// Create minimal package.
    #[must_use]
    pub fn new(id: PackageId, version: Version) -> Self {
        Self {
            id,
            version,
            package_type: PackageType::default(),
            description: String::new(),
            require: Vec::new(),
            require_dev: Vec::new(),
            source: None,
            dist: None,
            autoload: HashMap::new(),
            authors: Vec::new(),
            license: Vec::new(),
        }
    }

    /// Get full name with version.
    #[must_use]
    pub fn name_version(&self) -> String {
        format!("{}@{}", self.id, self.version)
    }
}

/// Package author.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Author {
    /// Name.
    pub name: String,
    /// Email.
    #[serde(default)]
    pub email: Option<String>,
    /// Homepage.
    #[serde(default)]
    pub homepage: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_package_id() {
        let id = PackageId::parse("symfony/console").expect("valid package id should parse");
        assert_eq!(id.vendor(), "symfony");
        assert_eq!(id.name(), "console");
        assert_eq!(id.full_name(), "symfony/console");
    }

    #[test]
    fn invalid_package_id() {
        assert!(PackageId::parse("invalid").is_none());
        assert!(PackageId::parse("/name").is_none());
        assert!(PackageId::parse("vendor/").is_none());
    }
}
