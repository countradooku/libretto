//! Remote package source with on-demand fetching.
//!
//! This module provides a `PackageSource` implementation that fetches package
//! metadata from remote repositories (Packagist) on-demand during resolution.
//! This enables full recursive transitive dependency resolution - when PubGrub
//! requests a package that isn't cached, we fetch it from the repository.
//!
//! # Architecture
//!
//! The `RemotePackageSource` wraps a repository client and implements the
//! `PackageSource` trait. When `fetch()` is called:
//!
//! 1. Check in-memory cache (DashMap, lock-free)
//! 2. If miss, fetch from repository via async runtime bridge
//! 3. Convert repository `Package` types to resolver `PackageEntry` types
//! 4. Cache and return
//!
//! # Performance
//!
//! - **Parallel prefetching**: Speculatively fetches likely dependencies
//! - **Lock-free caching**: Uses DashMap for concurrent access
//! - **Batch fetching**: Groups requests when possible
//! - **Connection pooling**: Reuses HTTP connections via repository client

use crate::index::PackageSource;
use crate::package::{Dependency, PackageEntry, PackageName, PackageVersion};
use crate::version::{ComposerConstraint, ComposerVersion};
use dashmap::DashMap;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tracing::{debug, trace, warn};

/// Statistics for remote fetching.
#[derive(Debug, Default)]
pub struct RemoteStats {
    /// Cache hits.
    pub cache_hits: AtomicU64,
    /// Cache misses (remote fetches).
    pub cache_misses: AtomicU64,
    /// Failed fetches.
    pub fetch_failures: AtomicU64,
    /// Total packages fetched.
    pub packages_fetched: AtomicU64,
    /// Total versions fetched.
    pub versions_fetched: AtomicU64,
    /// Total fetch time in milliseconds.
    pub fetch_time_ms: AtomicU64,
}

impl RemoteStats {
    /// Get cache hit rate as percentage.
    #[must_use]
    pub fn hit_rate(&self) -> f64 {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        if total == 0 {
            0.0
        } else {
            (hits as f64 / total as f64) * 100.0
        }
    }

    /// Get summary string.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "Remote: {} hits, {} misses ({:.1}% hit rate), {} packages, {} versions, {}ms total",
            self.cache_hits.load(Ordering::Relaxed),
            self.cache_misses.load(Ordering::Relaxed),
            self.hit_rate(),
            self.packages_fetched.load(Ordering::Relaxed),
            self.versions_fetched.load(Ordering::Relaxed),
            self.fetch_time_ms.load(Ordering::Relaxed),
        )
    }
}

/// Trait for async package fetching from repositories.
///
/// This abstracts over the actual repository implementation, allowing
/// the resolver to work with any package source.
pub trait AsyncPackageFetcher: Send + Sync {
    /// Fetch package metadata by name.
    ///
    /// Returns all available versions of the package.
    fn fetch_package(
        &self,
        name: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<Vec<RemotePackage>>> + Send + '_>>;

    /// Fetch multiple packages in parallel.
    fn fetch_packages_batch(
        &self,
        names: &[String],
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Vec<(String, Option<Vec<RemotePackage>>)>> + Send + '_,
        >,
    >;
}

/// Remote package data from repository.
#[derive(Debug, Clone)]
pub struct RemotePackage {
    /// Package name (vendor/name).
    pub name: String,
    /// Version string.
    pub version: String,
    /// Dependencies (require).
    pub require: Vec<(String, String)>,
    /// Dev dependencies (require-dev).
    pub require_dev: Vec<(String, String)>,
    /// Packages this replaces.
    pub replace: Vec<(String, String)>,
    /// Virtual packages this provides.
    pub provide: Vec<(String, String)>,
    /// Packages this conflicts with.
    pub conflict: Vec<(String, String)>,
    /// Distribution URL.
    pub dist_url: Option<String>,
    /// Distribution type (zip, tar).
    pub dist_type: Option<String>,
    /// Distribution checksum.
    pub dist_shasum: Option<String>,
    /// Source URL (git).
    pub source_url: Option<String>,
    /// Source type.
    pub source_type: Option<String>,
    /// Source reference (commit/tag).
    pub source_reference: Option<String>,
}

/// Remote package source that fetches from repositories on-demand.
///
/// This implements `PackageSource` and bridges the sync resolver interface
/// with async repository fetching.
pub struct RemotePackageSource<F: AsyncPackageFetcher> {
    /// The async fetcher.
    fetcher: Arc<F>,
    /// Tokio runtime handle for blocking on async calls.
    runtime: Handle,
    /// In-memory cache of fetched packages.
    cache: DashMap<String, PackageEntry>,
    /// Packages that failed to fetch (avoid retrying).
    failed: DashMap<String, Instant>,
    /// How long to remember failed fetches.
    failure_ttl: Duration,
    /// Statistics.
    stats: RemoteStats,
    /// In-flight requests for deduplication.
    in_flight: DashMap<String, Arc<Mutex<()>>>,
}

impl<F: AsyncPackageFetcher> std::fmt::Debug for RemotePackageSource<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemotePackageSource")
            .field("cached_packages", &self.cache.len())
            .field("failed_packages", &self.failed.len())
            .finish()
    }
}

