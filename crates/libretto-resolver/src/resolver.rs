//! High-level dependency resolver.
//!
//! This module provides the main `Resolver` struct that orchestrates
//! dependency resolution using the PubGrub algorithm.

use crate::index::{PackageIndex, PackageSource};
use crate::package::{Dependency, PackageName};
use crate::provider::{ComposerProvider, ProviderConfig, ProviderError, ResolutionMode};
use crate::version::{ComposerVersion, Stability};
use ahash::{AHashMap, AHashSet};
use petgraph::Direction;
use petgraph::graph::{DiGraph, NodeIndex};
use pubgrub::{DefaultStringReporter, PubGrubError, Reporter, resolve};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{debug, info, warn};

/// Resolution options.
#[derive(Debug, Clone)]
pub struct ResolveOptions {
    /// Resolution mode (prefer highest, lowest, or stable).
    pub mode: ResolutionMode,
    /// Minimum stability to accept.
    pub min_stability: Stability,
    /// Include dev dependencies.
    pub include_dev: bool,
    /// Maximum resolution time before timeout.
    pub timeout: Option<Duration>,
    /// Packages to exclude from resolution.
    pub excluded_packages: Vec<String>,
    /// Pre-locked packages (versions already determined).
    pub locked_packages: AHashMap<String, ComposerVersion>,
}

impl Default for ResolveOptions {
    fn default() -> Self {
        Self {
            mode: ResolutionMode::PreferHighest,
            min_stability: Stability::Stable,
            include_dev: false,
            timeout: Some(Duration::from_secs(120)),
            excluded_packages: Vec::new(),
            locked_packages: AHashMap::new(),
        }
    }
}

/// A resolved package with its version and dependencies.
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    /// Package name.
    pub name: PackageName,
    /// Resolved version.
    pub version: ComposerVersion,
    /// Direct dependencies.
    pub dependencies: Vec<PackageName>,
    /// Is this a dev dependency.
    pub is_dev: bool,
    /// Distribution URL for downloading.
    pub dist_url: Option<String>,
    /// Distribution type (zip, tar, etc.).
    pub dist_type: Option<String>,
    /// Distribution checksum.
    pub dist_shasum: Option<String>,
    /// Source URL (git repository).
    pub source_url: Option<String>,
    /// Source type (git, hg, etc.).
    pub source_type: Option<String>,
    /// Source reference (commit/tag).
    pub source_reference: Option<String>,
}

/// Result of dependency resolution.
#[derive(Debug)]
pub struct Resolution {
    /// Resolved packages in topological order (dependencies first).
    pub packages: Vec<ResolvedPackage>,
    /// Dependency graph.
    pub graph: DiGraph<PackageName, ()>,
    /// Node indices by package name.
    pub indices: AHashMap<String, NodeIndex>,
    /// Platform packages encountered.
    pub platform_packages: Vec<String>,
    /// Resolution time.
    pub duration: Duration,
}

impl Resolution {
    /// Get the number of resolved packages.
    #[must_use]
    pub fn len(&self) -> usize {
        self.packages.len()
    }

    /// Check if resolution is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.packages.is_empty()
    }

    /// Get a resolved package by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ResolvedPackage> {
        self.packages.iter().find(|p| p.name.as_str() == name)
    }

    /// Check if a package is in the resolution.
    #[must_use]
    pub fn contains(&self, name: &str) -> bool {
        self.indices.contains_key(name)
    }

    /// Get packages that depend on the given package.
    #[must_use]
    pub fn dependents(&self, name: &str) -> Vec<&ResolvedPackage> {
        let Some(&idx) = self.indices.get(name) else {
            return Vec::new();
        };

        // Outgoing edges point to packages that depend on this one
        self.graph
            .neighbors_directed(idx, Direction::Outgoing)
            .filter_map(|n| {
                let pkg_name = self.graph.node_weight(n)?;
                self.get(pkg_name.as_str())
            })
            .collect()
    }

    /// Get packages that the given package depends on.
    #[must_use]
    pub fn dependencies_of(&self, name: &str) -> Vec<&ResolvedPackage> {
        let Some(&idx) = self.indices.get(name) else {
            return Vec::new();
        };

        // Incoming edges come from dependencies
        self.graph
            .neighbors_directed(idx, Direction::Incoming)
            .filter_map(|n| {
                let pkg_name = self.graph.node_weight(n)?;
                self.get(pkg_name.as_str())
            })
            .collect()
    }

    /// Get packages in installation order (dependencies first).
    #[must_use]
    pub fn installation_order(&self) -> &[ResolvedPackage] {
        &self.packages
    }
}

