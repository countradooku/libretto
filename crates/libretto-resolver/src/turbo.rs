//! TURBO resolver - the fastest dependency resolver in the galaxy.
//!
//! Key optimizations:
//! 1. **Streaming fetch**: Process packages AS they arrive, don't wait for batches
//! 2. **Aggressive prefetch**: Start fetching dependencies before parent completes
//! 3. **No BFS levels**: Fire all requests immediately, process results as stream
//! 4. **Request deduplication**: Never fetch same package twice
//! 5. **Timeout per request**: 10s timeout, fail fast and move on
//! 6. **HTTP/2 multiplexing**: Reuse connections aggressively

use crate::package::{Dependency, PackageEntry, PackageName, PackageVersion};
use crate::provider::ResolutionMode;
use crate::resolver::{Resolution, ResolveError, ResolvedPackage};
use crate::version::{ComposerConstraint, ComposerVersion, Stability};
use ahash::{AHashMap, AHashSet};
use dashmap::DashSet;
use futures::stream::{FuturesUnordered, StreamExt};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use pubgrub::{
    DefaultStringReporter, Dependencies, DependencyConstraints, DependencyProvider,
    PackageResolutionStatistics, PubGrubError, Reporter, resolve,
};
use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tracing::{debug, info, trace, warn};
use version_ranges::Ranges;

/// Turbo resolver stats.
#[derive(Debug, Default)]
pub struct TurboStats {
    pub packages_fetched: AtomicU64,
    pub versions_total: AtomicU64,
    pub fetch_time_ms: AtomicU64,
    pub solver_time_ms: AtomicU64,
    pub requests_total: AtomicU64,
    pub requests_failed: AtomicU64,
}

/// Turbo resolver config.
#[derive(Debug, Clone)]
pub struct TurboConfig {
    /// Max concurrent HTTP requests (like Composer's 12, but we can go higher)
    pub max_concurrent: usize,
    /// Timeout per individual request
    pub request_timeout: Duration,
    /// Resolution mode
    pub mode: ResolutionMode,
    /// Minimum stability
    pub min_stability: Stability,
    /// Include dev dependencies
    pub include_dev: bool,
}

impl Default for TurboConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 32, // Higher than Composer - Rust can handle it
            request_timeout: Duration::from_secs(10),
            mode: ResolutionMode::PreferHighest,
            min_stability: Stability::Stable,
            include_dev: true,
        }
    }
}

/// Async package fetcher trait.
pub trait TurboFetcher: Send + Sync + 'static {
    fn fetch(
        &self,
        name: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<FetchedPackage>> + Send + '_>>;
}

// Implement for Arc<T> where T: TurboFetcher
impl<T: TurboFetcher> TurboFetcher for Arc<T> {
    fn fetch(
        &self,
        name: String,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Option<FetchedPackage>> + Send + '_>>
    {
        (**self).fetch(name)
    }
}

/// Fetched package data.
#[derive(Debug, Clone)]
pub struct FetchedPackage {
    pub name: String,
    pub versions: Vec<FetchedVersion>,
}

#[derive(Debug, Clone)]
pub struct FetchedVersion {
    pub version: String,
    pub require: Vec<(String, String)>,
    pub require_dev: Vec<(String, String)>,
    pub replace: Vec<(String, String)>,
    pub provide: Vec<(String, String)>,
    pub dist_url: Option<String>,
    pub dist_type: Option<String>,
    pub dist_shasum: Option<String>,
    pub source_url: Option<String>,
    pub source_type: Option<String>,
    pub source_reference: Option<String>,
}

/// THE TURBO RESOLVER - fastest in the galaxy.
pub struct TurboResolver<F: TurboFetcher> {
    fetcher: Arc<F>,
    config: TurboConfig,
    stats: Arc<TurboStats>,
}

impl<F: TurboFetcher> TurboResolver<F> {
    pub fn new(fetcher: Arc<F>, config: TurboConfig) -> Self {
        Self {
            fetcher,
            config,
            stats: Arc::new(TurboStats::default()),
        }
    }

    pub fn stats(&self) -> &TurboStats {
        &self.stats
    }