impl<F: AsyncPackageFetcher + 'static> RemotePackageSource<F> {
    /// Create a new remote package source.
    pub fn new(fetcher: Arc<F>, runtime: Handle) -> Self {
        Self {
            fetcher,
            runtime,
            cache: DashMap::with_capacity(1024),
            failed: DashMap::new(),
            failure_ttl: Duration::from_secs(300),
            stats: RemoteStats::default(),
            in_flight: DashMap::new(),
        }
    }

    /// Create with custom failure TTL.
    pub fn with_failure_ttl(mut self, ttl: Duration) -> Self {
        self.failure_ttl = ttl;
        self
    }

    /// Get statistics.
    #[must_use]
    pub fn stats(&self) -> &RemoteStats {
        &self.stats
    }

    /// Clear the cache.
    pub fn clear_cache(&self) {
        self.cache.clear();
        self.failed.clear();
    }

    /// Prefetch packages in the background.
    pub fn prefetch(&self, names: &[PackageName]) {
        let names: Vec<String> = names.iter().map(|n| n.as_str().to_string()).collect();
        let fetcher = Arc::clone(&self.fetcher);
        let cache = self.cache.clone();
        let runtime = self.runtime.clone();

        // Spawn prefetch task
        std::thread::spawn(move || {
            runtime.block_on(async {
                let results = fetcher.fetch_packages_batch(&names).await;
                for (name, packages) in results {
                    if let Some(packages) = packages {
                        if let Some(entry) = Self::convert_to_entry(&name, &packages) {
                            cache.insert(name, entry);
                        }
                    }
                }
            });
        });
    }

    /// Convert remote packages to a PackageEntry.
    fn convert_to_entry(name: &str, packages: &[RemotePackage]) -> Option<PackageEntry> {
        let pkg_name = PackageName::parse(name)?;
        let mut entry = PackageEntry::new(pkg_name.clone());

        for pkg in packages {
            let version = match ComposerVersion::parse(&pkg.version) {
                Some(v) => v,
                None => {
                    trace!(version = %pkg.version, "skipping unparseable version");
                    continue;
                }
            };

            let mut pkg_version = PackageVersion::new(pkg_name.clone(), version);

            // Convert dependencies
            for (dep_name, constraint) in &pkg.require {
                let constraint_str = if constraint == "self.version" {
                    pkg.version.as_str()
                } else {
                    constraint.as_str()
                };

                if let (Some(name), Some(constraint)) = (
                    PackageName::parse(dep_name),
                    ComposerConstraint::parse(constraint_str),
                ) {
                    pkg_version.add_dependency(Dependency::new(name, constraint));
                }
            }

            // Convert dev dependencies
            for (dep_name, constraint) in &pkg.require_dev {
                let constraint_str = if constraint == "self.version" {
                    pkg.version.as_str()
                } else {
                    constraint.as_str()
                };

                if let (Some(name), Some(constraint)) = (
                    PackageName::parse(dep_name),
                    ComposerConstraint::parse(constraint_str),
                ) {
                    pkg_version.add_dev_dependency(Dependency::new(name, constraint));
                }
            }

            // Convert replaces
            for (dep_name, constraint) in &pkg.replace {
                if let (Some(name), Some(constraint)) = (
                    PackageName::parse(dep_name),
                    ComposerConstraint::parse(constraint),
                ) {
                    pkg_version.add_replace(Dependency::new(name, constraint));
                }
            }

            // Convert provides
            for (dep_name, constraint) in &pkg.provide {
                if let (Some(name), Some(constraint)) = (
                    PackageName::parse(dep_name),
                    ComposerConstraint::parse(constraint),
                ) {
                    pkg_version.add_provide(Dependency::new(name, constraint));
                }
            }

            // Convert conflicts
            for (dep_name, constraint) in &pkg.conflict {
                if let (Some(name), Some(constraint)) = (
                    PackageName::parse(dep_name),
                    ComposerConstraint::parse(constraint),
                ) {
                    pkg_version.add_conflict(Dependency::new(name, constraint));
                }
            }

            // Set distribution info
            pkg_version.dist_url = pkg.dist_url.as_ref().map(|s| Arc::from(s.as_str()));
            pkg_version.dist_type = pkg.dist_type.as_ref().map(|s| Arc::from(s.as_str()));
            pkg_version.dist_shasum = pkg.dist_shasum.as_ref().map(|s| Arc::from(s.as_str()));
            pkg_version.source_url = pkg.source_url.as_ref().map(|s| Arc::from(s.as_str()));
            pkg_version.source_type = pkg.source_type.as_ref().map(|s| Arc::from(s.as_str()));
            pkg_version.source_reference =
                pkg.source_reference.as_ref().map(|s| Arc::from(s.as_str()));

            entry.add_version(pkg_version);
        }

        // Sort versions (highest first)
        entry.sort_versions();

        if entry.versions.is_empty() {
            None
        } else {
            Some(entry)
        }
    }

    /// Fetch a package, blocking on the async operation.
    fn fetch_blocking(&self, name: &PackageName) -> Option<PackageEntry> {
        let key = name.as_str().to_string();

        // Check cache first
        if let Some(entry) = self.cache.get(&key) {
            self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
            return Some(entry.clone());
        }

        // Check if recently failed
        if let Some(failed_at) = self.failed.get(&key) {
            if failed_at.elapsed() < self.failure_ttl {
                trace!(package = %key, "skipping recently failed package");
                return None;
            }
            // TTL expired, remove and retry
            drop(failed_at);
            self.failed.remove(&key);
        }

        self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);

        // Deduplicate in-flight requests
        let lock = self
            .in_flight
            .entry(key.clone())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();

        let _guard = lock.lock();

        // Double-check cache after acquiring lock
        if let Some(entry) = self.cache.get(&key) {
            return Some(entry.clone());
        }

        // Fetch from remote
        let start = Instant::now();
        debug!(package = %key, "fetching from remote");

        // Use block_in_place to allow blocking within an async context
        let result =
            tokio::task::block_in_place(|| self.runtime.block_on(self.fetcher.fetch_package(&key)));

        self.stats
            .fetch_time_ms
            .fetch_add(start.elapsed().as_millis() as u64, Ordering::Relaxed);

        match result {
            Some(packages) => {
                self.stats.packages_fetched.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .versions_fetched
                    .fetch_add(packages.len() as u64, Ordering::Relaxed);

                if let Some(entry) = Self::convert_to_entry(&key, &packages) {
                    self.cache.insert(key.clone(), entry.clone());
                    self.in_flight.remove(&key);
                    Some(entry)
                } else {
                    self.failed.insert(key.clone(), Instant::now());
                    self.in_flight.remove(&key);
                    None
                }
            }
            None => {
                self.stats.fetch_failures.fetch_add(1, Ordering::Relaxed);
                self.failed.insert(key.clone(), Instant::now());
                self.in_flight.remove(&key);
                warn!(package = %key, "failed to fetch package");
                None
            }
        }
    }
}

