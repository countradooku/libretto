//! Ultra-fast async resolver.
//!
//! This resolver uses a completely async approach with no blocking calls.
//! It pre-fetches all reachable packages in parallel before running PubGrub.

use crate::package::{Dependency, PackageEntry, PackageName, PackageVersion};
use crate::provider::ResolutionMode;
use crate::remote::{AsyncPackageFetcher, RemotePackage};
use crate::resolver::{Resolution, ResolveError, ResolvedPackage};
use crate::version::{ComposerConstraint, ComposerVersion, Stability};
use ahash::{AHashMap, AHashSet};
use dashmap::DashMap;
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
use tokio::sync::Semaphore;
use tracing::{debug, info, warn};
use version_ranges::Ranges;

/// Fast resolver statistics.
#[derive(Debug, Default)]
pub struct FastStats {
    pub packages_fetched: AtomicU64,
    pub versions_total: AtomicU64,
    pub fetch_time_ms: AtomicU64,
    pub solver_time_ms: AtomicU64,
}

/// Fast resolver configuration.
#[derive(Debug, Clone)]
pub struct FastConfig {
    pub max_concurrent: usize,
    pub mode: ResolutionMode,
    pub min_stability: Stability,
    pub include_dev: bool,
    pub max_versions: usize,
}

impl Default for FastConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 32,
            mode: ResolutionMode::PreferHighest,
            min_stability: Stability::Stable,
            include_dev: true,
            max_versions: 100,
        }
    }
}

/// Ultra-fast async resolver.
pub struct FastResolver<F: AsyncPackageFetcher> {
    fetcher: Arc<F>,
    config: FastConfig,
    stats: Arc<FastStats>,
}