    /// Resolve dependencies at WARP SPEED.
    pub async fn resolve(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
    ) -> Result<Resolution, ResolveError> {
        let start = Instant::now();

        // Phase 1: Stream-fetch all packages
        let fetch_start = Instant::now();
        let packages = self.stream_fetch_all(root_deps, dev_deps).await?;
        self.stats
            .fetch_time_ms
            .store(fetch_start.elapsed().as_millis() as u64, Ordering::Relaxed);

        info!(
            packages = packages.len(),
            fetch_ms = fetch_start.elapsed().as_millis(),
            requests = self.stats.requests_total.load(Ordering::Relaxed),
            failed = self.stats.requests_failed.load(Ordering::Relaxed),
            "TURBO fetch complete"
        );

        // Phase 2: Run PubGrub
        let solver_start = Instant::now();
        let resolution = self.run_pubgrub(root_deps, dev_deps, packages)?;
        self.stats
            .solver_time_ms
            .store(solver_start.elapsed().as_millis() as u64, Ordering::Relaxed);

        info!(
            total_ms = start.elapsed().as_millis(),
            packages = resolution.packages.len(),
            "TURBO resolution complete"
        );

        Ok(resolution)
    }

    /// Stream-based fetching - process results AS they arrive.
    async fn stream_fetch_all(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
    ) -> Result<AHashMap<String, PackageEntry>, ResolveError> {
        let packages: Arc<dashmap::DashMap<String, PackageEntry>> =
            Arc::new(dashmap::DashMap::new());
        let seen: Arc<DashSet<String>> = Arc::new(DashSet::new());

        // Seed with root dependencies
        let all_deps: Vec<_> = root_deps
            .iter()
            .chain(if self.config.include_dev {
                dev_deps
            } else {
                &[]
            })
            .collect();

        let mut pending: Vec<String> = Vec::new();
        for dep in &all_deps {
            let name = dep.name.as_str().to_string();
            if !is_platform(&name) && seen.insert(name.clone()) {
                pending.push(name);
            }
        }

        info!(initial = pending.len(), "TURBO fetch starting");

        let mut in_flight = FuturesUnordered::new();

        loop {
            // Launch new requests up to max_concurrent
            while in_flight.len() < self.config.max_concurrent && !pending.is_empty() {
                let name = pending.pop().unwrap();
                let fetcher = Arc::clone(&self.fetcher);
                let timeout = self.config.request_timeout;
                let stats = Arc::clone(&self.stats);

                in_flight.push(async move {
                    stats.requests_total.fetch_add(1, Ordering::Relaxed);
                    match tokio::time::timeout(timeout, fetcher.fetch(name.clone())).await {
                        Ok(result) => (name, result),
                        Err(_) => {
                            stats.requests_failed.fetch_add(1, Ordering::Relaxed);
                            (name, None)
                        }
                    }
                });
            }

            // Done when nothing in flight and nothing pending
            if in_flight.is_empty() {
                break;
            }

            // Wait for next result
            if let Some((name, result)) = in_flight.next().await {
                if let Some(fetched) = result {
                    self.stats.packages_fetched.fetch_add(1, Ordering::Relaxed);

                    if let Some(entry) = convert_fetched(&fetched, self.config.min_stability) {
                        self.stats
                            .versions_total
                            .fetch_add(entry.versions.len() as u64, Ordering::Relaxed);

                        // Queue new dependencies
                        for version in &entry.versions {
                            for dep in &version.dependencies {
                                let dep_name = dep.name.as_str().to_string();
                                if !is_platform(&dep_name) && seen.insert(dep_name.clone()) {
                                    pending.push(dep_name);
                                }
                            }
                        }

                        packages.insert(name, entry);
                    }
                }
            }

            // Log progress periodically
            let fetched = self.stats.packages_fetched.load(Ordering::Relaxed);
            if fetched % 50 == 0 && fetched > 0 {
                info!(
                    fetched,
                    in_flight = in_flight.len(),
                    pending = pending.len(),
                    "TURBO fetch progress"
                );
            }
        }

        info!(
            total = packages.len(),
            requests = self.stats.requests_total.load(Ordering::Relaxed),
            "TURBO fetch complete"
        );

        // Convert DashMap to HashMap
        Ok(packages
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect())
    }