impl<F: AsyncPackageFetcher + 'static> PackageSource for RemotePackageSource<F> {
    fn fetch(&self, name: &PackageName) -> Option<PackageEntry> {
        self.fetch_blocking(name)
    }

    fn exists(&self, name: &PackageName) -> bool {
        let key = name.as_str();

        // Check cache
        if self.cache.contains_key(key) {
            return true;
        }

        // Check if recently failed
        if let Some(failed_at) = self.failed.get(key) {
            if failed_at.elapsed() < self.failure_ttl {
                return false;
            }
        }

        // Actually fetch to check existence
        self.fetch(name).is_some()
    }

    fn fetch_batch(&self, names: &[PackageName]) -> Vec<Option<PackageEntry>> {
        // Separate cached from uncached
        let mut results: Vec<Option<PackageEntry>> = vec![None; names.len()];
        let mut to_fetch: Vec<(usize, String)> = Vec::new();

        for (i, name) in names.iter().enumerate() {
            let key = name.as_str().to_string();
            if let Some(entry) = self.cache.get(&key) {
                self.stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                results[i] = Some(entry.clone());
            } else if self
                .failed
                .get(&key)
                .map_or(true, |t| t.elapsed() >= self.failure_ttl)
            {
                to_fetch.push((i, key));
            }
        }

        if to_fetch.is_empty() {
            return results;
        }

        // Batch fetch uncached packages
        let fetch_names: Vec<String> = to_fetch.iter().map(|(_, name)| name.clone()).collect();

        let start = Instant::now();
        let fetched = tokio::task::block_in_place(|| {
            self.runtime
                .block_on(self.fetcher.fetch_packages_batch(&fetch_names))
        });

        self.stats
            .fetch_time_ms
            .fetch_add(start.elapsed().as_millis() as u64, Ordering::Relaxed);

        // Process results
        for ((idx, name), (_, packages)) in to_fetch.into_iter().zip(fetched.into_iter()) {
            self.stats.cache_misses.fetch_add(1, Ordering::Relaxed);

            match packages {
                Some(pkgs) => {
                    self.stats.packages_fetched.fetch_add(1, Ordering::Relaxed);
                    self.stats
                        .versions_fetched
                        .fetch_add(pkgs.len() as u64, Ordering::Relaxed);

                    if let Some(entry) = Self::convert_to_entry(&name, &pkgs) {
                        self.cache.insert(name.clone(), entry.clone());
                        results[idx] = Some(entry);
                    } else {
                        self.failed.insert(name, Instant::now());
                    }
                }
                None => {
                    self.stats.fetch_failures.fetch_add(1, Ordering::Relaxed);
                    self.failed.insert(name, Instant::now());
                }
            }
        }

        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    /// Mock fetcher for testing.
    struct MockFetcher {
        packages: HashMap<String, Vec<RemotePackage>>,
    }

    impl MockFetcher {
        fn new() -> Self {
            Self {
                packages: HashMap::new(),
            }
        }

        fn add_package(&mut self, name: &str, version: &str, deps: Vec<(&str, &str)>) {
            let pkg = RemotePackage {
                name: name.to_string(),
                version: version.to_string(),
                require: deps
                    .into_iter()
                    .map(|(n, c)| (n.to_string(), c.to_string()))
                    .collect(),
                require_dev: vec![],
                replace: vec![],
                provide: vec![],
                conflict: vec![],
                dist_url: Some(format!("https://example.com/{name}/{version}.zip")),
                dist_type: Some("zip".to_string()),
                dist_shasum: None,
                source_url: None,
                source_type: None,
                source_reference: None,
            };

            self.packages
                .entry(name.to_string())
                .or_insert_with(Vec::new)
                .push(pkg);
        }
    }

    impl AsyncPackageFetcher for MockFetcher {
        fn fetch_package(
            &self,
            name: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Option<Vec<RemotePackage>>> + Send + '_>,
        > {
            let result = self.packages.get(name).cloned();
            Box::pin(async move { result })
        }

        fn fetch_packages_batch(
            &self,
            names: &[String],
        ) -> std::pin::Pin<
            Box<
                dyn std::future::Future<Output = Vec<(String, Option<Vec<RemotePackage>>)>>
                    + Send
                    + '_,
            >,
        > {
            let results: Vec<_> = names
                .iter()
                .map(|n| (n.clone(), self.packages.get(n).cloned()))
                .collect();
            Box::pin(async move { results })
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_remote_source_fetch() {
        let mut fetcher = MockFetcher::new();
        fetcher.add_package("test/a", "1.0.0", vec![("test/b", "^1.0")]);
        fetcher.add_package("test/a", "1.1.0", vec![("test/b", "^1.0")]);
        fetcher.add_package("test/b", "1.0.0", vec![]);

        let source = RemotePackageSource::new(Arc::new(fetcher), Handle::current());

        let name = PackageName::parse("test/a").unwrap();
        let entry = source.fetch(&name).unwrap();

        assert_eq!(entry.versions.len(), 2);
        assert_eq!(entry.versions[0].version.minor, 1); // Sorted highest first
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_cache_hit() {
        let mut fetcher = MockFetcher::new();
        fetcher.add_package("test/a", "1.0.0", vec![]);

        let source = RemotePackageSource::new(Arc::new(fetcher), Handle::current());
        let name = PackageName::parse("test/a").unwrap();

        // First fetch - cache miss
        let _ = source.fetch(&name);
        assert_eq!(source.stats.cache_misses.load(Ordering::Relaxed), 1);
        assert_eq!(source.stats.cache_hits.load(Ordering::Relaxed), 0);

        // Second fetch - cache hit
        let _ = source.fetch(&name);
        assert_eq!(source.stats.cache_misses.load(Ordering::Relaxed), 1);
        assert_eq!(source.stats.cache_hits.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_batch_fetch() {
        let mut fetcher = MockFetcher::new();
        fetcher.add_package("test/a", "1.0.0", vec![]);
        fetcher.add_package("test/b", "1.0.0", vec![]);
        fetcher.add_package("test/c", "1.0.0", vec![]);

        let source = RemotePackageSource::new(Arc::new(fetcher), Handle::current());

        let names: Vec<PackageName> = vec!["test/a", "test/b", "test/c"]
            .into_iter()
            .filter_map(PackageName::parse)
            .collect();

        let results = source.fetch_batch(&names);

        assert_eq!(results.len(), 3);
        assert!(results.iter().all(|r| r.is_some()));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_missing_package() {
        let fetcher = MockFetcher::new();
        let source = RemotePackageSource::new(Arc::new(fetcher), Handle::current());

        let name = PackageName::parse("test/missing").unwrap();
        let entry = source.fetch(&name);

        assert!(entry.is_none());
        assert_eq!(source.stats.fetch_failures.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn test_dependency_conversion() {
        let mut fetcher = MockFetcher::new();
        fetcher.add_package(
            "test/a",
            "1.0.0",
            vec![("test/b", "^1.0"), ("php", ">=8.0"), ("ext-json", "*")],
        );

        let source = RemotePackageSource::new(Arc::new(fetcher), Handle::current());
        let name = PackageName::parse("test/a").unwrap();
        let entry = source.fetch(&name).unwrap();

        // Should have only vendor/name dependencies (platform packages like php/ext-* are filtered by PackageName::parse)
        let version = &entry.versions[0];
        assert_eq!(version.dependencies.len(), 1); // Only test/b, php and ext-json don't parse
    }
}
