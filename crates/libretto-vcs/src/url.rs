//! Git URL parsing with protocol detection and hosting service shortcuts.

use crate::error::{Result, VcsError};
use crate::types::VcsType;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fmt;
use url::Url;

/// Supported Git protocols.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitProtocol {
    /// HTTPS protocol.
    Https,
    /// SSH protocol.
    Ssh,
    /// Git protocol (git://).
    Git,
    /// Local file protocol.
    File,
}

impl GitProtocol {
    /// Check if this protocol requires authentication.
    #[must_use]
    pub fn requires_auth(&self) -> bool {
        matches!(self, Self::Https | Self::Ssh)
    }

    /// Check if this is a secure protocol.
    #[must_use]
    pub fn is_secure(&self) -> bool {
        matches!(self, Self::Https | Self::Ssh)
    }

    /// Get the default port for this protocol.
    #[must_use]
    pub fn default_port(&self) -> Option<u16> {
        match self {
            Self::Https => Some(443),
            Self::Ssh => Some(22),
            Self::Git => Some(9418),
            Self::File => None,
        }
    }
}

impl fmt::Display for GitProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Https => write!(f, "https"),
            Self::Ssh => write!(f, "ssh"),
            Self::Git => write!(f, "git"),
            Self::File => write!(f, "file"),
        }
    }
}

/// Known Git hosting services.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GitHosting {
    /// GitHub.
    GitHub,
    /// GitLab.
    GitLab,
    /// Bitbucket.
    Bitbucket,
    /// Azure DevOps.
    AzureDevOps,
    /// Gitea instance.
    Gitea,
    /// Self-hosted or unknown.
    Other,
}

impl GitHosting {
    /// Detect hosting from hostname.
    #[must_use]
    pub fn from_host(host: &str) -> Self {
        let host_lower = host.to_lowercase();
        if host_lower.contains("github.com") {
            Self::GitHub
        } else if host_lower.contains("gitlab.com") || host_lower.contains("gitlab") {
            Self::GitLab
        } else if host_lower.contains("bitbucket.org") || host_lower.contains("bitbucket") {
            Self::Bitbucket
        } else if host_lower.contains("dev.azure.com") || host_lower.contains("visualstudio.com") {
            Self::AzureDevOps
        } else if host_lower.contains("gitea") || host_lower.contains("forgejo") {
            Self::Gitea
        } else {
            Self::Other
        }
    }

    /// Get the archive download URL pattern for this hosting service.
    #[must_use]
    pub fn archive_url_pattern(&self) -> Option<&'static str> {
        match self {
            Self::GitHub => Some("https://github.com/{owner}/{repo}/archive/{ref}.zip"),
            Self::GitLab => {
                Some("https://gitlab.com/{owner}/{repo}/-/archive/{ref}/{repo}-{ref}.zip")
            }
            Self::Bitbucket => Some("https://bitbucket.org/{owner}/{repo}/get/{ref}.zip"),
            _ => None,
        }
    }
}

/// Parsed VCS URL with rich metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsUrl {
    /// Original URL string.
    pub original: String,
    /// Normalized URL.
    pub normalized: String,
    /// VCS type.
    pub vcs_type: VcsType,
    /// Protocol.
    pub protocol: GitProtocol,
    /// Host.
    pub host: Option<String>,
    /// Port (if non-default).
    pub port: Option<u16>,
    /// Owner/organization.
    pub owner: Option<String>,
    /// Repository name.
    pub repo: Option<String>,
    /// Hosting service.
    pub hosting: GitHosting,
    /// Path within the URL.
    pub path: String,
    /// Username from URL.
    pub username: Option<String>,
}

// Regex patterns for URL parsing
static SSH_URL_REGEX: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^(?:(?P<user>[^@]+)@)?(?P<host>[^:/]+):(?P<path>.+)$").expect("invalid ssh regex")
});

static GITHUB_SHORTHAND: Lazy<Regex> = Lazy::new(|| {
    // Only matches owner/repo without any prefix (no colons, no @)
    Regex::new(r"^(?P<owner>[a-zA-Z0-9_.-]+)/(?P<repo>[a-zA-Z0-9_.-]+)(?:\.git)?$")
        .expect("invalid github regex")
});

