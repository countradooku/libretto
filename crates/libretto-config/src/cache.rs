//! Configuration caching with file watching.

use crate::error::Result;
use crate::loader::ConfigLoader;
use crate::types::{ComposerManifest, ResolvedConfig};
use dashmap::DashMap;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};
use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Global configuration cache singleton.
static GLOBAL_CACHE: OnceCell<ConfigCache> = OnceCell::new();

/// Get or initialize the global configuration cache.
#[must_use]
pub fn global_cache() -> &'static ConfigCache {
    GLOBAL_CACHE.get_or_init(ConfigCache::new)
}

/// Configuration cache with lazy loading and invalidation.
#[derive(Debug)]
pub struct ConfigCache {
    /// Cached resolved configurations by project path.
    configs: DashMap<PathBuf, CachedConfig, ahash::RandomState>,
    /// Cached manifests by file path.
    manifests: DashMap<PathBuf, CachedManifest, ahash::RandomState>,
    /// File modification times for invalidation.
    mtimes: DashMap<PathBuf, u64, ahash::RandomState>,
    /// Generation counter for cache invalidation.
    generation: AtomicU64,
    /// Whether watching is enabled.
    watching: AtomicBool,
}

/// Cached configuration entry.
#[derive(Debug, Clone)]
struct CachedConfig {
    /// Resolved configuration.
    config: Arc<ResolvedConfig>,
    /// Cache generation when entry was created.
    generation: u64,
    /// Creation timestamp.
    created: Instant,
}

/// Cached manifest entry.
#[derive(Debug, Clone)]
struct CachedManifest {
    /// Parsed manifest.
    manifest: Arc<ComposerManifest>,
    /// File hash for invalidation.
    hash: u64,
}

impl ConfigCache {
    /// Create a new configuration cache.
    #[must_use]
    pub fn new() -> Self {
        Self {
            configs: DashMap::with_hasher(ahash::RandomState::new()),
            manifests: DashMap::with_hasher(ahash::RandomState::new()),
            mtimes: DashMap::with_hasher(ahash::RandomState::new()),
            generation: AtomicU64::new(0),
            watching: AtomicBool::new(false),
        }
    }

    /// Get cached resolved configuration for a project.
    #[must_use]
    pub fn get_config(&self, project_dir: &Path) -> Option<Arc<ResolvedConfig>> {
        let entry = self.configs.get(project_dir)?;
        let current_gen = self.generation.load(Ordering::Acquire);

        // Check if entry is stale
        if entry.generation < current_gen {
            return None;
        }

        // Check age limit (5 minutes max cache)
        if entry.created.elapsed() > Duration::from_secs(300) {
            return None;
        }

        Some(Arc::clone(&entry.config))
    }

    /// Cache a resolved configuration.
    pub fn put_config(&self, project_dir: PathBuf, config: ResolvedConfig) {
        let entry = CachedConfig {
            config: Arc::new(config),
            generation: self.generation.load(Ordering::Acquire),
            created: Instant::now(),
        };
        self.configs.insert(project_dir, entry);
    }

    /// Get cached manifest.
    #[must_use]
    pub fn get_manifest(&self, path: &Path) -> Option<Arc<ComposerManifest>> {
        // Check mtime first
        if let Some(mtime) = self.get_mtime(path)
            && let Some(entry) = self.manifests.get(path)
        {
            // Validate by hash
            if entry.hash == mtime {
                return Some(Arc::clone(&entry.manifest));
            }
        }
        None
    }

    /// Cache a manifest.
    pub fn put_manifest(&self, path: PathBuf, manifest: ComposerManifest) {
        let hash = self.get_mtime(&path).unwrap_or(0);
        let entry = CachedManifest {
            manifest: Arc::new(manifest),
            hash,
        };
        self.manifests.insert(path, entry);
    }

    /// Invalidate all cached configurations.
    pub fn invalidate_all(&self) {
        self.generation.fetch_add(1, Ordering::Release);
        self.configs.clear();
        self.manifests.clear();
        self.mtimes.clear();
    }

    /// Invalidate configuration for a specific project.
    pub fn invalidate_project(&self, project_dir: &Path) {
        self.configs.remove(project_dir);

        // Also invalidate related manifests
        let manifest_path = project_dir.join("composer.json");
        self.manifests.remove(&manifest_path);
        self.mtimes.remove(&manifest_path);
    }

    /// Invalidate a specific file.
    pub fn invalidate_file(&self, path: &Path) {
        self.manifests.remove(path);
        self.mtimes.remove(path);

        // Invalidate project config if this is a composer.json
        if path.file_name().is_some_and(|n| n == "composer.json")
            && let Some(parent) = path.parent()
        {
            self.configs.remove(parent);
        }

        // Invalidate global configs if this is in global config dir
        if path.to_string_lossy().contains("libretto") {
            self.generation.fetch_add(1, Ordering::Release);
        }
    }

    /// Get file modification time.
    fn get_mtime(&self, path: &Path) -> Option<u64> {
        // Check cache first
        if let Some(mtime) = self.mtimes.get(path) {
            return Some(*mtime);
        }

        // Get from filesystem
        let metadata = std::fs::metadata(path).ok()?;
        let mtime = metadata
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_secs();

        self.mtimes.insert(path.to_path_buf(), mtime);
        Some(mtime)
    }

    /// Get cache statistics.
    #[must_use]
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            config_entries: self.configs.len(),
            manifest_entries: self.manifests.len(),
            generation: self.generation.load(Ordering::Relaxed),
            watching: self.watching.load(Ordering::Relaxed),
        }
    }
}

impl Default for ConfigCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Cache statistics.
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of cached configurations.
    pub config_entries: usize,
    /// Number of cached manifests.
    pub manifest_entries: usize,
    /// Current generation counter.
    pub generation: u64,
    /// Whether watching is enabled.
    pub watching: bool,
}

