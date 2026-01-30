//! Security advisory fetching and vulnerability matching.

use crate::{Severity, Vulnerability};
use ahash::AHashMap;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use libretto_core::{PackageId, Version};
use parking_lot::RwLock;
use reqwest::Client;
use semver::VersionReq;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tracing::{debug, info, warn};
use url::Url;

/// Advisory error.
#[derive(Debug, Error)]
pub enum AdvisoryError {
    /// Network error.
    #[error("network error: {0}")]
    Network(String),

    /// Parse error.
    #[error("parse error: {0}")]
    Parse(String),

    /// Database error.
    #[error("database error: {0}")]
    Database(String),
}

/// Result type for advisory operations.
pub type Result<T> = std::result::Result<T, AdvisoryError>;

/// Raw advisory from API.
#[derive(Debug, Clone, Deserialize)]
struct RawAdvisory {
    #[serde(rename = "advisoryId")]
    advisory_id: String,

    #[serde(rename = "packageName")]
    package_name: String,

    #[serde(rename = "affectedVersions")]
    affected_versions: String,

    #[serde(rename = "title")]
    title: String,

    #[serde(rename = "link", default)]
    link: Option<String>,

    #[serde(rename = "cve", default)]
    cve: Option<String>,

    #[serde(rename = "reportedAt", default)]
    reported_at: Option<String>,

    #[serde(rename = "sources", default)]
    sources: Vec<SourceInfo>,

    #[serde(rename = "severity", default)]
    severity: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct SourceInfo {
    #[serde(rename = "name")]
    name: String,

    #[serde(rename = "remoteId")]
    remote_id: String,
}

/// Parsed version constraint from advisory.
#[derive(Debug, Clone)]
struct VersionConstraint {
    req: VersionReq,
    original: String,
}

impl VersionConstraint {
    /// Parse Composer version constraint.
    fn parse(constraint: &str) -> Option<Self> {
        // Try to parse directly first (semver format)
        if let Ok(req) = VersionReq::parse(constraint.trim()) {
            return Some(Self {
                req,
                original: constraint.to_string(),
            });
        }

        // Handle Composer-specific formats
        let normalized = constraint.trim().replace("||", " || ");

        if let Ok(req) = VersionReq::parse(&normalized) {
            return Some(Self {
                req,
                original: constraint.to_string(),
            });
        }

        // Fallback: try exact match
        Version::parse(constraint.trim_start_matches('='))
            .ok()
            .and_then(|version| {
                VersionReq::parse(&format!("={version}"))
                    .ok()
                    .map(|req| Self {
                        req,
                        original: constraint.to_string(),
                    })
            })
    }

    /// Check if version matches constraint.
    fn matches(&self, version: &Version) -> bool {
        self.req.matches(version)
    }