    fn run_pubgrub(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
        packages: AHashMap<String, PackageEntry>,
    ) -> Result<Resolution, ResolveError> {
        let provider = TurboProvider::new(packages, self.config.mode, self.config.min_stability);

        let all_deps: Vec<_> = if self.config.include_dev {
            root_deps.iter().chain(dev_deps.iter()).cloned().collect()
        } else {
            root_deps.to_vec()
        };

        let root_dep_ranges: Vec<_> = all_deps
            .iter()
            .filter(|d| !is_platform(d.name.as_str()))
            .map(|d| (d.name.clone(), d.constraint.ranges().clone()))
            .collect();

        provider.set_root_deps(root_dep_ranges);

        let root = PackageName::new("__root__", "__root__");
        let root_ver = ComposerVersion::new(1, 0, 0);

        match resolve(&provider, root.clone(), root_ver.clone()) {
            Ok(solution) => self.build_resolution(solution, &provider, dev_deps),
            Err(PubGrubError::NoSolution(mut tree)) => {
                tree.collapse_no_versions();
                Err(ResolveError::Conflict {
                    explanation: DefaultStringReporter::report(&tree),
                })
            }
            Err(PubGrubError::ErrorChoosingVersion { package, .. }) => {
                Err(ResolveError::PackageNotFound {
                    name: package.to_string(),
                })
            }
            Err(_) => Err(ResolveError::Cancelled),
        }
    }

    fn build_resolution(
        &self,
        solution: impl IntoIterator<Item = (PackageName, ComposerVersion)>,
        provider: &TurboProvider,
        dev_deps: &[Dependency],
    ) -> Result<Resolution, ResolveError> {
        let dev_names: AHashSet<_> = dev_deps
            .iter()
            .map(|d| d.name.as_str().to_string())
            .collect();

        let mut graph: DiGraph<PackageName, ()> = DiGraph::new();
        let mut indices: AHashMap<String, NodeIndex> = AHashMap::new();
        let mut pkg_map: AHashMap<String, (PackageName, ComposerVersion)> = AHashMap::new();

        for (name, version) in solution {
            if name.as_str() == "__root__/__root__" {
                continue;
            }
            let key = name.as_str().to_string();
            let idx = graph.add_node(name.clone());
            indices.insert(key.clone(), idx);
            pkg_map.insert(key, (name, version));
        }

        // Add edges
        for (key, (name, version)) in &pkg_map {
            if let Some(deps) = provider.deps_for(name, version) {
                let from = indices[key];
                for dep in deps {
                    if let Some(&to) = indices.get(dep.name.as_str()) {
                        graph.add_edge(to, from, ());
                    }
                }
            }
        }

        // Topological sort
        let packages = self.topo_sort(&graph, &indices, pkg_map, &dev_names, provider)?;

        Ok(Resolution {
            packages,
            graph,
            indices,
            platform_packages: vec![],
            duration: Duration::ZERO,
        })
    }

