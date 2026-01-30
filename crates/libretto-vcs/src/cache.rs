//! Reference repository caching for Git object sharing.
//!
//! Implements efficient cloning using:
//! - Bare repository caching
//! - Git alternates for object sharing
//! - Reference clones (--reference)

use crate::error::{Result, VcsError};
use crate::url::VcsUrl;
use blake3::Hasher;
use dashmap::DashMap;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use tracing::{debug, info, trace, warn};

/// Reference repository cache for efficient cloning.
#[derive(Debug)]
pub struct ReferenceCache {
    /// Cache directory.
    cache_dir: PathBuf,
    /// Cached repository paths by URL hash.
    repositories: DashMap<String, PathBuf>,
    /// Lock for cache operations.
    operation_locks: DashMap<String, Arc<RwLock<()>>>,
    /// Maximum cache size in bytes (0 = unlimited).
    max_size: u64,
    /// Current cache size.
    current_size: std::sync::atomic::AtomicU64,
}

impl ReferenceCache {
    /// Create a new reference cache.
    pub fn new(cache_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&cache_dir).map_err(|e| VcsError::io(&cache_dir, e))?;

        let cache = Self {
            cache_dir,
            repositories: DashMap::new(),
            operation_locks: DashMap::new(),
            max_size: 0,
            current_size: std::sync::atomic::AtomicU64::new(0),
        };

        // Scan existing cache
        cache.scan_cache()?;