static GITHUB_EXPLICIT_SHORTHAND: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^github:(?P<owner>[^/]+)/(?P<repo>[^/]+)(?:\.git)?$")
        .expect("invalid github explicit regex")
});

static GITLAB_SHORTHAND: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^gitlab:(?P<owner>[^/]+)/(?P<repo>[^/]+)(?:\.git)?$")
        .expect("invalid gitlab regex")
});

static BITBUCKET_SHORTHAND: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"^bitbucket:(?P<owner>[^/]+)/(?P<repo>[^/]+)(?:\.git)?$")
        .expect("invalid bitbucket regex")
});

impl VcsUrl {
    /// Parse a VCS URL string.
    ///
    /// Supports:
    /// - Standard URLs: `https://github.com/owner/repo.git`
    /// - SSH URLs: `git@github.com:owner/repo.git`
    /// - Git protocol: `git://github.com/owner/repo.git`
    /// - File URLs: `file:///path/to/repo`
    /// - Shortcuts: `github:owner/repo`, `gitlab:owner/repo`, `bitbucket:owner/repo`
    /// - Simple: `owner/repo` (assumes GitHub)
    ///
    /// # Errors
    /// Returns error if URL cannot be parsed.
    pub fn parse(url: &str) -> Result<Self> {
        let url = url.trim();

        // 1. Try explicit shorthand prefixes first (gitlab:, bitbucket:, github:)
        // These have explicit prefixes so they're unambiguous

        // Try GitLab shorthand (gitlab:owner/repo)
        if let Some(caps) = GITLAB_SHORTHAND.captures(url) {
            let owner = caps.name("owner").map(|m| m.as_str().to_string());
            let repo = caps.name("repo").map(|m| m.as_str().to_string());
            let owner_str = owner.as_deref().unwrap_or("");
            let repo_str = repo.as_deref().unwrap_or("");
            let normalized = format!("https://gitlab.com/{owner_str}/{repo_str}.git");
            let path = format!("/{owner_str}/{repo_str}.git");
            return Ok(Self {
                original: url.to_string(),
                normalized,
                vcs_type: VcsType::Git,
                protocol: GitProtocol::Https,
                host: Some("gitlab.com".to_string()),
                port: None,
                owner,
                repo,
                hosting: GitHosting::GitLab,
                path,
                username: None,
            });
        }

        // Try Bitbucket shorthand (bitbucket:owner/repo)
        if let Some(caps) = BITBUCKET_SHORTHAND.captures(url) {
            let owner = caps.name("owner").map(|m| m.as_str().to_string());
            let repo = caps.name("repo").map(|m| m.as_str().to_string());
            let owner_str = owner.as_deref().unwrap_or("");
            let repo_str = repo.as_deref().unwrap_or("");
            let normalized = format!("https://bitbucket.org/{owner_str}/{repo_str}.git");
            let path = format!("/{owner_str}/{repo_str}.git");
            return Ok(Self {
                original: url.to_string(),
                normalized,
                vcs_type: VcsType::Git,
                protocol: GitProtocol::Https,
                host: Some("bitbucket.org".to_string()),
                port: None,
                owner,
                repo,
                hosting: GitHosting::Bitbucket,
                path,
                username: None,
            });
        }

        // Try explicit GitHub shorthand (github:owner/repo)
        if let Some(caps) = GITHUB_EXPLICIT_SHORTHAND.captures(url) {
            let owner = caps.name("owner").map(|m| m.as_str().to_string());
            let repo = caps.name("repo").map(|m| m.as_str().to_string());
            let owner_str = owner.as_deref().unwrap_or("");
            let repo_str = repo.as_deref().unwrap_or("");
            let normalized = format!("https://github.com/{owner_str}/{repo_str}.git");
            let path = format!("/{owner_str}/{repo_str}.git");
            return Ok(Self {
                original: url.to_string(),
                normalized,
                vcs_type: VcsType::Git,
                protocol: GitProtocol::Https,
                host: Some("github.com".to_string()),
                port: None,
                owner,
                repo,
                hosting: GitHosting::GitHub,
                path,
                username: None,
            });
        }

        // 2. Try standard URL parsing (for URLs with scheme like https://, git://, etc.)
        if url.contains("://") {
            let parsed = Url::parse(url).map_err(|e| VcsError::InvalidUrl {
                url: url.to_string(),
                reason: e.to_string(),
            })?;

            let protocol = match parsed.scheme() {
                "https" | "http" => GitProtocol::Https,
                "ssh" => GitProtocol::Ssh,
                "git" => GitProtocol::Git,
                "file" => GitProtocol::File,
                scheme => {
                    return Err(VcsError::InvalidUrl {
                        url: url.to_string(),
                        reason: format!("unsupported scheme: {scheme}"),
                    });
                }
            };

            let host = parsed.host_str().map(ToString::to_string);
            let port = parsed.port();
            let username = if parsed.username().is_empty() {
                None
            } else {
                Some(parsed.username().to_string())
            };

            let path = parsed.path().to_string();
            let (owner, repo) = Self::extract_owner_repo(&path);
            let hosting = host
                .as_deref()
                .map(GitHosting::from_host)
                .unwrap_or(GitHosting::Other);

            return Ok(Self {
                original: url.to_string(),
                normalized: parsed.to_string(),
                vcs_type: VcsType::Git,
                protocol,
                host,
                port,
                owner,
                repo,
                hosting,
                path,
                username,
            });
        }

        // Try SSH URL format (git@host:path) - only for URLs without scheme
        if let Some(caps) = SSH_URL_REGEX.captures(url) {
            let host = caps.name("host").map(|m| m.as_str().to_string());
            let path = caps
                .name("path")
                .map(|m| m.as_str().to_string())
                .unwrap_or_default();
            let username = caps.name("user").map(|m| m.as_str().to_string());

            let (owner, repo) = Self::extract_owner_repo(&path);
            let hosting = host
                .as_deref()
                .map(GitHosting::from_host)
                .unwrap_or(GitHosting::Other);

            let normalized = format!(
                "ssh://{}@{}/{}",
                username.as_deref().unwrap_or("git"),
                host.as_deref().unwrap_or(""),
                path
            );

            return Ok(Self {
                original: url.to_string(),
                normalized,
                vcs_type: VcsType::Git,
                protocol: GitProtocol::Ssh,
                host,
                port: None,
                owner,
                repo,
                hosting,
                path: format!("/{path}"),
                username,
            });
        }

        // 4. Try simple GitHub shorthand (owner/repo) - last resort
        if let Some(caps) = GITHUB_SHORTHAND.captures(url) {
            let owner = caps.name("owner").map(|m| m.as_str().to_string());
            let repo = caps.name("repo").map(|m| m.as_str().to_string());
            let owner_str = owner.as_deref().unwrap_or("");
            let repo_str = repo.as_deref().unwrap_or("");
            let normalized = format!("https://github.com/{owner_str}/{repo_str}.git");
            let path = format!("/{owner_str}/{repo_str}.git");
            return Ok(Self {
                original: url.to_string(),
                normalized,
                vcs_type: VcsType::Git,
                protocol: GitProtocol::Https,
                host: Some("github.com".to_string()),
                port: None,
                owner,
                repo,
                hosting: GitHosting::GitHub,
                path,
                username: None,
            });
        }

        // If nothing matched, return an error
        Err(VcsError::InvalidUrl {
            url: url.to_string(),
            reason: "could not parse URL".to_string(),
        })
    }