impl<F: AsyncPackageFetcher + 'static> FastResolver<F> {
    pub fn new(fetcher: Arc<F>, config: FastConfig) -> Self {
        Self {
            fetcher,
            config,
            stats: Arc::new(FastStats::default()),
        }
    }

    pub fn stats(&self) -> &FastStats {
        &self.stats
    }

    /// Resolve dependencies fully async.
    pub async fn resolve(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
    ) -> Result<Resolution, ResolveError> {
        let start = Instant::now();

        // Phase 1: Discover and fetch all reachable packages in parallel
        let fetch_start = Instant::now();
        let packages = self.fetch_all(root_deps, dev_deps).await?;
        self.stats
            .fetch_time_ms
            .store(fetch_start.elapsed().as_millis() as u64, Ordering::Relaxed);

        info!(
            packages = packages.len(),
            fetch_ms = fetch_start.elapsed().as_millis(),
            "fetched all packages"
        );

        // Phase 2: Run PubGrub synchronously on pre-fetched data
        let solver_start = Instant::now();
        let resolution = self.run_pubgrub(root_deps, dev_deps, packages)?;
        self.stats
            .solver_time_ms
            .store(solver_start.elapsed().as_millis() as u64, Ordering::Relaxed);

        info!(
            total_ms = start.elapsed().as_millis(),
            fetch_ms = self.stats.fetch_time_ms.load(Ordering::Relaxed),
            solver_ms = self.stats.solver_time_ms.load(Ordering::Relaxed),
            "resolution complete"
        );

        Ok(resolution)
    }

    /// Fetch all reachable packages in parallel using BFS.
    async fn fetch_all(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
    ) -> Result<AHashMap<String, PackageEntry>, ResolveError> {
        let packages: DashMap<String, PackageEntry> = DashMap::new();
        let seen: DashMap<String, ()> = DashMap::new();
        let semaphore = Arc::new(Semaphore::new(self.config.max_concurrent));

        // Collect initial packages to fetch
        let mut to_fetch: Vec<String> = Vec::new();

        for dep in root_deps.iter().chain(if self.config.include_dev {
            dev_deps
        } else {
            &[]
        }) {
            let name = dep.name.as_str().to_string();
            if !is_platform_package(&name) && !seen.contains_key(&name) {
                seen.insert(name.clone(), ());
                to_fetch.push(name);
            }
        }

        // BFS: fetch packages level by level
        while !to_fetch.is_empty() {
            debug!(batch_size = to_fetch.len(), "fetching batch");

            // Fetch current batch in parallel
            let fetcher = Arc::clone(&self.fetcher);
            let sem = Arc::clone(&semaphore);

            let handles: Vec<_> = to_fetch
                .iter()
                .map(|name| {
                    let name = name.clone();
                    let fetcher = Arc::clone(&fetcher);
                    let sem = Arc::clone(&sem);

                    tokio::spawn(async move {
                        let _permit = sem.acquire().await.ok()?;
                        let result = fetcher.fetch_package(&name).await;
                        Some((name, result))
                    })
                })
                .collect();

            // Wait for all fetches
            let results = futures::future::join_all(handles).await;

            // Collect next level of packages to fetch
            let mut next_level: Vec<String> = Vec::new();

            for result in results {
                if let Ok(Some((name, Some(remote_pkgs)))) = result {
                    self.stats.packages_fetched.fetch_add(1, Ordering::Relaxed);

                    if let Some(entry) = convert_remote_packages(
                        &name,
                        &remote_pkgs,
                        self.config.min_stability,
                        self.config.max_versions,
                    ) {
                        self.stats
                            .versions_total
                            .fetch_add(entry.versions.len() as u64, Ordering::Relaxed);

                        // Collect dependencies from ALL versions (PubGrub might pick any)
                        for version in &entry.versions {
                            for dep in &version.dependencies {
                                let dep_name = dep.name.as_str().to_string();
                                if !is_platform_package(&dep_name) && !seen.contains_key(&dep_name)
                                {
                                    seen.insert(dep_name.clone(), ());
                                    next_level.push(dep_name);
                                }
                            }
                        }

                        packages.insert(name, entry);
                    }
                }
            }

            to_fetch = next_level;
        }

        Ok(packages.into_iter().collect())
    }

    /// Run PubGrub on pre-fetched packages.
    fn run_pubgrub(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
        packages: AHashMap<String, PackageEntry>,
    ) -> Result<Resolution, ResolveError> {
        let provider = FastProvider::new(packages, self.config.mode, self.config.min_stability);

        // Set root dependencies
        let all_deps: Vec<_> = if self.config.include_dev {
            root_deps.iter().chain(dev_deps.iter()).cloned().collect()
        } else {
            root_deps.to_vec()
        };

        let root_dep_ranges: Vec<_> = all_deps
            .iter()
            .filter(|d| !is_platform_package(d.name.as_str()))
            .map(|d| (d.name.clone(), d.constraint.ranges().clone()))
            .collect();

        provider.set_root_dependencies(root_dep_ranges);

        let root_name = PackageName::new("__root__", "__root__");
        let root_version = ComposerVersion::new(1, 0, 0);

        let resolution_result = resolve(&provider, root_name.clone(), root_version.clone());

        let selected = match resolution_result {
            Ok(selected) => selected,
            Err(PubGrubError::NoSolution(mut derivation_tree)) => {
                derivation_tree.collapse_no_versions();
                let explanation = DefaultStringReporter::report(&derivation_tree);
                return Err(ResolveError::Conflict { explanation });
            }
            Err(PubGrubError::ErrorChoosingVersion { package, .. }) => {
                return Err(ResolveError::PackageNotFound {
                    name: package.to_string(),
                });
            }
            Err(_) => {
                return Err(ResolveError::Cancelled);
            }
        };

        self.build_resolution(selected, &provider, dev_deps)
    }

    fn build_resolution(
        &self,
        selected: impl IntoIterator<Item = (PackageName, ComposerVersion)>,
        provider: &FastProvider,
        dev_deps: &[Dependency],
    ) -> Result<Resolution, ResolveError> {
        let dev_dep_names: AHashSet<String> = dev_deps
            .iter()
            .map(|d| d.name.as_str().to_string())
            .collect();

        let mut graph: DiGraph<PackageName, ()> = DiGraph::new();
        let mut indices: AHashMap<String, NodeIndex> = AHashMap::new();
        let mut packages_map: AHashMap<String, (PackageName, ComposerVersion)> = AHashMap::new();

        for (name, version) in selected {
            if name.as_str() == "__root__/__root__" {
                continue;
            }

            let key = name.as_str().to_string();
            let idx = graph.add_node(name.clone());
            indices.insert(key.clone(), idx);
            packages_map.insert(key, (name, version));
        }

        for (key, (name, version)) in &packages_map {
            if let Some(deps) = provider.get_deps_for_version(name, version) {
                let dependent_idx = indices[key];

                for dep in deps {
                    if let Some(&dependency_idx) = indices.get(dep.name.as_str()) {
                        graph.add_edge(dependency_idx, dependent_idx, ());
                    }
                }
            }
        }

        let packages =
            self.topological_sort(&graph, &indices, packages_map, &dev_dep_names, provider)?;

        Ok(Resolution {
            packages,
            graph,
            indices,
            platform_packages: vec![],
            duration: Duration::ZERO,
        })
    }

    fn topological_sort(
        &self,
        graph: &DiGraph<PackageName, ()>,
        indices: &AHashMap<String, NodeIndex>,
        mut packages_map: AHashMap<String, (PackageName, ComposerVersion)>,
        dev_deps: &AHashSet<String>,
        provider: &FastProvider,
    ) -> Result<Vec<ResolvedPackage>, ResolveError> {
        let mut result = Vec::with_capacity(packages_map.len());
        let mut in_degree: AHashMap<NodeIndex, usize> = AHashMap::new();

        for &idx in indices.values() {
            in_degree.insert(
                idx,
                graph.neighbors_directed(idx, Direction::Incoming).count(),
            );
        }

        let mut queue: Vec<NodeIndex> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&idx, _)| idx)
            .collect();

        while !in_degree.is_empty() {
            if queue.is_empty() {
                if let Some((idx, _)) = in_degree.iter().min_by_key(|(_, d)| *d) {
                    queue.push(*idx);
                } else {
                    break;
                }
            }

            while let Some(idx) = queue.pop() {
                if in_degree.remove(&idx).is_none() {
                    continue;
                }

                let name = &graph[idx];
                let key = name.as_str();

                if let Some((pkg_name, version)) = packages_map.remove(key) {
                    let deps: Vec<PackageName> = graph
                        .neighbors_directed(idx, Direction::Incoming)
                        .filter_map(|n| graph.node_weight(n).cloned())
                        .collect();

                    // Get dist/source info from the provider
                    let (
                        dist_url,
                        dist_type,
                        dist_shasum,
                        source_url,
                        source_type,
                        source_reference,
                    ) = if let Some(version_info) = provider.get_version_info(&pkg_name, &version) {
                        (
                            version_info.dist_url.as_ref().map(|s| s.to_string()),
                            version_info.dist_type.as_ref().map(|s| s.to_string()),
                            version_info.dist_shasum.as_ref().map(|s| s.to_string()),
                            version_info.source_url.as_ref().map(|s| s.to_string()),
                            version_info.source_type.as_ref().map(|s| s.to_string()),
                            version_info
                                .source_reference
                                .as_ref()
                                .map(|s| s.to_string()),
                        )
                    } else {
                        (None, None, None, None, None, None)
                    };

                    result.push(ResolvedPackage {
                        name: pkg_name,
                        version,
                        dependencies: deps,
                        is_dev: dev_deps.contains(key),
                        dist_url,
                        dist_type,
                        dist_shasum,
                        source_url,
                        source_type,
                        source_reference,
                    });
                }

                for neighbor in graph.neighbors_directed(idx, Direction::Outgoing) {
                    if let Some(deg) = in_degree.get_mut(&neighbor) {
                        if *deg > 0 {
                            *deg -= 1;
                            if *deg == 0 {
                                queue.push(neighbor);
                            }
                        }
                    }
                }
            }
        }

        Ok(result)
    }
}

