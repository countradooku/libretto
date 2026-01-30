//! Version constraint handling (Composer-compatible).

use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Composer-compatible version constraint.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct VersionConstraint {
    raw: String,
}

impl VersionConstraint {
    /// Create from raw string.
    #[must_use]
    pub fn new(constraint: impl Into<String>) -> Self {
        Self {
            raw: constraint.into(),
        }
    }

    /// Any version.
    #[must_use]
    pub fn any() -> Self {
        Self::new("*")
    }

    /// Exact version.
    #[must_use]
    pub fn exact(version: &Version) -> Self {
        Self::new(version.to_string())
    }

    /// Get raw constraint string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.raw
    }

    /// Check if version matches.
    #[must_use]
    pub fn matches(&self, version: &Version) -> bool {
        if self.raw == "*" {
            return true;
        }
        self.to_semver_req().is_some_and(|req| req.matches(version))
    }

    /// Convert to semver `VersionReq`.
    fn to_semver_req(&self) -> Option<VersionReq> {
        let normalized = self.normalize_constraint();
        VersionReq::parse(&normalized).ok()
    }

    /// Normalize Composer constraint to semver.
    fn normalize_constraint(&self) -> String {
        let s = self.raw.trim();

        // Handle * wildcard
        if s == "*" {
            return "*".to_string();
        }

        // Handle .* wildcard patterns (e.g., "3.*", "7.*", "1.2.*")
        // Note: ".x" is a version wildcard, not a file extension
        #[allow(clippy::case_sensitive_file_extension_comparisons)]
        if s.ends_with(".*") || s.ends_with(".x") {
            let prefix = &s[..s.len() - 2];
            let parts: Vec<&str> = prefix.split('.').collect();
            return match parts.len() {
                // "3.*" -> ">=3.0.0, <4.0.0"
                1 => format!(
                    ">={}.0.0, <{}.0.0",
                    parts[0],
                    parts[0].parse::<u64>().unwrap_or(0) + 1
                ),
                // "3.1.*" -> ">=3.1.0, <3.2.0"
                2 => format!(
                    ">={}.{}.0, <{}.{}.0",
                    parts[0],
                    parts[1],
                    parts[0],
                    parts[1].parse::<u64>().unwrap_or(0) + 1
                ),
                _ => s.to_string(),
            };
        }

        // Handle ^ (caret)
        if let Some(rest) = s.strip_prefix('^') {
            return format!("^{}", Self::normalize_version(rest));
        }

        // Handle ~ (tilde)
        if let Some(rest) = s.strip_prefix('~') {
            return format!("~{}", Self::normalize_version(rest));
        }

        // Handle >= <= > <
        if s.starts_with(">=")
            || s.starts_with("<=")
            || s.starts_with('>')
            || s.starts_with('<')
            || s.starts_with('=')
        {
            return s.to_string();
        }

        // Handle || or | (OR) - Composer supports both
        if s.contains("||") {
            return s
                .split("||")
                .map(|p| Self::new(p.trim()).normalize_constraint())
                .collect::<Vec<_>>()
                .join(" || ");
        }
        if s.contains('|') && !s.contains("||") {
            return s
                .split('|')
                .map(|p| Self::new(p.trim()).normalize_constraint())
                .collect::<Vec<_>>()
                .join(" || ");
        }

        // Handle space/comma (AND)
        if s.contains(',') {
            return s
                .split(',')
                .map(|p| Self::new(p.trim()).normalize_constraint())
                .collect::<Vec<_>>()
                .join(", ");
        }

        // Bare version = exact match
        format!("={}", Self::normalize_version(s))
    }

    /// Normalize version string.
    fn normalize_version(v: &str) -> String {
        let v = v.trim().trim_start_matches('v');

        // Count dots
        let dots = v.chars().filter(|&c| c == '.').count();

        match dots {
            0 => format!("{v}.0.0"),
            1 => format!("{v}.0"),
            _ => v.to_string(),
        }
    }
}

impl Default for VersionConstraint {
    fn default() -> Self {
        Self::any()
    }
}

impl fmt::Display for VersionConstraint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.raw)
    }
}

impl FromStr for VersionConstraint {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard() {
        let c = VersionConstraint::any();
        assert!(c.matches(&Version::new(1, 0, 0)));
        assert!(c.matches(&Version::new(99, 99, 99)));
    }

    #[test]
    fn major_wildcard() {
        // Test 3.* pattern
        let c = VersionConstraint::new("3.*");
        assert!(c.matches(&Version::new(3, 0, 0)));
        assert!(c.matches(&Version::new(3, 11, 0)));
        assert!(c.matches(&Version::new(3, 99, 99)));
        assert!(!c.matches(&Version::new(2, 0, 0)));
        assert!(!c.matches(&Version::new(4, 0, 0)));

        // Test 7.* pattern
        let c7 = VersionConstraint::new("7.*");
        assert!(c7.matches(&Version::new(7, 0, 0)));
        assert!(c7.matches(&Version::new(7, 17, 0)));
        assert!(!c7.matches(&Version::new(8, 0, 0)));
    }

    #[test]
    fn minor_wildcard() {
        // Test 3.1.* pattern
        let c = VersionConstraint::new("3.1.*");
        assert!(c.matches(&Version::new(3, 1, 0)));
        assert!(c.matches(&Version::new(3, 1, 99)));
        assert!(!c.matches(&Version::new(3, 0, 0)));
        assert!(!c.matches(&Version::new(3, 2, 0)));
    }

    #[test]
    fn caret() {
        let c = VersionConstraint::new("^1.2");
        assert!(c.matches(&Version::new(1, 2, 0)));
        assert!(c.matches(&Version::new(1, 9, 9)));
        assert!(!c.matches(&Version::new(2, 0, 0)));
    }

    #[test]
    fn tilde() {
        let c = VersionConstraint::new("~1.2");
        assert!(c.matches(&Version::new(1, 2, 0)));
        assert!(c.matches(&Version::new(1, 2, 9)));
        assert!(!c.matches(&Version::new(1, 3, 0)));
    }

    #[test]
    fn exact() {
        let c = VersionConstraint::exact(&Version::new(1, 2, 3));
        assert!(c.matches(&Version::new(1, 2, 3)));
        assert!(!c.matches(&Version::new(1, 2, 4)));
    }
}
