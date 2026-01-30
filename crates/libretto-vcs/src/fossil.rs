//! Fossil VCS operations via command-line.

use crate::error::{Result, VcsError};
use crate::types::RepoStatus;
use std::path::{Path, PathBuf};
use std::process::Command;
use tracing::{debug, info};

/// Fossil repository wrapper.
#[derive(Debug)]
pub struct FossilRepository {
    /// Repository path (working directory).
    path: PathBuf,
}

impl FossilRepository {
    /// Check if Fossil is available.
    #[must_use]
    pub fn is_available() -> bool {
        Command::new("fossil")
            .arg("version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Open an existing Fossil checkout.
    ///
    /// # Errors
    /// Returns error if path is not a Fossil checkout.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();

        if !path.join(".fslckout").exists() && !path.join("_FOSSIL_").exists() {
            return Err(VcsError::NotRepository { path });
        }

        Ok(Self { path })
    }

    /// Clone a Fossil repository.
    ///
    /// # Errors
    /// Returns error if clone fails.
    pub fn clone(url: &str, dest: &Path) -> Result<Self> {
        debug!(url, dest = ?dest, "fossil clone");

        // Ensure destination directory exists
        std::fs::create_dir_all(dest).map_err(|e| VcsError::io(dest, e))?;

        // Fossil clone creates a .fossil file (the repository)
        let repo_file = dest.join("repo.fossil");

        let output = Command::new("fossil")
            .args(["clone", url, repo_file.to_str().unwrap_or("repo.fossil")])
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil clone".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(Self::parse_fossil_error(&stderr, url));
        }

        // Open the repository in the destination directory
        let output = Command::new("fossil")
            .current_dir(dest)
            .args(["open", repo_file.to_str().unwrap_or("repo.fossil")])
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil open".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VcsError::Fossil {
                message: format!("failed to open repository: {stderr}"),
            });
        }

        info!(url, "fossil clone complete");
        Ok(Self {
            path: dest.to_path_buf(),
        })
    }

    /// Parse Fossil error output.
    fn parse_fossil_error(stderr: &str, url: &str) -> VcsError {
        let stderr_lower = stderr.to_lowercase();

        if stderr_lower.contains("not found") || stderr_lower.contains("does not exist") {
            return VcsError::RepositoryNotFound {
                url: url.to_string(),
            };
        }

        if stderr_lower.contains("authorization")
            || stderr_lower.contains("authentication")
            || stderr_lower.contains("login")
        {
            return VcsError::AuthenticationFailed {
                url: url.to_string(),
                reason: stderr.to_string(),
            };
        }

        VcsError::Fossil {
            message: stderr.to_string(),
        }
    }

    /// Get repository path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Update to latest.
    ///
    /// # Errors
    /// Returns error if update fails.
    pub fn update(&self) -> Result<()> {
        debug!(path = ?self.path, "fossil update");

        let output = Command::new("fossil")
            .current_dir(&self.path)
            .arg("update")
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil update".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VcsError::Fossil {
                message: format!("update failed: {stderr}"),
            });
        }

        Ok(())
    }

    /// Pull from remote.
    ///
    /// # Errors
    /// Returns error if pull fails.
    pub fn pull(&self) -> Result<()> {
        debug!(path = ?self.path, "fossil pull");

        let output = Command::new("fossil")
            .current_dir(&self.path)
            .arg("pull")
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil pull".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VcsError::Fossil {
                message: format!("pull failed: {stderr}"),
            });
        }

        Ok(())
    }

    /// Checkout a specific version.
    ///
    /// # Errors
    /// Returns error if checkout fails.
    pub fn checkout(&self, version: &str) -> Result<()> {
        debug!(path = ?self.path, version, "fossil checkout");

        let output = Command::new("fossil")
            .current_dir(&self.path)
            .args(["checkout", version])
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil checkout".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VcsError::CheckoutFailed {
                reference: version.to_string(),
                reason: stderr.to_string(),
            });
        }

        Ok(())
    }

    /// Get current checkout information.
    ///
    /// # Errors
    /// Returns error if info cannot be determined.
    pub fn info(&self) -> Result<String> {
        let output = Command::new("fossil")
            .current_dir(&self.path)
            .arg("info")
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil info".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            return Err(VcsError::Fossil {
                message: "failed to get info".to_string(),
            });
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    /// Get repository status.
    ///
    /// # Errors
    /// Returns error if status cannot be determined.
    pub fn status(&self) -> Result<RepoStatus> {
        let output = Command::new("fossil")
            .current_dir(&self.path)
            .arg("changes")
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil changes".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        let mut status = RepoStatus::default();

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            status.modified = stdout.lines().count();
            status.is_dirty = status.modified > 0;
        }

        // Get current checkout hash
        let output = Command::new("fossil")
            .current_dir(&self.path)
            .args(["info", "--short"])
            .output();

        if let Ok(output) = output
            && output.status.success()
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Extract the checkout hash
            for line in stdout.lines() {
                if line.starts_with("checkout:")
                    && let Some(hash) = line.split_whitespace().nth(1)
                {
                    status.head = hash.to_string();
                    break;
                }
            }
        }

        Ok(status)
    }

    /// Check if checkout has local modifications.
    ///
    /// # Errors
    /// Returns error if check fails.
    pub fn is_dirty(&self) -> Result<bool> {
        let output = Command::new("fossil")
            .current_dir(&self.path)
            .arg("changes")
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil changes".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        Ok(!output.stdout.is_empty())
    }

    /// Check if a path is a Fossil checkout.
    #[must_use]
    pub fn is_repository(path: &Path) -> bool {
        path.join(".fslckout").exists() || path.join("_FOSSIL_").exists()
    }

    /// Close the repository (opposite of open).
    ///
    /// # Errors
    /// Returns error if close fails.
    pub fn close(&self) -> Result<()> {
        let output = Command::new("fossil")
            .current_dir(&self.path)
            .arg("close")
            .output()
            .map_err(|e| VcsError::Command {
                command: "fossil close".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VcsError::Fossil {
                message: format!("close failed: {stderr}"),
            });
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_fossil_repository() {
        let temp = tempfile::tempdir().unwrap();
        assert!(!FossilRepository::is_repository(temp.path()));
    }

    #[test]
    fn parse_error_not_found() {
        let err = FossilRepository::parse_fossil_error(
            "Error: repository not found",
            "https://example.com/repo",
        );
        assert!(matches!(err, VcsError::RepositoryNotFound { .. }));
    }
}
