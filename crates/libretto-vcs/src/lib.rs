//! Version control operations for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use libretto_core::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};
use url::Url;

/// Git reference type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRef {
    /// Branch name.
    Branch(String),
    /// Tag name.
    Tag(String),
    /// Commit SHA.
    Commit(String),
}

impl GitRef {
    /// Parse reference string.
    #[must_use]
    pub fn parse(reference: &str) -> Self {
        if reference.len() == 40 && reference.chars().all(|c| c.is_ascii_hexdigit()) {
            Self::Commit(reference.to_string())
        } else if reference.starts_with('v')
            && reference[1..]
                .chars()
                .next()
                .is_some_and(|c| c.is_numeric())
        {
            Self::Tag(reference.to_string())
        } else {
            Self::Branch(reference.to_string())
        }
    }

    /// Get reference string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Branch(s) | Self::Tag(s) | Self::Commit(s) => s,
        }
    }
}

/// Git repository operations.
#[derive(Debug)]
pub struct GitRepository {
    path: PathBuf,
}

impl GitRepository {
    /// Open existing repository.
    ///
    /// # Errors
    /// Returns error if repository cannot be opened.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if !path.join(".git").exists() && !path.join("HEAD").exists() {
            return Err(Error::Vcs(format!(
                "not a git repository: {}",
                path.display()
            )));
        }

        Ok(Self { path })
    }

    /// Clone repository using git CLI.
    ///
    /// # Errors
    /// Returns error if clone fails.
    pub fn clone(url: &Url, dest: &Path, git_ref: Option<&GitRef>) -> Result<Self> {
        debug!(url = %url, dest = ?dest, "cloning repository");

        std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;

        let mut cmd = Command::new("git");
        cmd.arg("clone")
            .arg("--depth")
            .arg("1")
            .arg(url.as_str())
            .arg(dest);

        if let Some(reference) = git_ref {
            match reference {
                GitRef::Branch(b) => {
                    cmd.arg("--branch").arg(b);
                }
                GitRef::Tag(t) => {
                    cmd.arg("--branch").arg(t);
                }
                GitRef::Commit(_) => {
                    // For commits, we need full clone
                    cmd.args(["--depth", "1"]);
                }
            }
        }

        let output = cmd.output().map_err(|e| Error::Vcs(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Vcs(format!("git clone failed: {stderr}")));
        }

        let git_repo = Self {
            path: dest.to_path_buf(),
        };

        if let Some(GitRef::Commit(sha)) = git_ref {
            git_repo.checkout_commit(sha)?;
        }

        info!(url = %url, "clone complete");
        Ok(git_repo)
    }

    /// Get repository path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Checkout reference.
    ///
    /// # Errors
    /// Returns error if checkout fails.
    pub fn checkout(&self, git_ref: &GitRef) -> Result<()> {
        debug!(reference = git_ref.as_str(), "checking out");

        let output = Command::new("git")
            .current_dir(&self.path)
            .arg("checkout")
            .arg(git_ref.as_str())
            .output()
            .map_err(|e| Error::Vcs(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Vcs(format!("git checkout failed: {stderr}")));
        }

        info!(reference = git_ref.as_str(), "checkout complete");
        Ok(())
    }

    fn checkout_commit(&self, sha: &str) -> Result<()> {
        let output = Command::new("git")
            .current_dir(&self.path)
            .arg("fetch")
            .arg("--depth")
            .arg("1")
            .arg("origin")
            .arg(sha)
            .output()
            .map_err(|e| Error::Vcs(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Vcs(format!("git fetch failed: {stderr}")));
        }

        let output = Command::new("git")
            .current_dir(&self.path)
            .arg("checkout")
            .arg(sha)
            .output()
            .map_err(|e| Error::Vcs(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Vcs(format!("git checkout failed: {stderr}")));
        }

        Ok(())
    }

    /// Get current HEAD commit.
    ///
    /// # Errors
    /// Returns error if HEAD cannot be resolved.
    pub fn head_commit(&self) -> Result<String> {
        let output = Command::new("git")
            .current_dir(&self.path)
            .arg("rev-parse")
            .arg("HEAD")
            .output()
            .map_err(|e| Error::Vcs(e.to_string()))?;

        if !output.status.success() {
            return Err(Error::Vcs("failed to get HEAD".into()));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Fetch latest changes.
    ///
    /// # Errors
    /// Returns error if fetch fails.
    pub fn fetch(&self, remote: &str) -> Result<()> {
        debug!(remote, "fetching");

        let output = Command::new("git")
            .current_dir(&self.path)
            .arg("fetch")
            .arg(remote)
            .output()
            .map_err(|e| Error::Vcs(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Error::Vcs(format!("git fetch failed: {stderr}")));
        }

        info!(remote, "fetch complete");
        Ok(())
    }

    /// Check if path is a git repository.
    #[must_use]
    pub fn is_repository(path: &Path) -> bool {
        path.join(".git").exists() || (path.join("HEAD").exists() && path.join("objects").exists())
    }
}

/// Parse git URL to extract info.
#[derive(Debug, Clone)]
pub struct GitUrl {
    /// Full URL.
    pub url: Url,
    /// Repository owner/organization.
    pub owner: Option<String>,
    /// Repository name.
    pub repo: Option<String>,
}

impl GitUrl {
    /// Parse git URL.
    #[must_use]
    pub fn parse(url: &Url) -> Self {
        let (owner, repo) = Self::extract_owner_repo(url);
        Self {
            url: url.clone(),
            owner,
            repo,
        }
    }

    fn extract_owner_repo(url: &Url) -> (Option<String>, Option<String>) {
        let path = url.path().trim_start_matches('/').trim_end_matches(".git");
        let parts: Vec<&str> = path.split('/').collect();

        match parts.as_slice() {
            [owner, repo, ..] => (Some((*owner).to_string()), Some((*repo).to_string())),
            [repo] if !repo.is_empty() => (None, Some((*repo).to_string())),
            _ => (None, None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn git_ref_parse() {
        assert!(matches!(GitRef::parse("main"), GitRef::Branch(_)));
        assert!(matches!(GitRef::parse("v1.0.0"), GitRef::Tag(_)));
        assert!(matches!(
            GitRef::parse("abc123def456abc123def456abc123def456abcd"),
            GitRef::Commit(_)
        ));
    }

    #[test]
    fn git_url_parse() {
        let url = Url::parse("https://github.com/owner/repo.git").unwrap();
        let git_url = GitUrl::parse(&url);
        assert_eq!(git_url.owner, Some("owner".to_string()));
        assert_eq!(git_url.repo, Some("repo".to_string()));
    }

    #[test]
    fn is_not_repository() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!GitRepository::is_repository(temp.path()));
    }
}
