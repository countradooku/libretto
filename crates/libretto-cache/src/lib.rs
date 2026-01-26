//! Package caching for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use chrono::{DateTime, Utc};
use dashmap::DashMap;
use libretto_core::{ContentHash, Error, PackageId, Result};
use libretto_platform::Platform;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info};

/// Cache entry metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    /// Package identifier.
    pub package_id: PackageId,
    /// Version string.
    pub version: String,
    /// Content hash.
    pub hash: ContentHash,
    /// When cached.
    pub cached_at: DateTime<Utc>,
    /// Last accessed.
    pub last_accessed: DateTime<Utc>,
    /// Size in bytes.
    pub size: u64,
}

/// Cache statistics.
#[derive(Debug, Default, Clone)]
pub struct CacheStats {
    /// Number of entries.
    pub entries: usize,
    /// Total size in bytes.
    pub total_size: u64,
    /// Cache hits.
    pub hits: u64,
    /// Cache misses.
    pub misses: u64,
}

/// Package cache manager.
#[derive(Debug)]
pub struct Cache {
    root: PathBuf,
    entries: DashMap<String, CacheEntry>,
    stats: Arc<RwLock<CacheStats>>,
}

impl Cache {
    /// Create cache at default location.
    ///
    /// # Errors
    /// Returns error if cache directory cannot be created.
    pub fn new() -> Result<Self> {
        Self::at_path(Platform::current().cache_dir.join("packages"))
    }

    /// Create cache at specific path.
    ///
    /// # Errors
    /// Returns error if cache directory cannot be created.
    pub fn at_path(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root).map_err(|e| Error::io(&root, e))?;

        let cache = Self {
            root,
            entries: DashMap::new(),
            stats: Arc::new(RwLock::new(CacheStats::default())),
        };

