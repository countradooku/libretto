//! Package repository clients for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use libretto_core::{Dependency, Error, Package, PackageId, Result, Version, VersionConstraint};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, info};
use url::Url;

/// Repository configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryConfig {
    /// Repository URL.
    pub url: Url,
    /// Repository type.
    #[serde(default)]
    pub repo_type: RepositoryType,
    /// Optional authentication.
    #[serde(default)]
    pub auth: Option<AuthConfig>,
}

/// Repository type.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RepositoryType {
    /// Composer repository (packagist-compatible).
    #[default]
    Composer,
    /// VCS repository.
    Vcs,
    /// Path repository.
    Path,
}

/// Authentication configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum AuthConfig {
    /// HTTP Basic auth.
    Basic {
        /// Username.
        username: String,
        /// Password.
        password: String,
    },
    /// Bearer token.
    Bearer {
        /// Token.
        token: String,
    },
}

/// Cached package metadata.
#[derive(Debug, Clone)]
struct CachedPackages {
    packages: Vec<Package>,
    fetched_at: DateTime<Utc>,
}

/// Repository client for fetching package metadata.
#[derive(Debug)]
pub struct Repository {
    config: RepositoryConfig,
    client: Client,
    cache: DashMap<String, CachedPackages>,
    cache_ttl: Duration,
}

impl Repository {
    /// Create new repository client.
    ///
    /// # Errors
    /// Returns error if HTTP client cannot be created.
    pub fn new(config: RepositoryConfig) -> Result<Self> {
        let mut builder = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .gzip(true)
            .brotli(true);

        if let Some(AuthConfig::Bearer { ref token }) = config.auth {
            let mut headers = reqwest::header::HeaderMap::new();
            let value = format!("Bearer {token}");
            headers.insert(
                reqwest::header::AUTHORIZATION,
                value
                    .parse()
                    .map_err(|_| Error::Config("invalid token".into()))?,
            );
            builder = builder.default_headers(headers);
        }

        let client = builder.build().map_err(|e| Error::Network(e.to_string()))?;

        Ok(Self {
            config,
            client,
            cache: DashMap::new(),
            cache_ttl: Duration::from_secs(300),
        })
    }

    /// Create packagist.org repository.
    ///
    /// # Errors
    /// Returns error if URL is invalid.
    pub fn packagist() -> Result<Self> {
        Self::new(RepositoryConfig {
            url: Url::parse("https://repo.packagist.org/")
                .map_err(|e| Error::Config(e.to_string()))?,
            repo_type: RepositoryType::Composer,
            auth: None,
        })
    }

    /// Get repository URL.
    #[must_use]
    pub fn url(&self) -> &Url {
        &self.config.url
    }

    /// Search for packages.
    ///
    /// # Errors
    /// Returns error if search fails.
    pub async fn search(&self, query: &str) -> Result<Vec<PackageSearchResult>> {
        let url = format!("{}search.json?q={}", self.config.url, query);
        debug!(url = %url, "searching packages");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(Error::Network(format!("HTTP {}", response.status())));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let search: SearchResponse =
            sonic_rs::from_slice(&body).map_err(libretto_core::Error::from)?;

        Ok(search.results)
    }

    /// Get package versions.
    ///
    /// # Errors
    /// Returns error if package cannot be fetched.
    pub async fn get_package(&self, package_id: &PackageId) -> Result<Vec<Package>> {
        let key = package_id.full_name();

        if let Some(cached) = self.cache.get(&key) {
            let age = Utc::now() - cached.fetched_at;
            if age.to_std().unwrap_or(Duration::MAX) < self.cache_ttl {
                debug!(package = %package_id, "cache hit");
                return Ok(cached.packages.clone());
            }
        }

        let packages = self.fetch_package(package_id).await?;

        self.cache.insert(
            key,
            CachedPackages {
                packages: packages.clone(),
                fetched_at: Utc::now(),
            },
        );

        Ok(packages)
    }

    /// Find best matching version.
    ///
    /// # Errors
    /// Returns error if no matching version is found.
    pub async fn find_version(
        &self,
        package_id: &PackageId,
        constraint: &VersionConstraint,
    ) -> Result<Package> {
        let packages = self.get_package(package_id).await?;

        packages
            .into_iter()
            .filter(|p| constraint.matches(&p.version))
            .max_by(|a, b| a.version.cmp(&b.version))
            .ok_or_else(|| Error::VersionNotFound {
                name: package_id.full_name(),
                constraint: constraint.to_string(),
            })
    }

    async fn fetch_package(&self, package_id: &PackageId) -> Result<Vec<Package>> {
        let url = format!(
            "{}p2/{}/{}.json",
            self.config.url,
            package_id.vendor(),
            package_id.name()
        );
        debug!(url = %url, "fetching package");

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Err(Error::PackageNotFound {
                name: package_id.full_name(),
            });
        }