    /// Get the original constraint string.
    fn original(&self) -> &str {
        &self.original
    }
}

/// Processed vulnerability with better matching.
#[derive(Debug, Clone)]
pub struct ProcessedVulnerability {
    inner: Vulnerability,
    constraints: Vec<VersionConstraint>,
}

impl ProcessedVulnerability {
    /// Create from raw advisory.
    fn from_advisory(advisory: RawAdvisory) -> Option<Self> {
        let package = PackageId::parse(&advisory.package_name)?;

        // Parse affected version constraints
        let constraints: Vec<_> = advisory
            .affected_versions
            .split(',')
            .filter_map(|s| VersionConstraint::parse(s.trim()))
            .collect();

        if constraints.is_empty() {
            warn!(
                package = %package,
                constraint = %advisory.affected_versions,
                "failed to parse version constraint"
            );
        } else {
            debug!(
                package = %package,
                constraints = ?constraints.iter().map(VersionConstraint::original).collect::<Vec<_>>(),
                "parsed version constraints"
            );
        }

        let references = advisory
            .link
            .as_ref()
            .and_then(|l| Url::parse(l).ok())
            .into_iter()
            .chain(advisory.sources.iter().filter_map(|s| {
                // Use source name to determine the appropriate URL format
                let url_str = match s.name.to_lowercase().as_str() {
                    "cve" => format!(
                        "https://cve.mitre.org/cgi-bin/cvename.cgi?name={}",
                        s.remote_id
                    ),
                    "github" => format!("https://github.com/advisories/{}", s.remote_id),
                    "friendsofphp/security-advisories" => format!(
                        "https://github.com/FriendsOfPHP/security-advisories/blob/master/{}",
                        s.remote_id.replace("::", "/").replace(':', "/")
                    ),
                    _ => format!(
                        "https://cve.mitre.org/cgi-bin/cvename.cgi?name={}",
                        s.remote_id
                    ),
                };
                Url::parse(&url_str).ok()
            }))
            .collect();

        let published_at = advisory
            .reported_at
            .as_ref()
            .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
            .map(|dt| dt.with_timezone(&Utc));

        let advisory_id = advisory.cve.unwrap_or(advisory.advisory_id);

        // Use severity from API response, fall back to inference from advisory ID
        let severity = advisory
            .severity
            .as_ref()
            .and_then(|s| match s.to_lowercase().as_str() {
                "critical" => Some(Severity::Critical),
                "high" => Some(Severity::High),
                "medium" | "moderate" => Some(Severity::Medium),
                "low" => Some(Severity::Low),
                _ => None,
            })
            .unwrap_or_else(|| {
                // Fallback: infer from advisory ID
                if advisory_id.contains("CRITICAL") {
                    Severity::Critical
                } else if advisory_id.contains("HIGH") {
                    Severity::High
                } else if advisory_id.contains("MEDIUM") || advisory_id.contains("MODERATE") {
                    Severity::Medium
                } else if advisory_id.contains("LOW") {
                    Severity::Low
                } else {
                    Severity::Unknown
                }
            });

        Some(Self {
            inner: Vulnerability {
                advisory_id,
                package,
                affected_versions: advisory.affected_versions,
                fixed_version: None,
                severity,
                cvss_score: None,
                title: advisory.title,
                description: String::new(),
                references,
                published_at,
            },
            constraints,
        })
    }

    /// Check if version is affected.
    #[must_use]
    pub fn affects_version(&self, version: &Version) -> bool {
        if self.constraints.is_empty() {
            // If no constraints parsed, assume all versions affected (conservative)
            return true;
        }

        // Version is affected if it matches any constraint
        self.constraints.iter().any(|c| c.matches(version))
    }

    /// Get underlying vulnerability.
    #[must_use]
    pub const fn vulnerability(&self) -> &Vulnerability {
        &self.inner
    }
}

/// Advisory database with caching.
#[derive(Debug, Clone)]
pub struct AdvisoryDatabase {
    client: Client,
    base_url: Url,
    cache: Arc<DashMap<PackageId, Vec<ProcessedVulnerability>>>,
    cache_ttl: Duration,
    last_update: Arc<RwLock<Option<Instant>>>,
}

impl AdvisoryDatabase {
    /// Create new advisory database.
    ///
    /// # Errors
    /// Returns error if client cannot be created.
    pub fn new() -> Result<Self> {
        Self::with_url(
            Url::parse("https://packagist.org/api/security-advisories/").expect("valid URL"),
        )
    }

    /// Create with custom URL.
    ///
    /// # Errors
    /// Returns error if client cannot be created.
    pub fn with_url(base_url: Url) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .https_only(true)
            .build()
            .map_err(|e| AdvisoryError::Network(e.to_string()))?;

        Ok(Self {
            client,
            base_url,
            cache: Arc::new(DashMap::new()),
            cache_ttl: Duration::from_secs(3600), // 1 hour
            last_update: Arc::new(RwLock::new(None)),
        })
    }

    /// Set cache TTL.
    #[must_use]
    pub const fn with_cache_ttl(mut self, ttl: Duration) -> Self {
        self.cache_ttl = ttl;
        self
    }

    /// Check if cache is stale.
    fn is_cache_stale(&self) -> bool {
        if let Some(last) = *self.last_update.read() {
            last.elapsed() > self.cache_ttl
        } else {
            true
        }
    }

    /// Fetch advisories for a package.
    ///
    /// # Errors
    /// Returns error if fetch fails.
    pub async fn fetch_advisories(
        &self,
        package: &PackageId,
    ) -> Result<Vec<ProcessedVulnerability>> {
        // Check cache first
        if !self.is_cache_stale()
            && let Some(cached) = self.cache.get(package)
        {
            debug!(package = %package, "using cached advisories");
            return Ok(cached.clone());
        }

        info!(package = %package, "fetching security advisories");

        let url = format!("{}?packages[]={}", self.base_url, package.full_name());

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| AdvisoryError::Network(e.to_string()))?;