/// Resolution error.
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    /// Package not found.
    #[error("package not found: {name}")]
    PackageNotFound {
        /// Package name.
        name: String,
    },

    /// No version satisfies constraints.
    #[error("no version of {name} satisfies: {constraint}")]
    NoMatchingVersion {
        /// Package name.
        name: String,
        /// Constraint that couldn't be satisfied.
        constraint: String,
    },

    /// Conflict detected.
    #[error("dependency conflict:\n{explanation}")]
    Conflict {
        /// Human-readable explanation.
        explanation: String,
    },

    /// Resolution timeout.
    #[error("resolution timed out after {elapsed:?}")]
    Timeout {
        /// Time elapsed before timeout.
        elapsed: Duration,
    },

    /// Resolution cancelled.
    #[error("resolution cancelled")]
    Cancelled,

    /// Circular dependency.
    #[error("circular dependency: {cycle}")]
    CircularDependency {
        /// Packages in the cycle.
        cycle: String,
    },

    /// Provider error.
    #[error("provider error: {0}")]
    Provider(ProviderError),
}

/// The dependency resolver.
pub struct Resolver<S: PackageSource + 'static> {
    /// Package index.
    index: Arc<PackageIndex<S>>,
}

impl<S: PackageSource + 'static> std::fmt::Debug for Resolver<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Resolver")
            .field("index", &self.index)
            .finish()
    }
}