/// Fast provider for PubGrub.
struct FastProvider {
    packages: AHashMap<String, PackageEntry>,
    mode: ResolutionMode,
    min_stability: Stability,
    root_deps: parking_lot::Mutex<DependencyConstraints<PackageName, Ranges<ComposerVersion>>>,
}

impl FastProvider {
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

    fn set_root_dependencies(
        &self,
        deps: impl IntoIterator<Item = (PackageName, Ranges<ComposerVersion>)>,
    ) {
        let mut root_deps = self.root_deps.lock();
        root_deps.clear();
        for (name, ranges) in deps {
            root_deps.insert(name, ranges);
        }
    }

    fn get_deps_for_version(
        &self,
        name: &PackageName,
        version: &ComposerVersion,
    ) -> Option<Vec<Dependency>> {
        let entry = self.packages.get(name.as_str())?;
        entry
            .versions
            .iter()
            .find(|v| &v.version == version)
            .map(|v| v.dependencies.iter().cloned().collect())
    }

    fn get_version_info(
        &self,
        name: &PackageName,
        version: &ComposerVersion,
    ) -> Option<&PackageVersion> {
        let entry = self.packages.get(name.as_str())?;
        entry.versions.iter().find(|v| &v.version == version)
    }
}

impl DependencyProvider for FastProvider {
    type P = PackageName;
    type V = ComposerVersion;
    type VS = Ranges<ComposerVersion>;
    type M = String;
    type Err = Infallible;
    type Priority = std::cmp::Reverse<usize>;

    fn prioritize(
        &self,
        package: &PackageName,
        range: &Ranges<ComposerVersion>,
        _stats: &PackageResolutionStatistics,
    ) -> Self::Priority {
        let count = self
            .packages
            .get(package.as_str())
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
        package: &PackageName,
        range: &Ranges<ComposerVersion>,
    ) -> Result<Option<ComposerVersion>, Infallible> {
        if package.as_str() == "__root__/__root__" {
            let root_version = ComposerVersion::new(1, 0, 0);
            return Ok(if range.contains(&root_version) {
                Some(root_version)
            } else {
                None
            });
        }

        if is_platform_package(package.as_str()) {
            return Ok(None);
        }

        let entry = match self.packages.get(package.as_str()) {
            Some(e) => e,
            None => {
                warn!(package = %package.as_str(), "package not in pool");
                return Ok(None);
            }
        };

        let matching: Vec<_> = entry
            .versions
            .iter()
            .filter(|v| range.contains(&v.version))
            .collect();

        if matching.is_empty() {
            debug!(
                package = %package.as_str(),
                available = entry.versions.len(),
                first_version = ?entry.versions.first().map(|v| v.version.to_string()),
                "no matching version in range"
            );
        }

        let best = match self.mode {
            ResolutionMode::PreferHighest | ResolutionMode::PreferStable => matching.first(),
            ResolutionMode::PreferLowest => matching.last(),
        };

        Ok(best.map(|v| v.version.clone()))
    }