/// Configuration file watcher for hot reloading.
#[derive(Debug)]
pub struct ConfigWatcher {
    /// Internal watcher.
    watcher: RwLock<Option<RecommendedWatcher>>,
    /// Cache to invalidate.
    cache: &'static ConfigCache,
    /// Watched paths.
    watched: DashMap<PathBuf, (), ahash::RandomState>,
}

impl ConfigWatcher {
    /// Create a new configuration watcher.
    #[must_use]
    pub fn new(cache: &'static ConfigCache) -> Self {
        Self {
            watcher: RwLock::new(None),
            cache,
            watched: DashMap::with_hasher(ahash::RandomState::new()),
        }
    }

    /// Start watching configuration files.
    ///
    /// # Errors
    /// Returns error if watcher cannot be created.
    pub fn start(&self) -> Result<()> {
        let cache = self.cache;

        let watcher =
            notify::recommended_watcher(move |res: std::result::Result<Event, notify::Error>| {
                if let Ok(event) = res {
                    for path in event.paths {
                        cache.invalidate_file(&path);
                    }
                }
            })?;

        *self.watcher.write() = Some(watcher);
        self.cache.watching.store(true, Ordering::Release);
        Ok(())
    }

    /// Stop watching.
    pub fn stop(&self) {
        *self.watcher.write() = None;
        self.cache.watching.store(false, Ordering::Release);
        self.watched.clear();
    }

    /// Watch a specific file or directory.
    ///
    /// # Errors
    /// Returns error if path cannot be watched.
    pub fn watch(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();

        // Skip if already watching
        if self.watched.contains_key(path) {
            return Ok(());
        }

        if let Some(ref mut watcher) = *self.watcher.write() {
            watcher.watch(path, RecursiveMode::NonRecursive)?;
            self.watched.insert(path.to_path_buf(), ());
        }

        Ok(())
    }

    /// Unwatch a path.
    pub fn unwatch(&self, path: impl AsRef<Path>) {
        let path = path.as_ref();

        if let Some(ref mut watcher) = *self.watcher.write() {
            let _ = watcher.unwatch(path);
            self.watched.remove(path);
        }
    }

    /// Check if watching is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.watcher.read().is_some()
    }
}

/// Cached configuration manager combining cache and loader.
#[derive(Debug)]
pub struct CachedConfigManager {
    /// Configuration cache.
    cache: &'static ConfigCache,
    /// Configuration loader.
    loader: RwLock<ConfigLoader>,
}

impl CachedConfigManager {
    /// Create a new cached configuration manager.
    #[must_use]
    pub fn new(project_dir: impl Into<PathBuf>) -> Self {
        Self {
            cache: global_cache(),
            loader: RwLock::new(ConfigLoader::new(project_dir)),
        }
    }

    /// Get resolved configuration (cached if possible).
    ///
    /// # Errors
    /// Returns error if configuration cannot be resolved.
    pub fn get_config(&self) -> Result<Arc<ResolvedConfig>> {
        let project_dir = self.loader.read().project_dir().to_path_buf();

        // Check cache first
        if let Some(config) = self.cache.get_config(&project_dir) {
            return Ok(config);
        }

        // Load and cache
        let config = self.loader.read().resolve()?;
        self.cache.put_config(project_dir.clone(), config);

        // Return from cache to get Arc
        Ok(self.cache.get_config(&project_dir).expect("just inserted"))
    }

    /// Get manifest (cached if possible).
    ///
    /// # Errors
    /// Returns error if manifest cannot be loaded.
    pub fn get_manifest(&self) -> Result<Arc<ComposerManifest>> {
        let manifest_path = self.loader.read().project_manifest_path();

        // Check cache first
        if let Some(manifest) = self.cache.get_manifest(&manifest_path) {
            return Ok(manifest);
        }

        // Load and cache
        let manifest = self.loader.write().load_project_manifest()?.clone();
        self.cache.put_manifest(manifest_path.clone(), manifest);

        // Return from cache to get Arc
        Ok(self
            .cache
            .get_manifest(&manifest_path)
            .expect("just inserted"))
    }

    /// Force reload configuration.
    ///
    /// # Errors
    /// Returns error if configuration cannot be resolved.
    pub fn reload(&self) -> Result<Arc<ResolvedConfig>> {
        let project_dir = self.loader.read().project_dir().to_path_buf();
        self.cache.invalidate_project(&project_dir);
        self.get_config()
    }

    /// Get the underlying cache.
    #[must_use]
    pub const fn cache(&self) -> &'static ConfigCache {
        self.cache
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_put_get() {
        let cache = ConfigCache::new();
        let config = ResolvedConfig::default();
        let path = PathBuf::from("/test/project");

        cache.put_config(path.clone(), config);
        assert!(cache.get_config(&path).is_some());
    }

    #[test]
    fn cache_invalidate() {
        let cache = ConfigCache::new();
        let config = ResolvedConfig::default();
        let path = PathBuf::from("/test/project");

        cache.put_config(path.clone(), config);
        cache.invalidate_project(&path);
        assert!(cache.get_config(&path).is_none());
    }

    #[test]
    fn cache_generation() {
        let cache = ConfigCache::new();
        let config = ResolvedConfig::default();
        let path = PathBuf::from("/test/project");

        cache.put_config(path.clone(), config);
        cache.invalidate_all();
        assert!(cache.get_config(&path).is_none());
    }

    #[test]
    fn cache_stats() {
        let cache = ConfigCache::new();
        let stats = cache.stats();
        assert_eq!(stats.config_entries, 0);
        assert_eq!(stats.manifest_entries, 0);
    }
}