        Ok(cache)
    }

    /// Create with size limit.
    pub fn with_max_size(cache_dir: PathBuf, max_size: u64) -> Result<Self> {
        let mut cache = Self::new(cache_dir)?;
        cache.max_size = max_size;
        Ok(cache)
    }

    /// Scan cache directory for existing repositories.
    fn scan_cache(&self) -> Result<()> {
        let entries =
            std::fs::read_dir(&self.cache_dir).map_err(|e| VcsError::io(&self.cache_dir, e))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && is_bare_repo(&path) {
                // Extract URL hash from directory name
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    self.repositories.insert(name.to_string(), path.clone());

                    // Calculate size
                    if let Ok(size) = dir_size(&path) {
                        self.current_size
                            .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                    }
                }
            }
        }

        debug!(count = self.repositories.len(), "scanned reference cache");
        Ok(())
    }

    /// Get or create a reference repository for a URL.
    ///
    /// Returns the path to the bare repository.
    ///
    /// # Errors
    /// Returns error if reference cannot be created.
    pub fn get_or_create(&self, url: &VcsUrl) -> Result<PathBuf> {
        let url_hash = hash_url(&url.normalized);

        // Check if already cached
        if let Some(path) = self.repositories.get(&url_hash) {
            trace!(url = %url, "reference cache hit");
            return Ok(path.clone());
        }

        // Get lock for this URL
        let lock = self
            .operation_locks
            .entry(url_hash.clone())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone();

        let _guard = lock.write();

        // Double-check after acquiring lock
        if let Some(path) = self.repositories.get(&url_hash) {
            return Ok(path.clone());
        }

        // Create bare repository
        let repo_path = self.cache_dir.join(&url_hash);
        self.clone_bare(url, &repo_path)?;

        self.repositories.insert(url_hash, repo_path.clone());

        // Update size
        if let Ok(size) = dir_size(&repo_path) {
            self.current_size
                .fetch_add(size, std::sync::atomic::Ordering::Relaxed);
        }

        // Check if we need to evict
        if self.max_size > 0 && self.current_size() > self.max_size {
            self.evict_lru()?;
        }

        info!(url = %url, path = ?repo_path, "created reference repository");
        Ok(repo_path)
    }

    /// Get reference for a URL if it exists.
    #[must_use]
    pub fn get_reference(&self, url: &VcsUrl) -> Option<PathBuf> {
        let url_hash = hash_url(&url.normalized);
        self.repositories.get(&url_hash).map(|r| r.clone())
    }

    /// Check if a reference exists for a URL.
    #[must_use]
    pub fn has_reference(&self, url: &VcsUrl) -> bool {
        let url_hash = hash_url(&url.normalized);
        self.repositories.contains_key(&url_hash)
    }

    /// Clone as bare repository.
    fn clone_bare(&self, url: &VcsUrl, dest: &Path) -> Result<()> {
        debug!(url = %url, dest = ?dest, "cloning bare repository");

        let output = Command::new("git")
            .args([
                "clone",
                "--bare",
                "--mirror",
                &url.normalized,
                dest.to_str().unwrap_or(""),
            ])
            .env("GIT_PROTOCOL", "version=2")
            .output()
            .map_err(|e| VcsError::Command {
                command: "git clone --bare".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(VcsError::clone_failed(
                url.to_string(),
                format!("bare clone failed: {stderr}"),
                true,
            ));
        }

        Ok(())
    }

    /// Update a reference repository.
    ///
    /// # Errors
    /// Returns error if update fails.
    pub fn update(&self, url: &VcsUrl) -> Result<()> {
        let url_hash = hash_url(&url.normalized);

        let Some(repo_path) = self.repositories.get(&url_hash) else {
            return Err(VcsError::RepositoryNotFound {
                url: url.to_string(),
            });
        };

        let lock = self
            .operation_locks
            .entry(url_hash.clone())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone();

        let _guard = lock.write();

        debug!(url = %url, path = ?repo_path.value(), "updating reference repository");

        let output = Command::new("git")
            .current_dir(repo_path.value())
            .args(["fetch", "--all", "--prune"])
            .env("GIT_PROTOCOL", "version=2")
            .output()
            .map_err(|e| VcsError::Command {
                command: "git fetch --all".to_string(),
                message: e.to_string(),
                exit_code: None,
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(url = %url, error = %stderr, "failed to update reference");
        }

        Ok(())
    }

    /// Update all reference repositories.
    pub fn update_all(&self) -> Vec<Result<()>> {
        self.repositories
            .iter()
            .filter_map(|entry| {
                // Reconstruct URL from path (not ideal, but works for updates)
                let path = entry.value();

                // Read origin URL from config
                let output = Command::new("git")
                    .current_dir(path)
                    .args(["config", "--get", "remote.origin.url"])
                    .output()
                    .ok()?;

                if output.status.success() {
                    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    VcsUrl::parse(&url).ok().map(|u| self.update(&u))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Remove a reference repository.
    pub fn remove(&self, url: &VcsUrl) -> Result<()> {
        let url_hash = hash_url(&url.normalized);

        if let Some((_, path)) = self.repositories.remove(&url_hash)
            && path.exists()
        {
            // Calculate size before removal
            if let Ok(size) = dir_size(&path) {
                self.current_size
                    .fetch_sub(size, std::sync::atomic::Ordering::Relaxed);
            }

            std::fs::remove_dir_all(&path).map_err(|e| VcsError::io(&path, e))?;
            info!(url = %url, "removed reference repository");
        }

        Ok(())
    }

    /// Clear the entire cache.
    pub fn clear(&self) -> Result<()> {
        for entry in &self.repositories {
            let path = entry.value();
            if path.exists() {
                let _ = std::fs::remove_dir_all(path);
            }
        }

        self.repositories.clear();
        self.current_size
            .store(0, std::sync::atomic::Ordering::Relaxed);

        info!("cleared reference cache");
        Ok(())
    }

    /// Get current cache size in bytes.
    #[must_use]
    pub fn current_size(&self) -> u64 {
        self.current_size.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get number of cached repositories.
    #[must_use]
    pub fn count(&self) -> usize {
        self.repositories.len()
    }

    /// Evict least recently used entries.
    fn evict_lru(&self) -> Result<()> {
        // Simple LRU: evict oldest repositories by access time
        let mut entries: Vec<_> = self
            .repositories
            .iter()
            .filter_map(|entry| {
                let path = entry.value();
                let mtime = std::fs::metadata(path).and_then(|m| m.modified()).ok()?;
                Some((entry.key().clone(), mtime))
            })
            .collect();

        entries.sort_by_key(|(_, mtime)| *mtime);

        // Evict until under limit
        for (hash, _) in entries {
            if self.current_size() <= self.max_size {
                break;
            }

            if let Some((_, path)) = self.repositories.remove(&hash) {
                if let Ok(size) = dir_size(&path) {
                    self.current_size
                        .fetch_sub(size, std::sync::atomic::Ordering::Relaxed);
                }
                let _ = std::fs::remove_dir_all(&path);
                debug!(path = ?path, "evicted reference repository");
            }
        }

        Ok(())
    }

    /// Get cache directory.
    #[must_use]
    pub fn cache_dir(&self) -> &Path {
        &self.cache_dir
    }
}

/// Hash a URL to create a cache key.
fn hash_url(url: &str) -> String {
    let mut hasher = Hasher::new();
    hasher.update(url.as_bytes());
    let hash = hasher.finalize();
    hex::encode(&hash.as_bytes()[..16])
}

/// Check if a path is a bare Git repository.
fn is_bare_repo(path: &Path) -> bool {
    path.join("HEAD").exists() && path.join("objects").exists() && !path.join(".git").exists()
}

/// Calculate directory size.
fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut size = 0;
    for entry in walkdir::WalkDir::new(path).into_iter().flatten() {
        if entry.file_type().is_file() {
            size += entry.metadata()?.len();
        }
    }
    Ok(size)
}

/// Hex encoding for hash.
mod hex {
    pub fn encode(data: &[u8]) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(data.len() * 2);
        for byte in data {
            s.push(HEX[(byte >> 4) as usize] as char);
            s.push(HEX[(byte & 0x0f) as usize] as char);
        }
        s
    }
}

/// Git alternates file manager.
#[derive(Debug)]
pub struct AlternatesManager {
    /// Repository path.
    repo_path: PathBuf,
}

impl AlternatesManager {
    /// Create a new alternates manager.
    #[must_use]
    pub const fn new(repo_path: PathBuf) -> Self {
        Self { repo_path }
    }

    /// Add an alternate objects directory.
    ///
    /// # Errors
    /// Returns error if alternates cannot be updated.
    pub fn add_alternate(&self, alternate_path: &Path) -> Result<()> {
        let objects_dir = self.repo_path.join(".git").join("objects");
        let alternates_file = objects_dir.join("info").join("alternates");

        // Create info directory if needed
        std::fs::create_dir_all(alternates_file.parent().unwrap())
            .map_err(|e| VcsError::io(&alternates_file, e))?;

        // Read existing alternates
        let mut alternates = if alternates_file.exists() {
            std::fs::read_to_string(&alternates_file)
                .map_err(|e| VcsError::io(&alternates_file, e))?
        } else {
            String::new()
        };

        let alternate_objects = alternate_path.join("objects");
        let alternate_str = alternate_objects.to_string_lossy();

        // Check if already present
        if !alternates.lines().any(|l| l == alternate_str) {
            if !alternates.is_empty() && !alternates.ends_with('\n') {
                alternates.push('\n');
            }
            alternates.push_str(&alternate_str);
            alternates.push('\n');

            std::fs::write(&alternates_file, &alternates)
                .map_err(|e| VcsError::io(&alternates_file, e))?;

            debug!(
                repo = ?self.repo_path,
                alternate = ?alternate_path,
                "added git alternate"
            );
        }

        Ok(())
    }

    /// Remove an alternate.
    ///
    /// # Errors
    /// Returns error if alternates cannot be updated.
    pub fn remove_alternate(&self, alternate_path: &Path) -> Result<()> {
        let alternates_file = self
            .repo_path
            .join(".git")
            .join("objects")
            .join("info")
            .join("alternates");

        if !alternates_file.exists() {
            return Ok(());
        }

        let content = std::fs::read_to_string(&alternates_file)
            .map_err(|e| VcsError::io(&alternates_file, e))?;

        let alternate_objects = alternate_path.join("objects");
        let alternate_str = alternate_objects.to_string_lossy();

        let new_content: String = content
            .lines()
            .filter(|l| *l != alternate_str)
            .collect::<Vec<_>>()
            .join("\n");

        if new_content.is_empty() {
            std::fs::remove_file(&alternates_file)
                .map_err(|e| VcsError::io(&alternates_file, e))?;
        } else {
            std::fs::write(&alternates_file, new_content + "\n")
                .map_err(|e| VcsError::io(&alternates_file, e))?;
        }

        Ok(())
    }

    /// List current alternates.
    ///
    /// # Errors
    /// Returns error if alternates cannot be read.
    pub fn list_alternates(&self) -> Result<Vec<PathBuf>> {
        let alternates_file = self
            .repo_path
            .join(".git")
            .join("objects")
            .join("info")
            .join("alternates");

        if !alternates_file.exists() {
            return Ok(Vec::new());
        }

        let content = std::fs::read_to_string(&alternates_file)
            .map_err(|e| VcsError::io(&alternates_file, e))?;

        Ok(content
            .lines()
            .filter(|l| !l.is_empty())
            .map(PathBuf::from)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_url_consistent() {
        let hash1 = hash_url("https://github.com/owner/repo.git");
        let hash2 = hash_url("https://github.com/owner/repo.git");
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_url_different() {
        let hash1 = hash_url("https://github.com/owner/repo1.git");
        let hash2 = hash_url("https://github.com/owner/repo2.git");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn reference_cache_new() {
        let temp = tempfile::tempdir().unwrap();
        let cache = ReferenceCache::new(temp.path().to_path_buf()).unwrap();
        assert_eq!(cache.count(), 0);
    }

    #[test]
    fn alternates_manager() {
        let temp = tempfile::tempdir().unwrap();
        let repo_path = temp.path().join("repo");
        std::fs::create_dir_all(repo_path.join(".git/objects/info")).unwrap();

        let manager = AlternatesManager::new(repo_path);
        assert!(manager.list_alternates().unwrap().is_empty());
    }
}