    fn get_dependencies(
        &self,
        package: &PackageName,
        version: &ComposerVersion,
    ) -> Result<Dependencies<PackageName, Ranges<ComposerVersion>, String>, Infallible> {
        if package.as_str() == "__root__/__root__" {
            return Ok(Dependencies::Available(self.root_deps.lock().clone()));
        }

        if is_platform_package(package.as_str()) {
            return Ok(Dependencies::Available(DependencyConstraints::default()));
        }

        let entry = match self.packages.get(package.as_str()) {
            Some(e) => e,
            None => return Ok(Dependencies::Available(DependencyConstraints::default())),
        };

        let pkg_version = match entry.versions.iter().find(|v| &v.version == version) {
            Some(v) => v,
            None => return Ok(Dependencies::Available(DependencyConstraints::default())),
        };

        let mut result: DependencyConstraints<PackageName, Ranges<ComposerVersion>> =
            DependencyConstraints::default();

        for dep in &pkg_version.dependencies {
            if !is_platform_package(dep.name.as_str()) {
                result.insert(dep.name.clone(), dep.constraint.ranges().clone());
            }
        }

        Ok(Dependencies::Available(result))
    }
}

#[inline]
fn is_platform_package(name: &str) -> bool {
    name == "php"
        || name.starts_with("php-")
        || name.starts_with("ext-")
        || name.starts_with("lib-")
        || name == "composer"
        || name == "composer-plugin-api"
        || name == "composer-runtime-api"
}

fn convert_remote_packages(
    name: &str,
    packages: &[RemotePackage],
    min_stability: Stability,
    max_versions: usize,
) -> Option<PackageEntry> {
    let pkg_name = PackageName::parse(name)?;
    let mut entry = PackageEntry::new(pkg_name.clone());

    for pkg in packages {
        let version = match ComposerVersion::parse(&pkg.version) {
            Some(v) => v,
            None => {
                // Skip unparseable versions but continue processing others
                continue;
            }
        };

        // Include all versions including dev - PubGrub will filter based on constraints
        // Dev versions are needed when packages explicitly require them (e.g., "dev-master")
        let _ = min_stability; // Stability filtering happens at constraint level

        let mut pkg_version = PackageVersion::new(pkg_name.clone(), version);

        for (dep_name, constraint) in &pkg.require {
            let constraint_str = if constraint == "self.version" {
                &pkg.version
            } else {
                constraint
            };
            if let (Some(name), Some(constraint)) = (
                PackageName::parse(dep_name),
                ComposerConstraint::parse(constraint_str),
            ) {
                pkg_version.add_dependency(Dependency::new(name, constraint));
            }
        }

        for (dep_name, constraint) in &pkg.replace {
            if let (Some(name), Some(constraint)) = (
                PackageName::parse(dep_name),
                ComposerConstraint::parse(constraint),
            ) {
                pkg_version.add_replace(Dependency::new(name, constraint));
            }
        }

        for (dep_name, constraint) in &pkg.provide {
            if let (Some(name), Some(constraint)) = (
                PackageName::parse(dep_name),
                ComposerConstraint::parse(constraint),
            ) {
                pkg_version.add_provide(Dependency::new(name, constraint));
            }
        }

        pkg_version.dist_url = pkg.dist_url.as_ref().map(|s| Arc::from(s.as_str()));
        pkg_version.dist_type = pkg.dist_type.as_ref().map(|s| Arc::from(s.as_str()));
        pkg_version.dist_shasum = pkg.dist_shasum.as_ref().map(|s| Arc::from(s.as_str()));
        pkg_version.source_url = pkg.source_url.as_ref().map(|s| Arc::from(s.as_str()));
        pkg_version.source_type = pkg.source_type.as_ref().map(|s| Arc::from(s.as_str()));
        pkg_version.source_reference = pkg.source_reference.as_ref().map(|s| Arc::from(s.as_str()));

        entry.add_version(pkg_version);
    }

    entry.sort_versions();

    // Don't truncate versions - PubGrub needs all available versions to find valid solutions
    // Truncating can cause resolution failures when older major versions are needed
    let _ = max_versions; // unused now

    if entry.versions.is_empty() {
        None
    } else {
        Some(entry)
    }
}
