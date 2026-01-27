//! High-performance package repository clients for Libretto.
//!
//! This crate provides comprehensive repository integration for the Libretto
//! package manager, supporting:
//!
//! - **Packagist API v2**: Full support for packagist.org and private Packagist
//!   instances, including lazy metadata loading, provider-includes, and ETags
//!   for efficient caching.
//!
//! - **VCS Providers**: GitHub, GitLab, and Bitbucket API integration for
//!   fetching composer.json from version control repositories.
//!
//! - **Multiple Repository Types**: Composer, VCS, Path, Package, and Artifact
//!   repositories as defined by the Composer specification.
//!
//! - **HTTP Client**: High-performance HTTP/2 client with connection pooling,
//!   per-host rate limiting, exponential backoff retry, and conditional
//!   request support (ETags, If-Modified-Since).
//!
//! - **Caching**: Multi-tier caching with in-memory LRU and optional persistent
//!   disk storage via libretto-cache.
//!
//! - **Security Advisories**: Integration with Packagist security advisories API
//!   for vulnerability checking.
//!
//! ## Performance Targets
//!
//! - Fetch metadata for 100 packages in <500ms (cold cache)
//! - Fetch metadata for 100 packages in <50ms (warm cache)
//! - Support 1000+ requests/second with proper caching
//!
//! ## Example
//!
//! ```no_run
//! use libretto_repository::{RepositoryManager, PackagistClient};
//! use libretto_core::PackageId;
//!
//! # async fn example() -> libretto_repository::Result<()> {
//! // Create a repository manager
//! let manager = RepositoryManager::new();
//! manager.add_packagist()?;
//!
//! // Search for packages
//! let results = manager.search("monolog").await?;
//! for result in results.iter().take(5) {
//!     println!("{}: {}", result.name, result.description);
//! }
//!
//! // Get package versions
//! let package_id = PackageId::parse("monolog/monolog").unwrap();
//! let versions = manager.get_package(&package_id).await?;
//! println!("Found {} versions of monolog/monolog", versions.len());
//!
//! // Check security advisories
//! let advisories = manager
//!     .get_security_advisories(&["monolog/monolog".to_string()])
//!     .await?;
//! if !advisories.is_empty() {
//!     println!("Warning: {} security advisories found!", advisories.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Direct Packagist Client
//!
//! For simpler use cases, you can use the Packagist client directly:
//!
//! ```no_run
//! use libretto_repository::packagist::{PackagistClient, PackagistConfig};
//! use libretto_core::PackageId;
//!
//! # async fn example() -> libretto_repository::Result<()> {
//! let client = PackagistClient::new()?;
//! client.init().await?;
//!
//! let package_id = PackageId::parse("symfony/console").unwrap();
//! let packages = client.get_package(&package_id).await?;
//!
//! for pkg in packages.iter().take(5) {
//!     println!("{} v{}", pkg.id, pkg.version);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Private Packagist
//!
//! ```no_run
//! use libretto_repository::packagist::{PackagistClient, PackagistConfig};
//! use url::Url;
//!
//! # fn example() -> libretto_repository::Result<()> {
//! let config = PackagistConfig::private(
//!     Url::parse("https://repo.private.packagist.com/acme/").unwrap(),
//!     Url::parse("https://private.packagist.com/acme/").unwrap(),
//!     "your-api-token".to_string(),
//! );
//!
//! let client = PackagistClient::with_config(config)?;
//! # Ok(())
//! # }
//! ```
//!
//! ## GitHub Integration
//!
//! ```no_run
//! use libretto_repository::providers::{GitHubClient, GitHubConfig, VcsProvider};
//! use url::Url;
//!
//! # async fn example() -> libretto_repository::Result<()> {
//! // With authentication for higher rate limits
//! let config = GitHubConfig::with_token("your-github-token".to_string());
//! let client = GitHubClient::with_config(config)?;
//!
//! // Check rate limit
//! let rate_limit = client.get_rate_limit().await?;
//! println!("Remaining: {}/{}", rate_limit.remaining, rate_limit.limit);
//!
//! // Fetch composer.json from a repository
//! let url = Url::parse("https://github.com/symfony/console").unwrap();
//! let composer_json = client.fetch_composer_json(&url, "main").await?;
//! # Ok(())
//! # }
//! ```

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

pub mod cache;
pub mod client;
pub mod error;
pub mod manager;
pub mod packagist;
pub mod providers;
pub mod types;

// Re-export main types
pub use cache::{RepositoryCache, RepositoryCacheStats};
pub use client::{AuthType, HttpClient, HttpClientConfig, HttpClientStats, HttpResponse};
pub use error::{RepositoryError, Result};
pub use manager::{ManagerStats, RepositoryManager};
pub use packagist::{PackagistClient, PackagistConfig, PackagistStats};
pub use types::{
    AuthConfig, InlinePackage, PackageSearchResult, PackageVersion, PrioritizedRepository,
    RepositoryConfig, RepositoryOptions, RepositoryPriority, RepositoryType, Stability,
};

// Re-export commonly used types from packagist
pub use packagist::{SearchResult, SecurityAdvisory};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Create a default repository manager with Packagist.
///
/// # Errors
/// Returns error if manager cannot be created.
pub fn default_manager() -> Result<RepositoryManager> {
    let manager = RepositoryManager::new();
    manager.add_packagist()?;
    Ok(manager)
}

/// Create a Packagist client for packagist.org.
///
/// # Errors
/// Returns error if client cannot be created.
pub fn packagist() -> Result<PackagistClient> {
    PackagistClient::new()
}

/// Type alias for backward compatibility with CLI.
pub type Repository = RepositoryManager;

impl Repository {
    /// Create a repository with Packagist configured and initialized.
    ///
    /// # Errors
    /// Returns error if Packagist cannot be configured.
    pub fn packagist() -> Result<Self> {
        default_manager()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use libretto_core::PackageId;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_default_manager() {
        let manager = default_manager().unwrap();
        assert_eq!(manager.repository_count(), 1);
    }

    #[test]
    fn test_packagist_client() {
        let client = packagist().unwrap();
        assert_eq!(client.repo_url().host_str(), Some("repo.packagist.org"));
    }

    #[tokio::test]
    async fn test_stability_filtering() {
        let manager = default_manager().unwrap();

        // Default should be stable
        assert_eq!(manager.minimum_stability(), Stability::Stable);

        // Can change stability
        manager.set_minimum_stability(Stability::Dev);
        assert_eq!(manager.minimum_stability(), Stability::Dev);
    }
}