        if !response.status().is_success() {
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                // No advisories is not an error
                self.cache.insert(package.clone(), Vec::new());
                return Ok(Vec::new());
            }
            return Err(AdvisoryError::Network(format!(
                "HTTP {}",
                response.status()
            )));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| AdvisoryError::Network(e.to_string()))?;

        // Response format: {"advisories": {"package/name": [advisories...]}}
        #[derive(Deserialize)]
        struct ApiResponse {
            advisories: HashMap<String, Vec<RawAdvisory>>,
        }

        let raw: ApiResponse =
            sonic_rs::from_slice(&body).map_err(|e| AdvisoryError::Parse(e.to_string()))?;

        let vulnerabilities: Vec<_> = raw
            .advisories
            .get(&package.full_name())
            .map(|advisories| {
                advisories
                    .iter()
                    .filter_map(|a| ProcessedVulnerability::from_advisory(a.clone()))
                    .collect()
            })
            .unwrap_or_default();

        info!(
            package = %package,
            count = vulnerabilities.len(),
            "fetched advisories"
        );

        // Update cache
        self.cache.insert(package.clone(), vulnerabilities.clone());
        *self.last_update.write() = Some(Instant::now());

        Ok(vulnerabilities)
    }

    /// Check specific package version for vulnerabilities.
    ///
    /// # Errors
    /// Returns error if fetch fails.
    pub async fn check_version(
        &self,
        package: &PackageId,
        version: &Version,
    ) -> Result<Vec<Vulnerability>> {
        let advisories = self.fetch_advisories(package).await?;

        let affected: Vec<_> = advisories
            .iter()
            .filter(|adv| adv.affects_version(version))
            .map(|adv| adv.vulnerability().clone())
            .collect();

        if !affected.is_empty() {
            warn!(
                package = %package,
                version = %version,
                count = affected.len(),
                "found vulnerabilities"
            );
        }

        Ok(affected)
    }

    /// Bulk check multiple packages.
    ///
    /// # Errors
    /// Returns error if any fetch fails.
    pub async fn check_packages(
        &self,
        packages: &[(PackageId, Version)],
    ) -> Result<AHashMap<PackageId, Vec<Vulnerability>>> {
        let mut results = AHashMap::with_capacity(packages.len());

        // Use concurrent fetching with rate limiting
        let semaphore = Arc::new(tokio::sync::Semaphore::new(10)); // Max 10 concurrent

        let mut tasks = Vec::with_capacity(packages.len());

        for (package, version) in packages {
            let db = self.clone();
            let package = package.clone();
            let version = version.clone();
            let permit = semaphore.clone();

            tasks.push(tokio::spawn(async move {
                let _guard = permit.acquire().await.expect("semaphore");
                db.check_version(&package, &version)
                    .await
                    .map(|vulns| (package, vulns))
            }));
        }

        for task in tasks {
            if let Ok(Ok((package, vulns))) = task.await
                && !vulns.is_empty()
            {
                results.insert(package, vulns);
            }
        }

        Ok(results)
    }

    /// Clear cache.
    pub fn clear_cache(&self) {
        self.cache.clear();
        *self.last_update.write() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_constraint_parsing() {
        let constraint = VersionConstraint::parse(">=1.0.0,<2.0.0");
        assert!(constraint.is_some());

        let constraint = VersionConstraint::parse("^1.2.3");
        assert!(constraint.is_some());
    }

    #[test]
    fn test_version_matching() {
        let constraint = VersionConstraint::parse(">=1.0.0").unwrap();
        assert!(constraint.matches(&Version::parse("1.0.0").unwrap()));
        assert!(constraint.matches(&Version::parse("2.0.0").unwrap()));
        assert!(!constraint.matches(&Version::parse("0.9.0").unwrap()));
    }

    #[test]
    fn test_cache_staleness() {
        let db = AdvisoryDatabase::new().unwrap();
        assert!(db.is_cache_stale());

        *db.last_update.write() = Some(Instant::now());
        assert!(!db.is_cache_stale());
    }
}