    fn topo_sort(
        &self,
        graph: &DiGraph<PackageName, ()>,
        indices: &AHashMap<String, NodeIndex>,
        mut pkg_map: AHashMap<String, (PackageName, ComposerVersion)>,
        dev_names: &AHashSet<String>,
        provider: &TurboProvider,
    ) -> Result<Vec<ResolvedPackage>, ResolveError> {
        let mut result = Vec::with_capacity(pkg_map.len());
        let mut in_deg: AHashMap<NodeIndex, usize> = AHashMap::new();

        for &idx in indices.values() {
            in_deg.insert(
                idx,
                graph.neighbors_directed(idx, Direction::Incoming).count(),
            );
        }

        let mut queue: Vec<_> = in_deg
            .iter()
            .filter(|(_, d)| **d == 0)
            .map(|(&i, _)| i)
            .collect();

        while !in_deg.is_empty() {
            if queue.is_empty() {
                // Cycle - pick minimum
                if let Some((&idx, _)) = in_deg.iter().min_by_key(|(_, d)| *d) {
                    queue.push(idx);
                } else {
                    break;
                }
            }

            while let Some(idx) = queue.pop() {
                if in_deg.remove(&idx).is_none() {
                    continue;
                }

                let name = &graph[idx];
                let key = name.as_str();

                if let Some((pkg_name, version)) = pkg_map.remove(key) {
                    let deps: Vec<_> = graph
                        .neighbors_directed(idx, Direction::Incoming)
                        .filter_map(|n| graph.node_weight(n).cloned())
                        .collect();

                    let (dist_url, dist_type, dist_shasum, src_url, src_type, src_ref) = provider
                        .version_info(&pkg_name, &version)
                        .map_or((None, None, None, None, None, None), |v| {
                            (
                                v.dist_url.as_ref().map(|s| s.to_string()),
                                v.dist_type.as_ref().map(|s| s.to_string()),
                                v.dist_shasum.as_ref().map(|s| s.to_string()),
                                v.source_url.as_ref().map(|s| s.to_string()),
                                v.source_type.as_ref().map(|s| s.to_string()),
                                v.source_reference.as_ref().map(|s| s.to_string()),
                            )
                        });

                    result.push(ResolvedPackage {
                        name: pkg_name,
                        version,
                        dependencies: deps,
                        is_dev: dev_names.contains(key),
                        dist_url,
                        dist_type,
                        dist_shasum,
                        source_url: src_url,
                        source_type: src_type,
                        source_reference: src_ref,
                    });
                }

                for neighbor in graph.neighbors_directed(idx, Direction::Outgoing) {
                    if let Some(deg) = in_deg.get_mut(&neighbor) {
                        *deg = deg.saturating_sub(1);
                        if *deg == 0 {
                            queue.push(neighbor);
                        }
                    }
                }
            }
        }

        Ok(result)
    }
}

// --- Provider ---

struct TurboProvider {
    packages: AHashMap<String, PackageEntry>,
    mode: ResolutionMode,
    #[allow(dead_code)]
    min_stability: Stability,
    root_deps: parking_lot::Mutex<DependencyConstraints<PackageName, Ranges<ComposerVersion>>>,
}

impl TurboProvider {
    fn new(
        packages: AHashMap<String, PackageEntry>,
        mode: ResolutionMode,
        min_stability: Stability,
    ) -> Self {
        Self {
            packages,
            mode,
            min_stability,
            root_deps: parking_lot::Mutex::new(DependencyConstraints::default()),
        }
    }

    fn set_root_deps(
        &self,
        deps: impl IntoIterator<Item = (PackageName, Ranges<ComposerVersion>)>,
    ) {
        let mut root = self.root_deps.lock();
        root.clear();
        for (n, r) in deps {
            root.insert(n, r);
        }
    }

    fn deps_for(&self, name: &PackageName, version: &ComposerVersion) -> Option<Vec<Dependency>> {
        self.packages
            .get(name.as_str())?
            .versions
            .iter()
            .find(|v| &v.version == version)
            .map(|v| v.dependencies.iter().cloned().collect())
    }

    fn version_info(
        &self,
        name: &PackageName,
        version: &ComposerVersion,
    ) -> Option<&PackageVersion> {
        self.packages
            .get(name.as_str())?
            .versions
            .iter()
            .find(|v| &v.version == version)
    }
}

impl DependencyProvider for TurboProvider {
    type P = PackageName;
    type V = ComposerVersion;
    type VS = Ranges<ComposerVersion>;
    type M = String;
    type Err = Infallible;
    type Priority = std::cmp::Reverse<usize>;

    fn prioritize(
        &self,
        pkg: &PackageName,
        range: &Ranges<ComposerVersion>,
        _: &PackageResolutionStatistics,
    ) -> Self::Priority {
        let count = self
            .packages
            .get(pkg.as_str())
            .map(|e| {
                e.versions
                    .iter()
                    .filter(|v| range.contains(&v.version))
                    .count()
            })
            .unwrap_or(0);
        std::cmp::Reverse(count)
    }