        cache.load_index()?;
        Ok(cache)
    }

    /// Get cache root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Check if package is cached.
    #[must_use]
    pub fn contains(&self, package_id: &PackageId, version: &str) -> bool {
        let key = Self::make_key(package_id, version);
        self.entries.contains_key(&key)
    }

    /// Get cached package path.
    #[must_use]
    pub fn get_path(&self, package_id: &PackageId, version: &str) -> Option<PathBuf> {
        let key = Self::make_key(package_id, version);
        if let Some(mut entry) = self.entries.get_mut(&key) {
            entry.last_accessed = Utc::now();
            self.stats.write().hits += 1;
            let path = self.package_path(package_id, version);
            if path.exists() {
                return Some(path);
            }
        }
        self.stats.write().misses += 1;
        None
    }

    /// Store package in cache.
    ///
    /// # Errors
    /// Returns error if package cannot be stored.
    pub fn store(
        &self,
        package_id: &PackageId,
        version: &str,
        source: &Path,
        hash: ContentHash,
    ) -> Result<PathBuf> {
        let dest = self.package_path(package_id, version);

        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }

        let size = copy_recursive(source, &dest)?;

        let entry = CacheEntry {
            package_id: package_id.clone(),
            version: version.to_string(),
            hash,
            cached_at: Utc::now(),
            last_accessed: Utc::now(),
            size,
        };

        let key = Self::make_key(package_id, version);
        self.entries.insert(key, entry);

        {
            let mut stats = self.stats.write();
            stats.entries = self.entries.len();
            stats.total_size += size;
        }

        self.save_index()?;
        info!(
            package = %package_id,
            version = %version,
            "cached package"
        );

        Ok(dest)
    }

    /// Remove package from cache.
    ///
    /// # Errors
    /// Returns error if package cannot be removed.
    pub fn remove(&self, package_id: &PackageId, version: &str) -> Result<bool> {
        let key = Self::make_key(package_id, version);
        if let Some((_, entry)) = self.entries.remove(&key) {
            let path = self.package_path(package_id, version);
            if path.exists() {
                std::fs::remove_dir_all(&path).map_err(|e| Error::io(&path, e))?;
            }

            {
                let mut stats = self.stats.write();
                stats.entries = self.entries.len();
                stats.total_size = stats.total_size.saturating_sub(entry.size);
            }

            self.save_index()?;
            debug!(package = %package_id, version = %version, "removed from cache");
            return Ok(true);
        }
        Ok(false)
    }

    /// Clear entire cache.
    ///
    /// # Errors
    /// Returns error if cache cannot be cleared.
    pub fn clear(&self) -> Result<()> {
        self.entries.clear();
        *self.stats.write() = CacheStats::default();

        if self.root.exists() {
            std::fs::remove_dir_all(&self.root).map_err(|e| Error::io(&self.root, e))?;
            std::fs::create_dir_all(&self.root).map_err(|e| Error::io(&self.root, e))?;
        }

        info!("cache cleared");
        Ok(())
    }

    /// Get cache statistics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        self.stats.read().clone()
    }

    /// Prune old entries.
    ///
    /// # Errors
    /// Returns error if pruning fails.
    pub fn prune(&self, max_age_days: i64) -> Result<usize> {
        let cutoff = Utc::now() - chrono::Duration::days(max_age_days);
        let mut removed = 0;

        let to_remove: Vec<_> = self
            .entries
            .iter()
            .filter(|e| e.last_accessed < cutoff)
            .map(|e| (e.package_id.clone(), e.version.clone()))
            .collect();

        for (package_id, version) in to_remove {
            if self.remove(&package_id, &version)? {
                removed += 1;
            }
        }

        if removed > 0 {
            info!(count = removed, "pruned cache entries");
        }

        Ok(removed)
    }

    fn package_path(&self, package_id: &PackageId, version: &str) -> PathBuf {
        self.root
            .join(package_id.vendor())
            .join(package_id.name())
            .join(version)
    }

    fn make_key(package_id: &PackageId, version: &str) -> String {
        format!("{}:{}", package_id, version)
    }

    fn index_path(&self) -> PathBuf {
        self.root.join("cache-index.json")
    }

    fn load_index(&self) -> Result<()> {
        let path = self.index_path();
        if !path.exists() {
            return Ok(());
        }

        let data = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        let entries: Vec<CacheEntry> =
            sonic_rs::from_slice(&data).map_err(libretto_core::Error::from)?;

        let mut total_size = 0u64;
        for entry in entries {
            total_size += entry.size;
            let key = Self::make_key(&entry.package_id, &entry.version);
            self.entries.insert(key, entry);
        }

        let mut stats = self.stats.write();
        stats.entries = self.entries.len();
        stats.total_size = total_size;

        debug!(entries = stats.entries, "loaded cache index");
        Ok(())
    }

    fn save_index(&self) -> Result<()> {
        let entries: Vec<CacheEntry> = self.entries.iter().map(|e| e.value().clone()).collect();
        let data = sonic_rs::to_string_pretty(&entries).map_err(libretto_core::Error::from)?;
        let path = self.index_path();
        std::fs::write(&path, data).map_err(|e| Error::io(&path, e))?;
        Ok(())
    }
}

fn copy_recursive(src: &Path, dest: &Path) -> Result<u64> {
    let mut total_size = 0u64;

    if src.is_file() {
        std::fs::copy(src, dest).map_err(|e| Error::io(src, e))?;
        return Ok(std::fs::metadata(src).map_err(|e| Error::io(src, e))?.len());
    }

    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;

    for entry in walkdir::WalkDir::new(src).min_depth(1) {
        let entry = entry.map_err(|e| Error::Cache(e.to_string()))?;
        let relative = entry
            .path()
            .strip_prefix(src)
            .map_err(|e| Error::Cache(e.to_string()))?;
        let dest_path = dest.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest_path).map_err(|e| Error::io(&dest_path, e))?;
        } else {
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            std::fs::copy(entry.path(), &dest_path).map_err(|e| Error::io(entry.path(), e))?;
            total_size += entry
                .metadata()
                .map_err(|e| Error::Cache(e.to_string()))?
                .len();
        }
    }

    Ok(total_size)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_operations() {
        let dir = tempfile::tempdir().unwrap();
        let cache = Cache::at_path(dir.path().join("cache")).unwrap();

        let pkg_id = PackageId::new("test", "package");
        assert!(!cache.contains(&pkg_id, "1.0.0"));

        let stats = cache.stats();
        assert_eq!(stats.entries, 0);
    }
}