    /// Extract owner and repo from a path.
    fn extract_owner_repo(path: &str) -> (Option<String>, Option<String>) {
        let path = path.trim_start_matches('/').trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();

        match parts.as_slice() {
            [owner, repo, ..] => (Some((*owner).to_string()), Some((*repo).to_string())),
            [repo] if !repo.is_empty() => (None, Some((*repo).to_string())),
            _ => (None, None),
        }
    }

    /// Convert to HTTPS URL.
    #[must_use]
    pub fn to_https(&self) -> String {
        if self.protocol == GitProtocol::Https {
            return self.normalized.clone();
        }

        let host = self.host.as_deref().unwrap_or("github.com");
        let port_str = self.port.map(|p| format!(":{p}")).unwrap_or_default();
        format!("https://{host}{port_str}{}", self.path)
    }

    /// Convert to SSH URL.
    #[must_use]
    pub fn to_ssh(&self) -> String {
        if self.protocol == GitProtocol::Ssh {
            return self.normalized.clone();
        }

        let host = self.host.as_deref().unwrap_or("github.com");
        let path = self.path.trim_start_matches('/');
        format!("git@{host}:{path}")
    }

    /// Get the canonical repository identifier.
    #[must_use]
    pub fn repository_id(&self) -> String {
        match (&self.owner, &self.repo) {
            (Some(owner), Some(repo)) => format!("{owner}/{repo}"),
            (None, Some(repo)) => repo.clone(),
            _ => self
                .path
                .trim_start_matches('/')
                .trim_end_matches(".git")
                .to_string(),
        }
    }