impl<S: PackageSource + 'static> Resolver<S> {
    /// Create a new resolver with the given index.
    pub fn new(index: Arc<PackageIndex<S>>) -> Self {
        Self { index }
    }

    /// Resolve dependencies for the given root requirements.
    ///
    /// # Errors
    ///
    /// Returns an error if resolution fails.
    pub fn resolve(
        &self,
        root_deps: &[Dependency],
        options: &ResolveOptions,
    ) -> Result<Resolution, ResolveError> {
        self.resolve_with_dev(root_deps, &[], options)
    }

    /// Resolve dependencies with separate dev dependencies tracking.
    ///
    /// # Errors
    ///
    /// Returns an error if resolution fails.
    pub fn resolve_with_dev(
        &self,
        root_deps: &[Dependency],
        dev_deps: &[Dependency],
        options: &ResolveOptions,
    ) -> Result<Resolution, ResolveError> {
        let start = Instant::now();

        // Track which packages are dev dependencies
        let root_dev_dep_names: AHashSet<String> = dev_deps
            .iter()
            .map(|d| d.name.as_str().to_string())
            .collect();

        // Combine all dependencies for resolution
        let all_deps: Vec<_> = if options.include_dev {
            root_deps.iter().chain(dev_deps.iter()).cloned().collect()
        } else {
            root_deps.to_vec()
        };

        info!(
            deps = all_deps.len(),
            dev_deps = dev_deps.len(),
            mode = ?options.mode,
            min_stability = ?options.min_stability,
            include_dev = options.include_dev,
            "starting resolution"
        );

        // Prefetch root dependencies
        let root_names: Vec<_> = all_deps.iter().map(|d| d.name.clone()).collect();
        self.index.prefetch(&root_names);

        // Create provider
        let config = ProviderConfig {
            mode: options.mode,
            min_stability: options.min_stability,
            include_dev: options.include_dev,
            max_versions_per_package: 100,
        };
        let mut provider = ComposerProvider::new(Arc::clone(&self.index), config);

        // Add exclusions
        for excluded in &options.excluded_packages {
            provider.exclude(excluded);
        }

        // Create root package
        let root_name = PackageName::new("__root__", "__root__");
        let root_version = ComposerVersion::new(1, 0, 0);

        // Register locked packages with the provider
        for (name, locked_version) in &options.locked_packages {
            if let Some(pkg_name) = PackageName::parse(name) {
                provider.lock_version(pkg_name, locked_version.clone());
            }
        }

        // Set root dependencies on the provider
        let root_dep_ranges = all_deps
            .iter()
            .filter(|d| !is_platform_package(d.name.as_str()))
            .map(|d| (d.name.clone(), d.constraint.ranges().clone()));
        provider.set_root_dependencies(root_dep_ranges);

        // Run PubGrub resolution
        debug!("running pubgrub resolution");

        let resolution_result = resolve(&provider, root_name.clone(), root_version.clone());

        // Check timeout
        if let Some(timeout) = options.timeout {
            if start.elapsed() > timeout {
                return Err(ResolveError::Timeout {
                    elapsed: start.elapsed(),
                });
            }
        }

        // Handle result
        let selected = match resolution_result {
            Ok(selected) => selected,
            Err(PubGrubError::NoSolution(mut derivation_tree)) => {
                derivation_tree.collapse_no_versions();
                let explanation = DefaultStringReporter::report(&derivation_tree);
                return Err(ResolveError::Conflict { explanation });
            }
            Err(PubGrubError::ErrorInShouldCancel(_e)) => {
                // Infallible, can't happen
                return Err(ResolveError::Cancelled);
            }
            Err(PubGrubError::ErrorChoosingVersion { package, source: _ }) => {
                return Err(ResolveError::PackageNotFound {
                    name: package.to_string(),
                });
            }
            Err(PubGrubError::ErrorRetrievingDependencies {
                package,
                version,
                source: _,
            }) => {
                warn!(package = %package, version = %version, "error retrieving dependencies");
                return Err(ResolveError::PackageNotFound {
                    name: package.to_string(),
                });
            }
        };

        // Build resolution result
        let resolution =
            self.build_resolution(selected, &provider, &root_dev_dep_names, start.elapsed())?;

        info!(
            packages = resolution.len(),
            duration = ?resolution.duration,
            "resolution complete"
        );

        Ok(resolution)
    }

    /// Build the resolution result from selected packages.
    fn build_resolution(
        &self,
        selected: impl IntoIterator<Item = (PackageName, ComposerVersion)>,
        provider: &ComposerProvider<S>,
        root_dev_deps: &AHashSet<String>,
        duration: Duration,
    ) -> Result<Resolution, ResolveError> {
        let mut graph: DiGraph<PackageName, ()> = DiGraph::new();
        let mut indices: AHashMap<String, NodeIndex> = AHashMap::new();
        let mut packages_map: AHashMap<String, (PackageName, ComposerVersion)> = AHashMap::new();

        // Add nodes
        for (name, version) in selected {
            // Skip root package
            if name.as_str() == "__root__/__root__" {
                continue;
            }

            let key = name.as_str().to_string();
            let idx = graph.add_node(name.clone());
            indices.insert(key.clone(), idx);
            packages_map.insert(key, (name, version));
        }

        // Add edges (dependency -> dependent, so dependencies come first in topological order)
        for (key, (name, version)) in &packages_map {
            if let Some(deps) = self.index.get_dependencies(name, version) {
                let dependent_idx = indices[key];

                for dep in deps {
                    if let Some(&dependency_idx) = indices.get(dep.name.as_str()) {
                        // Edge from dependency to dependent
                        // This means: dependency must be installed before dependent
                        graph.add_edge(dependency_idx, dependent_idx, ());
                    }
                }
            }
        }

        // Topological sort (with cycle handling)
        let packages = self.topological_sort(&graph, &indices, packages_map, root_dev_deps)?;

        // Get platform packages
        let platform_packages: Vec<String> = provider
            .platform_packages()
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        Ok(Resolution {
            packages,
            graph,
            indices,
            platform_packages,
            duration,
        })
    }

    /// Perform topological sort with cycle breaking.
    fn topological_sort(
        &self,
        graph: &DiGraph<PackageName, ()>,
        indices: &AHashMap<String, NodeIndex>,
        mut packages_map: AHashMap<String, (PackageName, ComposerVersion)>,
        root_dev_deps: &AHashSet<String>,
    ) -> Result<Vec<ResolvedPackage>, ResolveError> {
        let mut result = Vec::with_capacity(packages_map.len());
        let mut in_degree: AHashMap<NodeIndex, usize> = AHashMap::new();

        // Calculate in-degrees
        for &idx in indices.values() {
            in_degree.insert(
                idx,
                graph.neighbors_directed(idx, Direction::Incoming).count(),
            );
        }

        // Start with nodes that have no incoming edges
        let mut queue: Vec<NodeIndex> = in_degree
            .iter()
            .filter(|(_, deg)| **deg == 0)
            .map(|(&idx, _)| idx)
            .collect();

        // Process until all nodes are handled
        while !in_degree.is_empty() {
            // If queue is empty but nodes remain, we have a cycle. Break it.
            if queue.is_empty() {
                // Heuristic: pick node with lowest in-degree (fewest dependencies blocking it)
                if let Some((idx, _)) = in_degree.iter().min_by_key(|(_, d)| *d) {
                    queue.push(*idx);
                } else {
                    break; // Should not happen if in_degree is not empty
                }
            }

            while let Some(idx) = queue.pop() {
                // Remove from in_degree to mark as processed
                // If already removed, skip (handle potential duplicates if any)
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

                    let is_dev = root_dev_deps.contains(key);

                    result.push(ResolvedPackage {
                        name: pkg_name,
                        version,
                        dependencies: deps,
                        is_dev,
                        dist_url: None,
                        dist_type: None,
                        dist_shasum: None,
                        source_url: None,
                        source_type: None,
                        source_reference: None,
                    });
                }

                // Decrement in-degree of neighbors (dependents)
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

/// Check if a package is a platform package.
fn is_platform_package(name: &str) -> bool {
    name == "php"
        || name.starts_with("php-")
        || name.starts_with("ext-")
        || name.starts_with("lib-")
        || name == "composer"
        || name == "composer-plugin-api"
        || name == "composer-runtime-api"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ComposerConstraint, index::MemorySource};

    fn create_test_resolver() -> Resolver<MemorySource> {
        let source = MemorySource::new();
        source.add_version("test/a", "1.0.0", vec![]);
        source.add_version("test/a", "1.1.0", vec![]);
        source.add_version("test/a", "2.0.0", vec![]);
        source.add_version("test/b", "1.0.0", vec![("test/a", "^1.0")]);
        source.add_version(
            "test/c",
            "1.0.0",
            vec![("test/a", "^1.0"), ("test/b", "^1.0")],
        );

        let index = Arc::new(PackageIndex::new(source));
        Resolver::new(index)
    }

    #[test]
    fn test_simple_resolution() {
        let resolver = create_test_resolver();
        let deps = vec![Dependency::new(
            PackageName::parse("test/a").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let resolution = resolver.resolve(&deps, &ResolveOptions::default()).unwrap();

        assert_eq!(resolution.len(), 1);
        assert_eq!(resolution.packages[0].name.as_str(), "test/a");
        assert_eq!(resolution.packages[0].version.minor, 1); // Should pick 1.1.0
    }

    #[test]
    fn test_transitive_resolution() {
        let resolver = create_test_resolver();
        let deps = vec![Dependency::new(
            PackageName::parse("test/b").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let resolution = resolver.resolve(&deps, &ResolveOptions::default()).unwrap();

        assert_eq!(resolution.len(), 2);
        assert!(resolution.contains("test/a"));
        assert!(resolution.contains("test/b"));
    }

    #[test]
    fn test_prefer_lowest() {
        let resolver = create_test_resolver();
        let deps = vec![Dependency::new(
            PackageName::parse("test/a").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let options = ResolveOptions {
            mode: ResolutionMode::PreferLowest,
            ..Default::default()
        };

        let resolution = resolver.resolve(&deps, &options).unwrap();

        assert_eq!(resolution.packages[0].version.minor, 0); // Should pick 1.0.0
    }

    #[test]
    fn test_exclusion() {
        let resolver = create_test_resolver();
        let deps = vec![Dependency::new(
            PackageName::parse("test/c").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let options = ResolveOptions {
            excluded_packages: vec!["test/b".to_string()],
            ..Default::default()
        };

        // This should fail because test/c requires test/b which is excluded
        let result = resolver.resolve(&deps, &options);
        // Resolution fails because test/b is excluded but test/c needs it
        // PubGrub will report this as "no version of test/b" conflict
        assert!(
            result.is_err(),
            "resolution should fail when required package is excluded: {result:?}"
        );
    }

    #[test]
    fn test_locked_packages() {
        let resolver = create_test_resolver();
        let deps = vec![Dependency::new(
            PackageName::parse("test/a").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let mut locked = AHashMap::new();
        locked.insert(
            "test/a".to_string(),
            ComposerVersion::parse("1.0.0").unwrap(),
        );

        let options = ResolveOptions {
            locked_packages: locked,
            ..Default::default()
        };

        let resolution = resolver.resolve(&deps, &options).unwrap();

        // Should use the locked version
        assert_eq!(resolution.packages[0].version.minor, 0);
    }

    #[test]
    fn test_installation_order() {
        let resolver = create_test_resolver();
        let deps = vec![Dependency::new(
            PackageName::parse("test/c").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let resolution = resolver.resolve(&deps, &ResolveOptions::default()).unwrap();

        // Dependencies should come before dependents
        let a_pos = resolution
            .packages
            .iter()
            .position(|p| p.name.as_str() == "test/a")
            .unwrap();
        let b_pos = resolution
            .packages
            .iter()
            .position(|p| p.name.as_str() == "test/b")
            .unwrap();
        let c_pos = resolution
            .packages
            .iter()
            .position(|p| p.name.as_str() == "test/c")
            .unwrap();

        assert!(a_pos < b_pos);
        assert!(b_pos < c_pos);
    }
}
