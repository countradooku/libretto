//! Packagist repository integration.
//!
//! This module provides a high-performance client for the Packagist API v2,
//! supporting:
//!
//! - Lazy package metadata loading via metadata-url pattern
//! - Provider-includes for incremental metadata updates
//! - `ETags` and If-Modified-Since for efficient caching
//! - Private Packagist instances
//! - Parallel package metadata fetching
//! - Security advisories checking
//! - Package search and discovery
//!
//! # Example
//!
//! ```no_run
//! use libretto_repository::packagist::{PackagistClient, PackagistConfig};
//! use libretto_core::PackageId;
//!
//! # async fn example() -> libretto_repository::error::Result<()> {
//! // Create client for packagist.org
//! let client = PackagistClient::new()?;
//!
//! // Initialize (fetches packages.json)
//! client.init().await?;
//!
//! // Search for packages
//! let results = client.search("monolog", None).await?;
//! for result in results.iter().take(5) {
//!     println!("{}: {}", result.name, result.description);
//! }
//!
//! // Get package metadata
//! let package_id = PackageId::parse("monolog/monolog").unwrap();
//! let versions = client.get_package(&package_id).await?;
//! println!("Found {} versions", versions.len());
//!
//! // Check security advisories
//! let advisories = client
//!     .get_security_advisories(&["monolog/monolog".to_string()])
//!     .await?;
//! if !advisories.is_empty() {
//!     println!("Found {} security advisories!", advisories.len());
//! }
//! # Ok(())
//! # }
//! ```
//!
//! # Private Packagist
//!
//! ```no_run
//! use libretto_repository::packagist::{PackagistClient, PackagistConfig};
//! use url::Url;
//!
//! # fn example() -> libretto_repository::error::Result<()> {
//! let config = PackagistConfig::private(
//!     Url::parse("https://repo.private.packagist.com/acme/").unwrap(),
//!     Url::parse("https://private.packagist.com/acme/").unwrap(),
//!     "your-token-here".to_string(),
//! );
//!
//! let client = PackagistClient::with_config(config)?;
//! # Ok(())
//! # }
//! ```

mod client;
mod types;

pub use client::{
    PACKAGIST_API_URL, PACKAGIST_URL, PackagistClient, PackagistConfig, PackagistStats,
};
pub use types::{
    AbandonedValue, AdvisorySource, AuthorJson, AutoloadJson, ChangeAction, ChangesResponse,
    DistJson, FundingJson, LicenseValue, PackageListEntry, PackageListResponse,
    PackageMetadataResponse, PackageVersionJson, PackagesJson, PopularPackage,
    PopularPackagesResponse, ProviderInclude, PsrValue, SearchResponse, SearchResult,
    SecurityAdvisoriesResponse, SecurityAdvisory, SourceJson, StatisticsResponse, TotalStats,
    expand_minified_versions,
};