    /// Get archive download URL if supported.
    #[must_use]
    pub fn archive_url(&self, reference: &str) -> Option<String> {
        let pattern = self.hosting.archive_url_pattern()?;
        let owner = self.owner.as_deref()?;
        let repo = self.repo.as_deref()?;

        Some(
            pattern
                .replace("{owner}", owner)
                .replace("{repo}", repo)
                .replace("{ref}", reference),
        )
    }

    /// Check if this URL requires authentication.
    #[must_use]
    pub fn requires_auth(&self) -> bool {
        self.protocol.requires_auth()
    }
}

impl fmt::Display for VcsUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.normalized)
    }
}

impl std::str::FromStr for VcsUrl {
    type Err = VcsError;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_https_url() {
        let url = VcsUrl::parse("https://github.com/owner/repo.git").unwrap();
        assert_eq!(url.protocol, GitProtocol::Https);
        assert_eq!(url.host, Some("github.com".to_string()));
        assert_eq!(url.owner, Some("owner".to_string()));
        assert_eq!(url.repo, Some("repo".to_string()));
        assert_eq!(url.hosting, GitHosting::GitHub);
    }

    #[test]
    fn parse_ssh_url() {
        let url = VcsUrl::parse("git@github.com:owner/repo.git").unwrap();
        assert_eq!(url.protocol, GitProtocol::Ssh);
        assert_eq!(url.host, Some("github.com".to_string()));
        assert_eq!(url.owner, Some("owner".to_string()));
        assert_eq!(url.repo, Some("repo".to_string()));
    }

    #[test]
    fn parse_github_shorthand() {
        let url = VcsUrl::parse("owner/repo").unwrap();
        assert_eq!(url.protocol, GitProtocol::Https);
        assert_eq!(url.hosting, GitHosting::GitHub);
        assert_eq!(url.owner, Some("owner".to_string()));
        assert_eq!(url.repo, Some("repo".to_string()));
    }

    #[test]
    fn parse_gitlab_shorthand() {
        let url = VcsUrl::parse("gitlab:owner/repo").unwrap();
        assert_eq!(url.protocol, GitProtocol::Https);
        assert_eq!(url.hosting, GitHosting::GitLab);
        assert_eq!(url.owner, Some("owner".to_string()));
        assert_eq!(url.repo, Some("repo".to_string()));
    }

    #[test]
    fn parse_git_protocol() {
        let url = VcsUrl::parse("git://github.com/owner/repo.git").unwrap();
        assert_eq!(url.protocol, GitProtocol::Git);
    }

    #[test]
    fn parse_file_protocol() {
        let url = VcsUrl::parse("file:///path/to/repo").unwrap();
        assert_eq!(url.protocol, GitProtocol::File);
    }

    #[test]
    fn to_https_conversion() {
        let url = VcsUrl::parse("git@github.com:owner/repo.git").unwrap();
        assert!(url.to_https().starts_with("https://"));
    }

    #[test]
    fn to_ssh_conversion() {
        let url = VcsUrl::parse("https://github.com/owner/repo.git").unwrap();
        assert!(url.to_ssh().starts_with("git@"));
    }

    #[test]
    fn archive_url_github() {
        let url = VcsUrl::parse("https://github.com/owner/repo.git").unwrap();
        let archive = url.archive_url("v1.0.0").unwrap();
        assert!(archive.contains("archive"));
        assert!(archive.contains("v1.0.0"));
    }

    #[test]
    fn repository_id() {
        let url = VcsUrl::parse("https://github.com/symfony/console.git").unwrap();
        assert_eq!(url.repository_id(), "symfony/console");
    }
}