        if !response.status().is_success() {
            return Err(Error::Network(format!("HTTP {}", response.status())));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let pkg_response: PackageResponse =
            sonic_rs::from_slice(&body).map_err(libretto_core::Error::from)?;

        let key = package_id.full_name();
        let versions = pkg_response.packages.get(&key).cloned().unwrap_or_default();

        info!(
            package = %package_id,
            versions = versions.len(),
            "fetched package"
        );

        Ok(versions
            .into_iter()
            .filter_map(|v| convert_package_version(package_id, v))
            .collect())
    }
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
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<PackageSearchResult>,
}

#[derive(Debug, Deserialize)]
struct PackageResponse {
    packages: HashMap<String, Vec<PackageVersion>>,
}

#[derive(Debug, Clone, Deserialize)]
struct PackageVersion {
    version: String,
    #[serde(default)]
    description: String,
    #[serde(default)]
    require: HashMap<String, String>,
    #[serde(default, rename = "require-dev")]
    require_dev: HashMap<String, String>,
    dist: Option<DistInfo>,
    source: Option<SourceInfo>,
}

#[derive(Debug, Clone, Deserialize)]
struct DistInfo {
    url: String,
    #[serde(rename = "type")]
    archive_type: String,
    shasum: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceInfo {
    url: String,
    reference: String,
}

fn convert_package_version(package_id: &PackageId, v: PackageVersion) -> Option<Package> {
    let version_str = v.version.trim_start_matches('v');
    let version = Version::parse(version_str).ok()?;

    let mut pkg = Package::new(package_id.clone(), version);
    pkg.description = v.description;

    for (name, constraint) in v.require {
        if let Some(dep_id) = PackageId::parse(&name) {
            pkg.require
                .push(Dependency::new(dep_id, VersionConstraint::new(constraint)));
        }
    }

    for (name, constraint) in v.require_dev {
        if let Some(dep_id) = PackageId::parse(&name) {
            pkg.require_dev
                .push(Dependency::dev(dep_id, VersionConstraint::new(constraint)));
        }
    }

    if let Some(dist) = v.dist {
        if let Ok(url) = Url::parse(&dist.url) {
            pkg.dist = Some(libretto_core::PackageSource::Dist {
                url,
                archive_type: dist.archive_type,
                shasum: dist.shasum,
            });
        }
    }

    if let Some(source) = v.source {
        if let Ok(url) = Url::parse(&source.url) {
            pkg.source = Some(libretto_core::PackageSource::Git {
                url,
                reference: source.reference,
            });
        }
    }

    Some(pkg)
}

/// Repository manager for multiple repositories.
#[derive(Debug, Default)]
pub struct RepositoryManager {
    repositories: Vec<Arc<Repository>>,
}

impl RepositoryManager {
    /// Create new manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            repositories: Vec::new(),
        }
    }

    /// Add repository.
    pub fn add(&mut self, repo: Repository) {
        self.repositories.push(Arc::new(repo));
    }

    /// Get all repositories.
    #[must_use]
    pub fn repositories(&self) -> &[Arc<Repository>] {
        &self.repositories
    }

    /// Search across all repositories.
    ///
    /// # Errors
    /// Returns error if search fails.
    pub async fn search(&self, query: &str) -> Result<Vec<PackageSearchResult>> {
        let mut results = Vec::new();
        for repo in &self.repositories {
            match repo.search(query).await {
                Ok(r) => results.extend(r),
                Err(e) => debug!(error = %e, "search failed"),
            }
        }
        Ok(results)
    }

    /// Find package in repositories.
    ///
    /// # Errors
    /// Returns error if package not found in any repository.
    pub async fn find_package(
        &self,
        package_id: &PackageId,
        constraint: &VersionConstraint,
    ) -> Result<Package> {
        for repo in &self.repositories {
            match repo.find_version(package_id, constraint).await {
                Ok(pkg) => return Ok(pkg),
                Err(Error::PackageNotFound { .. } | Error::VersionNotFound { .. }) => continue,
                Err(e) => return Err(e),
            }
        }

        Err(Error::PackageNotFound {
            name: package_id.full_name(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repository_config() {
        let config = RepositoryConfig {
            url: Url::parse("https://example.com").unwrap(),
            repo_type: RepositoryType::Composer,
            auth: None,
        };
        assert_eq!(config.repo_type, RepositoryType::Composer);
    }

    #[test]
    fn manager_default() {
        let manager = RepositoryManager::new();
        assert!(manager.repositories().is_empty());
    }
}
