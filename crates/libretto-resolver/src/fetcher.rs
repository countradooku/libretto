//! Package fetching trait and types.
//!
//! This module defines the interface for fetching package metadata from
//! repositories like Packagist.

use std::pin::Pin;
use std::sync::Arc;

/// Trait for asynchronously fetching package metadata.
///
/// Implement this trait to provide package data to the resolver.
/// The default implementation fetches from Packagist.
pub trait PackageFetcher: Send + Sync + 'static {
    /// Fetch package metadata by name.
    ///
    /// Returns `None` if the package doesn't exist or couldn't be fetched.
    fn fetch(
        &self,
        name: String,
    ) -> Pin<Box<dyn std::future::Future<Output = Option<FetchedPackage>> + Send + '_>>;
}

// Implement for Arc<T> where T: PackageFetcher
impl<T: PackageFetcher> PackageFetcher for Arc<T> {
    fn fetch(
        &self,
        name: String,
    ) -> Pin<Box<dyn std::future::Future<Output = Option<FetchedPackage>> + Send + '_>> {
        (**self).fetch(name)
    }
}

/// Package data returned by the fetcher.
#[derive(Debug, Clone)]
pub struct FetchedPackage {
    /// Package name (vendor/name).
    pub name: String,
    /// Available versions.
    pub versions: Vec<FetchedVersion>,
}

/// Version data returned by the fetcher.
#[derive(Debug, Clone)]
pub struct FetchedVersion {
    /// Version string (e.g., "1.0.0", "v2.1.3").
    pub version: String,
    /// Required dependencies.
    pub require: Vec<(String, String)>,
    /// Dev dependencies.
    pub require_dev: Vec<(String, String)>,
    /// Packages this replaces.
    pub replace: Vec<(String, String)>,
    /// Virtual packages this provides.
    pub provide: Vec<(String, String)>,
    /// Suggested packages.
    pub suggest: Vec<(String, String)>,
    /// Distribution URL.
    pub dist_url: Option<String>,
    /// Distribution type (zip, tar).
    pub dist_type: Option<String>,
    /// Distribution checksum.
    pub dist_shasum: Option<String>,
    /// Source repository URL.
    pub source_url: Option<String>,
    /// Source type (git, hg).
    pub source_type: Option<String>,
    /// Source reference (commit, tag).
    pub source_reference: Option<String>,
    /// Package type (library, project, etc.).
    pub package_type: Option<String>,
    /// Package description.
    pub description: Option<String>,
    /// Homepage URL.
    pub homepage: Option<String>,
    /// License(s).
    pub license: Option<Vec<String>>,
    /// Authors.
    pub authors: Option<sonic_rs::Value>,
    /// Keywords.
    pub keywords: Option<Vec<String>>,
    /// Release time.
    pub time: Option<String>,
    /// Autoload configuration.
    pub autoload: Option<sonic_rs::Value>,
    /// Autoload-dev configuration.
    pub autoload_dev: Option<sonic_rs::Value>,
    /// Extra metadata.
    pub extra: Option<sonic_rs::Value>,
    /// Support links.
    pub support: Option<sonic_rs::Value>,
    /// Funding links.
    pub funding: Option<sonic_rs::Value>,
    /// Notification URL.
    pub notification_url: Option<String>,
    /// Binary files.
    pub bin: Option<Vec<String>>,
}

// ============================================================================
// Backward Compatibility Aliases
// ============================================================================

/// Backward-compatible alias for `PackageFetcher`.
pub type TurboFetcher = dyn PackageFetcher;
