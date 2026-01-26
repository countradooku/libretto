//! Security auditing for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use chrono::{DateTime, Utc};
use libretto_core::{Error, PackageId, Result, Version};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, info, warn};
use url::Url;

/// Vulnerability severity level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    /// Unknown severity.
    Unknown,
    /// Low severity.
    Low,
    /// Medium severity.
    Medium,
    /// High severity.
    High,
    /// Critical severity.
    Critical,
}

impl Severity {
    /// Get severity from CVSS score.
    #[must_use]
    pub fn from_cvss(score: f32) -> Self {
        match score {
            s if s >= 9.0 => Self::Critical,
            s if s >= 7.0 => Self::High,
            s if s >= 4.0 => Self::Medium,
            s if s > 0.0 => Self::Low,
            _ => Self::Unknown,
        }
    }

    /// Get display color (ANSI).
    #[must_use]
    pub const fn color(&self) -> &'static str {
        match self {
            Self::Critical => "\x1b[91m", // Bright red
            Self::High => "\x1b[31m",     // Red
            Self::Medium => "\x1b[33m",   // Yellow
            Self::Low => "\x1b[36m",      // Cyan
            Self::Unknown => "\x1b[37m",  // White
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Critical => write!(f, "CRITICAL"),
            Self::High => write!(f, "HIGH"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::Low => write!(f, "LOW"),
            Self::Unknown => write!(f, "UNKNOWN"),
        }
    }
}

/// Security vulnerability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vulnerability {
    /// Advisory ID (e.g., CVE-2024-1234).
    pub advisory_id: String,
    /// Affected package.
    pub package: PackageId,
    /// Affected version range.
    pub affected_versions: String,
    /// Fixed in version (if known).
    pub fixed_version: Option<Version>,
    /// Severity level.
    pub severity: Severity,
    /// CVSS score (0-10).
    pub cvss_score: Option<f32>,
    /// Title/summary.
    pub title: String,
    /// Description.
    pub description: String,
    /// Reference URLs.
    pub references: Vec<Url>,
    /// Published date.
    pub published_at: Option<DateTime<Utc>>,
}

impl Vulnerability {
    /// Check if a version is affected.
    #[must_use]
    pub fn affects_version(&self, version: &Version) -> bool {
        // Simple check - in production would use proper constraint parsing
        if let Some(ref fixed) = self.fixed_version {
            version < fixed
        } else {
            true
        }
    }
}

/// Audit result for a single package.
#[derive(Debug, Clone)]
pub struct PackageAudit {
    /// Package identifier.
    pub package: PackageId,
    /// Package version.
    pub version: Version,
    /// Found vulnerabilities.
    pub vulnerabilities: Vec<Vulnerability>,
}

impl PackageAudit {
    /// Check if package is vulnerable.
    #[must_use]
    pub fn is_vulnerable(&self) -> bool {
        !self.vulnerabilities.is_empty()
    }

    /// Get highest severity.
    #[must_use]
    pub fn max_severity(&self) -> Severity {
        self.vulnerabilities
            .iter()
            .map(|v| v.severity)
            .max()
            .unwrap_or(Severity::Unknown)
    }
}

/// Full audit report.
#[derive(Debug, Clone)]
pub struct AuditReport {
    /// Audited packages.
    pub packages: Vec<PackageAudit>,
    /// When audit was performed.
    pub audited_at: DateTime<Utc>,
    /// Advisory database version.
    pub database_version: Option<String>,
}

impl AuditReport {
    /// Get total vulnerability count.
    #[must_use]
    pub fn vulnerability_count(&self) -> usize {
        self.packages.iter().map(|p| p.vulnerabilities.len()).sum()
    }

    /// Get vulnerable package count.
    #[must_use]
    pub fn vulnerable_package_count(&self) -> usize {
        self.packages.iter().filter(|p| p.is_vulnerable()).count()
    }

    /// Check if any critical vulnerabilities exist.
    #[must_use]
    pub fn has_critical(&self) -> bool {
        self.packages
            .iter()
            .flat_map(|p| &p.vulnerabilities)
            .any(|v| v.severity == Severity::Critical)
    }

    /// Get all vulnerabilities grouped by severity.
    #[must_use]
    pub fn by_severity(&self) -> Vec<(Severity, Vec<&Vulnerability>)> {
        let mut by_sev: std::collections::BTreeMap<Severity, Vec<&Vulnerability>> =
            std::collections::BTreeMap::new();

        for pkg in &self.packages {
            for vuln in &pkg.vulnerabilities {
                by_sev.entry(vuln.severity).or_default().push(vuln);
            }
        }

        by_sev.into_iter().rev().collect()
    }
}

