//! VCS provider integrations (GitHub, GitLab, Bitbucket).
//!
//! This module provides API clients for fetching package metadata from
//! version control hosting services.

mod bitbucket;
mod github;
mod gitlab;

pub use bitbucket::{BitbucketClient, BitbucketConfig};
pub use github::{GitHubClient, GitHubConfig};
pub use gitlab::{GitLabClient, GitLabConfig};

use crate::error::Result;
use url::Url;

use std::future::Future;
use std::pin::Pin;

/// Trait for VCS providers that can fetch composer.json from repositories.
pub trait VcsProvider: Send + Sync {
    /// Get the provider name.
    fn name(&self) -> &str;

    /// Check if this provider can handle the given URL.
    fn can_handle(&self, url: &Url) -> bool;

    /// Fetch composer.json from a repository.
    ///
    /// # Arguments
    /// * `url` - Repository URL.
    /// * `reference` - Branch, tag, or commit reference.
    ///
    /// # Errors
    /// Returns error if composer.json cannot be fetched.
    fn fetch_composer_json<'a>(
        &'a self,
        url: &'a Url,
        reference: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;

    /// List available tags/releases.
    ///
    /// # Errors
    /// Returns error if tags cannot be fetched.
    fn list_tags<'a>(
        &'a self,
        url: &'a Url,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

    /// List branches.
    ///
    /// # Errors
    /// Returns error if branches cannot be fetched.
    fn list_branches<'a>(
        &'a self,
        url: &'a Url,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<String>>> + Send + 'a>>;

    /// Get the default branch.
    ///
    /// # Errors
    /// Returns error if default branch cannot be determined.
    fn get_default_branch<'a>(
        &'a self,
        url: &'a Url,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

/// Parse a VCS URL to extract owner and repository name.
#[must_use]
pub fn parse_vcs_url(url: &Url) -> Option<(String, String)> {
    let path = url.path().trim_start_matches('/').trim_end_matches(".git");
    let parts: Vec<&str> = path.split('/').collect();

    if parts.len() >= 2 {
        Some((parts[0].to_string(), parts[1].to_string()))
    } else {
        None
    }
}

/// Detect the VCS provider from a URL.
#[must_use]
pub fn detect_provider(url: &Url) -> Option<ProviderType> {
    let host = url.host_str()?;

    if host.contains("github.com") || host.contains("github.") {
        Some(ProviderType::GitHub)
    } else if host.contains("gitlab.com") || host.contains("gitlab.") {
        Some(ProviderType::GitLab)
    } else if host.contains("bitbucket.org") || host.contains("bitbucket.") {
        Some(ProviderType::Bitbucket)
    } else {
        None
    }
}

/// VCS provider type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderType {
    /// GitHub (github.com or GitHub Enterprise).
    GitHub,
    /// GitLab (gitlab.com or self-hosted).
    GitLab,
    /// Bitbucket (bitbucket.org or Bitbucket Server).
    Bitbucket,
}

impl std::fmt::Display for ProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitHub => write!(f, "GitHub"),
            Self::GitLab => write!(f, "GitLab"),
            Self::Bitbucket => write!(f, "Bitbucket"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_vcs_url() {
        let url = Url::parse("https://github.com/symfony/console").unwrap();
        let (owner, repo) = parse_vcs_url(&url).unwrap();
        assert_eq!(owner, "symfony");
        assert_eq!(repo, "console");

        let url = Url::parse("https://github.com/symfony/console.git").unwrap();
        let (owner, repo) = parse_vcs_url(&url).unwrap();
        assert_eq!(owner, "symfony");
        assert_eq!(repo, "console");
    }

    #[test]
    fn test_detect_provider() {
        let github = Url::parse("https://github.com/owner/repo").unwrap();
        assert_eq!(detect_provider(&github), Some(ProviderType::GitHub));

        let gitlab = Url::parse("https://gitlab.com/owner/repo").unwrap();
        assert_eq!(detect_provider(&gitlab), Some(ProviderType::GitLab));

        let bitbucket = Url::parse("https://bitbucket.org/owner/repo").unwrap();
        assert_eq!(detect_provider(&bitbucket), Some(ProviderType::Bitbucket));

        let unknown = Url::parse("https://example.com/owner/repo").unwrap();
        assert_eq!(detect_provider(&unknown), None);
    }
}