    fn choose_version(
        &self,
        pkg: &PackageName,
        range: &Ranges<ComposerVersion>,
    ) -> Result<Option<ComposerVersion>, Infallible> {
        if pkg.as_str() == "__root__/__root__" {
            let v = ComposerVersion::new(1, 0, 0);
            return Ok(if range.contains(&v) { Some(v) } else { None });
        }

        if is_platform(pkg.as_str()) {
            return Ok(None);
        }

        let entry = match self.packages.get(pkg.as_str()) {
            Some(e) => e,
            None => return Ok(None),
        };

        let matching: Vec<_> = entry
            .versions
            .iter()
            .filter(|v| range.contains(&v.version))
            .collect();

        let best = match self.mode {
            ResolutionMode::PreferHighest | ResolutionMode::PreferStable => matching.first(),
            ResolutionMode::PreferLowest => matching.last(),
        };

        Ok(best.map(|v| v.version.clone()))
    }

    fn get_dependencies(
        &self,
        pkg: &PackageName,
        ver: &ComposerVersion,
    ) -> Result<Dependencies<PackageName, Ranges<ComposerVersion>, String>, Infallible> {
        if pkg.as_str() == "__root__/__root__" {
            return Ok(Dependencies::Available(self.root_deps.lock().clone()));
        }

        if is_platform(pkg.as_str()) {
            return Ok(Dependencies::Available(DependencyConstraints::default()));
        }

        let entry = match self.packages.get(pkg.as_str()) {
            Some(e) => e,
            None => return Ok(Dependencies::Available(DependencyConstraints::default())),
        };

        let version = match entry.versions.iter().find(|v| &v.version == ver) {
            Some(v) => v,
            None => return Ok(Dependencies::Available(DependencyConstraints::default())),
        };

        let mut deps = DependencyConstraints::default();
        for dep in &version.dependencies {
            if !is_platform(dep.name.as_str()) {
                deps.insert(dep.name.clone(), dep.constraint.ranges().clone());
            }
        }

        Ok(Dependencies::Available(deps))
    }
}

// --- Helpers ---

#[inline]
fn is_platform(name: &str) -> bool {
    name == "php"
        || name.starts_with("php-")
        || name.starts_with("ext-")
        || name.starts_with("lib-")
        || name == "composer"
        || name == "composer-plugin-api"
        || name == "composer-runtime-api"
}

fn convert_fetched(pkg: &FetchedPackage, _min_stability: Stability) -> Option<PackageEntry> {
    let name = PackageName::parse(&pkg.name)?;
    let mut entry = PackageEntry::new(name.clone());

    for v in &pkg.versions {
        let version = ComposerVersion::parse(&v.version)?;
        let mut pv = PackageVersion::new(name.clone(), version);

        for (dep_name, constraint) in &v.require {
            if let (Some(n), Some(c)) = (
                PackageName::parse(dep_name),
                ComposerConstraint::parse(constraint),
            ) {
                pv.add_dependency(Dependency::new(n, c));
            }
        }

        for (dep_name, constraint) in &v.replace {
            if let (Some(n), Some(c)) = (
                PackageName::parse(dep_name),
                ComposerConstraint::parse(constraint),
            ) {
                pv.add_replace(Dependency::new(n, c));
            }
        }

        for (dep_name, constraint) in &v.provide {
            if let (Some(n), Some(c)) = (
                PackageName::parse(dep_name),
                ComposerConstraint::parse(constraint),
            ) {
                pv.add_provide(Dependency::new(n, c));
            }
        }

        pv.dist_url = v.dist_url.as_ref().map(|s| Arc::from(s.as_str()));
        pv.dist_type = v.dist_type.as_ref().map(|s| Arc::from(s.as_str()));
        pv.dist_shasum = v.dist_shasum.as_ref().map(|s| Arc::from(s.as_str()));
        pv.source_url = v.source_url.as_ref().map(|s| Arc::from(s.as_str()));
        pv.source_type = v.source_type.as_ref().map(|s| Arc::from(s.as_str()));
        pv.source_reference = v.source_reference.as_ref().map(|s| Arc::from(s.as_str()));

        entry.add_version(pv);
    }

    entry.sort_versions();

    if entry.versions.is_empty() {
        None
    } else {
        Some(entry)
    }
}