/// Security auditor.
#[derive(Debug)]
pub struct Auditor {
    client: Client,
    advisory_url: Url,
}

impl Auditor {
    /// Create new auditor with default advisory database.
    ///
    /// # Errors
    /// Returns error if HTTP client cannot be created.
    pub fn new() -> Result<Self> {
        Self::with_advisory_url(
            Url::parse("https://packagist.org/api/security-advisories/").expect("valid url"),
        )
    }

    /// Create auditor with custom advisory URL.
    ///
    /// # Errors
    /// Returns error if HTTP client cannot be created.
    pub fn with_advisory_url(advisory_url: Url) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| Error::Network(e.to_string()))?;

        Ok(Self {
            client,
            advisory_url,
        })
    }

    /// Audit a list of packages.
    ///
    /// # Errors
    /// Returns error if audit fails.
    pub async fn audit(&self, packages: &[(PackageId, Version)]) -> Result<AuditReport> {
        info!(packages = packages.len(), "starting security audit");

        let mut audits = Vec::with_capacity(packages.len());

        for (package_id, version) in packages {
            let vulnerabilities = self.check_package(package_id, version).await?;

            if !vulnerabilities.is_empty() {
                warn!(
                    package = %package_id,
                    version = %version,
                    count = vulnerabilities.len(),
                    "vulnerabilities found"
                );
            }

            audits.push(PackageAudit {
                package: package_id.clone(),
                version: version.clone(),
                vulnerabilities,
            });
        }

        let report = AuditReport {
            packages: audits,
            audited_at: Utc::now(),
            database_version: None,
        };

        info!(
            total = report.vulnerability_count(),
            packages = report.vulnerable_package_count(),
            "audit complete"
        );

        Ok(report)
    }

    async fn check_package(
        &self,
        package_id: &PackageId,
        version: &Version,
    ) -> Result<Vec<Vulnerability>> {
        debug!(package = %package_id, version = %version, "checking package");

        let url = format!(
            "{}?packages[{}]={}",
            self.advisory_url,
            package_id.full_name(),
            version
        );

        let response = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if !response.status().is_success() {
            // No advisories found is not an error
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Ok(Vec::new());
            }
            return Err(Error::Network(format!("HTTP {}", response.status())));
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        let advisory_response: AdvisoryResponse = sonic_rs::from_slice(&body).unwrap_or_default();

        let vulnerabilities = advisory_response
            .advisories
            .into_values()
            .flatten()
            .filter_map(|a| convert_advisory(package_id, a))
            .filter(|v| v.affects_version(version))
            .collect();

        Ok(vulnerabilities)
    }
}

#[derive(Debug, Default, Deserialize)]
struct AdvisoryResponse {
    #[serde(default)]
    advisories: std::collections::HashMap<String, Vec<Advisory>>,
}

#[derive(Debug, Deserialize)]
struct Advisory {
    #[serde(rename = "advisoryId")]
    advisory_id: String,
    #[serde(rename = "affectedVersions")]
    affected_versions: String,
    title: String,
    #[serde(default)]
    cve: Option<String>,
    #[serde(default)]
    link: Option<String>,
}

fn convert_advisory(package_id: &PackageId, a: Advisory) -> Option<Vulnerability> {
    let advisory_id = a.cve.unwrap_or(a.advisory_id);

    let references = a
        .link
        .and_then(|l| Url::parse(&l).ok())
        .map(|u| vec![u])
        .unwrap_or_default();

    Some(Vulnerability {
        advisory_id,
        package: package_id.clone(),
        affected_versions: a.affected_versions,
        fixed_version: None,
        severity: Severity::Unknown,
        cvss_score: None,
        title: a.title,
        description: String::new(),
        references,
        published_at: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn severity_from_cvss() {
        assert_eq!(Severity::from_cvss(9.5), Severity::Critical);
        assert_eq!(Severity::from_cvss(7.5), Severity::High);
        assert_eq!(Severity::from_cvss(5.0), Severity::Medium);
        assert_eq!(Severity::from_cvss(2.0), Severity::Low);
        assert_eq!(Severity::from_cvss(0.0), Severity::Unknown);
    }

    #[test]
    fn severity_ordering() {
        assert!(Severity::Critical > Severity::High);
        assert!(Severity::High > Severity::Medium);
        assert!(Severity::Medium > Severity::Low);
    }

    #[test]
    fn empty_audit_report() {
        let report = AuditReport {
            packages: Vec::new(),
            audited_at: Utc::now(),
            database_version: None,
        };
        assert_eq!(report.vulnerability_count(), 0);
        assert!(!report.has_critical());
    }
}
