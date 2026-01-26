//! PubGrub algorithm implementation for dependency resolution.
//!
//! PubGrub is a version solving algorithm that provides:
//! - Efficient backtracking through conflict-driven clause learning
//! - Clear explanations of why resolution failed
//! - Near-optimal performance for package management use cases
//!
//! The algorithm maintains a "partial solution" of package assignments and
//! "incompatibilities" (constraints that cannot all be true simultaneously).
//! When a conflict is detected, it performs "conflict resolution" to learn
//! a new incompatibility and backtrack to a valid state.

use crate::version::{Stability, Version, VersionConstraint};
use ahash::{AHashMap, AHashSet};
use bumpalo::Bump;
use parking_lot::RwLock;
use petgraph::graph::{DiGraph, NodeIndex};
use smallvec::SmallVec;
use std::collections::VecDeque;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};
use std::sync::Arc;

/// Unique identifier for a package in the resolution.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PackageRef {
    /// Package name (vendor/name format).
    pub name: Arc<str>,
    /// Unique ID for fast comparison.
    id: u64,
}

impl PackageRef {
    /// Create a new package reference.
    pub fn new(name: impl Into<Arc<str>>) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Self {
            name: name.into(),
            id: NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed),
        }
    }

    /// Get the package name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }
}

impl fmt::Display for PackageRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

/// A term in an incompatibility: package + version constraint + positive/negative.
#[derive(Debug, Clone)]
pub struct Term {
    /// The package this term refers to.
    pub package: Arc<PackageRef>,
    /// Version constraint.
    pub constraint: VersionConstraint,
    /// True if positive (package must satisfy constraint),
    /// false if negative (package must NOT satisfy constraint).
    pub positive: bool,
}

impl Term {
    /// Create a positive term.
    #[must_use]
    pub fn positive(package: Arc<PackageRef>, constraint: VersionConstraint) -> Self {
        Self {
            package,
            constraint,
            positive: true,
        }
    }

    /// Create a negative term.
    #[must_use]
    pub fn negative(package: Arc<PackageRef>, constraint: VersionConstraint) -> Self {
        Self {
            package,
            constraint,
            positive: false,
        }
    }

    /// Negate this term.
    #[must_use]
    pub fn negate(&self) -> Self {
        Self {
            package: Arc::clone(&self.package),
            constraint: self.constraint.clone(),
            positive: !self.positive,
        }
    }

    /// Check if a version satisfies this term.
    #[must_use]
    pub fn satisfies(&self, version: &Version) -> bool {
        let matches = self.constraint.matches(version);
        if self.positive {
            matches
        } else {
            !matches
        }
    }

    /// Get the relation to another term for the same package.
    #[must_use]
    pub fn relation_to(&self, other: &Term) -> SetRelation {
        if self.package.name != other.package.name {
            return SetRelation::Disjoint;
        }

        // For simplicity, we check intersection
        let intersection = self.constraint.intersect(&other.constraint);

        if !intersection.is_satisfiable() {
            SetRelation::Disjoint
        } else if self.constraint.is_any() && other.constraint.is_any() {
            SetRelation::Equal
        } else {
            SetRelation::Overlapping
        }
    }
}

impl fmt::Display for Term {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.positive {
            write!(f, "{} {}", self.package, self.constraint)
        } else {
            write!(f, "not({} {})", self.package, self.constraint)
        }
    }
}

/// Relation between two sets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetRelation {
    /// Sets are equal.
    Equal,
    /// First is subset of second.
    Subset,
    /// First is superset of second.
    Superset,
    /// Sets overlap but neither contains the other.
    Overlapping,
    /// Sets are disjoint.
    Disjoint,
}

/// An incompatibility: a set of terms that cannot all be true simultaneously.
#[derive(Debug, Clone)]
pub struct Incompatibility {
    /// Unique ID.
    pub id: u64,
    /// Terms in this incompatibility.
    pub terms: SmallVec<[Term; 2]>,
    /// Cause of this incompatibility.
    pub cause: IncompatibilityCause,
}

impl Incompatibility {
    /// Create a new incompatibility.
    pub fn new(terms: SmallVec<[Term; 2]>, cause: IncompatibilityCause) -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        Self {
            id: NEXT_ID.fetch_add(1, AtomicOrdering::Relaxed),
            terms,
            cause,
        }
    }

    /// Create a root incompatibility (package must exist).
    pub fn root(package: Arc<PackageRef>, constraint: VersionConstraint) -> Self {
        Self::new(
            smallvec::smallvec![Term::negative(package, constraint)],
            IncompatibilityCause::Root,
        )
    }

    /// Create a dependency incompatibility.
    pub fn dependency(
        depender: Arc<PackageRef>,
        depender_constraint: VersionConstraint,
        dependency: Arc<PackageRef>,
        dependency_constraint: VersionConstraint,
    ) -> Self {
        Self::new(
            smallvec::smallvec![
                Term::positive(depender, depender_constraint),
                Term::negative(dependency, dependency_constraint),
            ],
            IncompatibilityCause::Dependency,
        )
    }

    /// Create a "no versions" incompatibility.
    pub fn no_versions(package: Arc<PackageRef>, constraint: VersionConstraint) -> Self {
        Self::new(
            smallvec::smallvec![Term::positive(package, constraint)],
            IncompatibilityCause::NoVersions,
        )
    }

    /// Get packages mentioned in this incompatibility.
    pub fn packages(&self) -> impl Iterator<Item = &Arc<PackageRef>> {
        self.terms.iter().map(|t| &t.package)
    }

    /// Check if this incompatibility is satisfied by the partial solution.
    #[must_use]
    pub fn is_satisfied_by(&self, assignments: &AHashMap<Arc<str>, Assignment>) -> bool {
        self.terms.iter().all(|term| {
            if let Some(assignment) = assignments.get(&term.package.name) {
                term.satisfies(&assignment.version)
            } else {
                // Unassigned packages don't satisfy terms
                false
            }
        })
    }

    /// Find a term that is not satisfied by current assignments.
    #[must_use]
    pub fn find_unsatisfied_term(
        &self,
        assignments: &AHashMap<Arc<str>, Assignment>,
    ) -> Option<&Term> {
        self.terms.iter().find(|term| {
            assignments
                .get(&term.package.name)
                .map_or(true, |a| !term.satisfies(&a.version))
        })
    }
}

impl fmt::Display for Incompatibility {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.terms.is_empty() {
            return write!(f, "impossible");
        }

        match self.cause {
            IncompatibilityCause::Root => {
                write!(f, "{} is required", self.terms[0])
            }
            IncompatibilityCause::Dependency => {
                if self.terms.len() == 2 {
                    write!(f, "{} depends on {}", self.terms[0], self.terms[1])
                } else {
                    write!(f, "{:?}", self.terms)
                }
            }
            IncompatibilityCause::NoVersions => {
                write!(
                    f,
                    "no versions of {} match {}",
                    self.terms[0].package, self.terms[0].constraint
                )
            }
            IncompatibilityCause::ConflictResolution { .. } => {
                write!(f, "conflict: ")?;
                for (i, term) in self.terms.iter().enumerate() {
                    if i > 0 {
                        write!(f, " and ")?;
                    }
                    write!(f, "{term}")?;
                }
                Ok(())
            }
            IncompatibilityCause::PackageNotFound => {
                write!(f, "{} not found", self.terms[0].package)
            }
        }
    }
}

/// Cause of an incompatibility.
#[derive(Debug, Clone)]
pub enum IncompatibilityCause {
    /// Root package dependency.
    Root,
    /// Package dependency.
    Dependency,
    /// No versions match constraint.
    NoVersions,
    /// Package not found in any repository.
    PackageNotFound,
    /// Derived through conflict resolution.
    ConflictResolution {
        /// First incompatibility.
        left: u64,
        /// Second incompatibility.
        right: u64,
    },
}

/// An assignment in the partial solution.
#[derive(Debug, Clone)]
pub struct Assignment {
    /// The assigned version.
    pub version: Version,
    /// Decision level (0 = root, higher = later decisions).
    pub decision_level: u32,
    /// Is this a decision (chosen) or derivation (forced)?
    pub is_decision: bool,
    /// Cause of this assignment (incompatibility ID if derivation).
    pub cause: Option<u64>,
}

/// Conflict resolution result.
#[derive(Debug)]
pub enum ConflictResult {
    /// Resolution found, backtrack to this level.
    Backtrack {
        /// New incompatibility learned.
        incompatibility: Incompatibility,
        /// Decision level to backtrack to.
        level: u32,
    },
    /// No resolution possible (root conflict).
    RootConflict(Incompatibility),
}

/// The partial solution being built during resolution.
#[derive(Debug)]
pub struct PartialSolution {
    /// Package assignments.
    pub assignments: AHashMap<Arc<str>, Assignment>,
    /// Assignment order for backtracking.
    pub assignment_order: Vec<Arc<str>>,
    /// Current decision level.
    pub decision_level: u32,
    /// Positive derivations per package (cached for performance).
    positive_derivations: AHashMap<Arc<str>, SmallVec<[VersionConstraint; 4]>>,
    /// Negative derivations per package.
    negative_derivations: AHashMap<Arc<str>, SmallVec<[VersionConstraint; 4]>>,
}

impl Default for PartialSolution {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialSolution {
    /// Create a new empty partial solution.
    #[must_use]
    pub fn new() -> Self {
        Self {
            assignments: AHashMap::new(),
            assignment_order: Vec::new(),
            decision_level: 0,
            positive_derivations: AHashMap::new(),
            negative_derivations: AHashMap::new(),
        }
    }

    /// Make a decision (assign a version by choice).
    pub fn decide(&mut self, package: Arc<str>, version: Version) {
        self.decision_level += 1;
        self.assignments.insert(
            package.clone(),
            Assignment {
                version,
                decision_level: self.decision_level,
                is_decision: true,
                cause: None,
            },
        );
        self.assignment_order.push(package);
    }

    /// Make a derivation (forced assignment due to incompatibility).
    pub fn derive(&mut self, package: Arc<str>, version: Version, cause: u64) {
        self.assignments.insert(
            package.clone(),
            Assignment {
                version,
                decision_level: self.decision_level,
                is_decision: false,
                cause: Some(cause),
            },
        );
        self.assignment_order.push(package);
    }

    /// Add a positive derivation (package must satisfy this constraint).
    pub fn add_positive(&mut self, package: Arc<str>, constraint: VersionConstraint) {
        self.positive_derivations
            .entry(package)
            .or_default()
            .push(constraint);
    }

    /// Add a negative derivation (package must NOT satisfy this constraint).
    pub fn add_negative(&mut self, package: Arc<str>, constraint: VersionConstraint) {
        self.negative_derivations
            .entry(package)
            .or_default()
            .push(constraint);
    }

    /// Get the effective constraint for a package.
    #[must_use]
    pub fn effective_constraint(&self, package: &str) -> VersionConstraint {
        let positive = self
            .positive_derivations
            .get(package)
            .map(|constraints| {
                constraints
                    .iter()
                    .fold(VersionConstraint::any(), |acc, c| acc.intersect(c))
            })
            .unwrap_or_else(VersionConstraint::any);

        // TODO: properly handle negative constraints
        positive
    }

    /// Backtrack to a decision level.
    pub fn backtrack(&mut self, level: u32) {
        while self.decision_level > level {
            // Remove assignments at current level
            while let Some(pkg) = self.assignment_order.last() {
                let assignment = self.assignments.get(pkg);
                if let Some(a) = assignment {
                    if a.decision_level <= level {
                        break;
                    }
                }
                let pkg = self.assignment_order.pop().expect("non-empty");
                self.assignments.remove(&pkg);
            }
            self.decision_level = level;
        }
    }

    /// Get an unassigned package.
    #[must_use]
    pub fn unassigned_package(&self, packages: &[Arc<PackageRef>]) -> Option<Arc<PackageRef>> {
        packages
            .iter()
            .find(|p| !self.assignments.contains_key(&p.name))
            .cloned()
    }

    /// Check if all packages are assigned.
    #[must_use]
    pub fn is_complete(&self, packages: &[Arc<PackageRef>]) -> bool {
        packages
            .iter()
            .all(|p| self.assignments.contains_key(&p.name))
    }
}

/// Resolution mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ResolutionMode {
    /// Prefer highest versions (default).
    #[default]
    PreferHighest,
    /// Prefer lowest versions (useful for testing).
    PreferLowest,
    /// Prefer stable versions.
    PreferStable,
}

/// Resolution options.
#[derive(Debug, Clone)]
pub struct ResolutionOptions {
    /// Resolution mode.
    pub mode: ResolutionMode,
    /// Minimum stability.
    pub min_stability: Stability,
    /// Include dev dependencies.
    pub include_dev: bool,
    /// Maximum resolution iterations.
    pub max_iterations: u32,
}

impl Default for ResolutionOptions {
    fn default() -> Self {
        Self {
            mode: ResolutionMode::PreferHighest,
            min_stability: Stability::Stable,
            include_dev: true,
            max_iterations: 100_000,
        }
    }
}

/// Package provider trait - abstraction over package sources.
pub trait PackageProvider: Send + Sync {
    /// Get available versions for a package.
    fn versions(&self, package: &str) -> Vec<Version>;

    /// Get dependencies for a specific version of a package.
    fn dependencies(&self, package: &str, version: &Version) -> Vec<(String, VersionConstraint)>;

    /// Check if package exists.
    fn exists(&self, package: &str) -> bool;
}

/// Result of resolution.
#[derive(Debug)]
pub struct ResolutionResult {
    /// Selected versions for each package.
    pub selections: AHashMap<String, Version>,
    /// Dependency graph.
    pub graph: DiGraph<String, ()>,
    /// Node indices for packages.
    pub indices: AHashMap<String, NodeIndex>,
}

/// The PubGrub solver.
pub struct PubGrubSolver<P: PackageProvider> {
    /// Package provider.
    provider: Arc<P>,
    /// Resolution options.
    options: ResolutionOptions,
    /// Known incompatibilities.
    incompatibilities: RwLock<Vec<Arc<Incompatibility>>>,
    /// Package references by name.
    packages: RwLock<AHashMap<Arc<str>, Arc<PackageRef>>>,
    /// Arena allocator for temporary allocations.
    arena: Bump,
}

impl<P: PackageProvider> PubGrubSolver<P> {
    /// Create a new solver.
    #[must_use]
    pub fn new(provider: Arc<P>, options: ResolutionOptions) -> Self {
        Self {
            provider,
            options,
            incompatibilities: RwLock::new(Vec::with_capacity(1024)),
            packages: RwLock::new(AHashMap::new()),
            arena: Bump::with_capacity(64 * 1024),
        }
    }

    /// Get or create a package reference.
    fn get_package(&self, name: &str) -> Arc<PackageRef> {
        {
            let packages = self.packages.read();
            if let Some(pkg) = packages.get(name) {
                return Arc::clone(pkg);
            }
        }

        let mut packages = self.packages.write();
        packages
            .entry(Arc::from(name))
            .or_insert_with(|| Arc::new(PackageRef::new(name)))
            .clone()
    }

    /// Add an incompatibility.
    fn add_incompatibility(&self, incompat: Incompatibility) -> Arc<Incompatibility> {
        let arc = Arc::new(incompat);
        self.incompatibilities.write().push(Arc::clone(&arc));
        arc
    }

    /// Resolve dependencies starting from root requirements.
    pub fn solve(
        &self,
        root_deps: &[(String, VersionConstraint)],
    ) -> Result<ResolutionResult, ResolutionError> {
        let mut solution = PartialSolution::new();
        let mut packages_to_process: VecDeque<Arc<PackageRef>> = VecDeque::new();
        let mut known_packages: Vec<Arc<PackageRef>> = Vec::new();

        // Add root package (virtual)
        let root = self.get_package("root");
        known_packages.push(Arc::clone(&root));
        solution.decide(root.name.clone(), Version::new(0, 0, 0));

        // Add root dependencies as incompatibilities
        for (name, constraint) in root_deps {
            let pkg = self.get_package(name);

            // Check if package exists
            if !self.provider.exists(name) {
                return Err(ResolutionError::PackageNotFound { name: name.clone() });
            }

            // Add incompatibility: root requires this package
            self.add_incompatibility(Incompatibility::root(Arc::clone(&pkg), constraint.clone()));

            known_packages.push(Arc::clone(&pkg));
            packages_to_process.push_back(pkg);
        }

        let mut iterations = 0;

        // Main resolution loop
        loop {
            iterations += 1;
            if iterations > self.options.max_iterations {
                return Err(ResolutionError::TooManyIterations);
            }

            // Unit propagation
            if let Some(conflict) = self.propagate(&mut solution, &known_packages) {
                // Conflict resolution
                match self.resolve_conflict(&solution, &conflict) {
                    ConflictResult::Backtrack {
                        incompatibility,
                        level,
                    } => {
                        self.add_incompatibility(incompatibility);
                        solution.backtrack(level);
                        continue;
                    }
                    ConflictResult::RootConflict(incompat) => {
                        return Err(ResolutionError::Conflict {
                            explanation: self.explain_conflict(&incompat),
                        });
                    }
                }
            }

            // Try to make a decision
            if let Some(pkg) = solution.unassigned_package(&known_packages) {
                let constraint = solution.effective_constraint(&pkg.name);
                let versions = self.provider.versions(&pkg.name);

                // Filter and sort versions
                let mut candidates: Vec<_> = versions
                    .into_iter()
                    .filter(|v| {
                        constraint.matches(v) && v.stability.is_at_least(self.options.min_stability)
                    })
                    .collect();

                match self.options.mode {
                    ResolutionMode::PreferHighest => candidates.sort_by(|a, b| b.cmp(a)),
                    ResolutionMode::PreferLowest => candidates.sort(),
                    ResolutionMode::PreferStable => {
                        candidates.sort_by(|a, b| match (a.is_prerelease(), b.is_prerelease()) {
                            (true, false) => std::cmp::Ordering::Greater,
                            (false, true) => std::cmp::Ordering::Less,
                            _ => b.cmp(a),
                        });
                    }
                }

                if let Some(version) = candidates.first() {
                    // Make decision
                    solution.decide(pkg.name.clone(), version.clone());

                    // Add dependencies
                    let deps = self.provider.dependencies(&pkg.name, version);
                    for (dep_name, dep_constraint) in deps {
                        if is_platform_package(&dep_name) {
                            continue;
                        }

                        let dep_pkg = self.get_package(&dep_name);

                        // Add dependency incompatibility
                        self.add_incompatibility(Incompatibility::dependency(
                            Arc::clone(&pkg),
                            VersionConstraint::exact(version.clone()),
                            Arc::clone(&dep_pkg),
                            dep_constraint,
                        ));

                        if !known_packages.iter().any(|p| p.name == dep_pkg.name) {
                            if !self.provider.exists(&dep_name) {
                                return Err(ResolutionError::PackageNotFound { name: dep_name });
                            }
                            known_packages.push(Arc::clone(&dep_pkg));
                            packages_to_process.push_back(dep_pkg);
                        }
                    }
                } else {
                    // No versions match - add incompatibility
                    let incompat = Incompatibility::no_versions(pkg, constraint);
                    return Err(ResolutionError::NoMatchingVersions {
                        explanation: self.explain_conflict(&incompat),
                    });
                }
            } else {
                // All packages assigned - we're done!
                break;
            }
        }

        // Build result
        self.build_result(&solution, &known_packages)
    }

    /// Perform unit propagation.
    fn propagate(
        &self,
        solution: &mut PartialSolution,
        known_packages: &[Arc<PackageRef>],
    ) -> Option<Incompatibility> {
        let incompats = self.incompatibilities.read();

        loop {
            let mut changed = false;

            for incompat in incompats.iter() {
                // Count satisfied and unsatisfied terms
                let mut satisfied = 0;
                let mut unsatisfied_term = None;

                for term in &incompat.terms {
                    if let Some(assignment) = solution.assignments.get(&term.package.name) {
                        if term.satisfies(&assignment.version) {
                            satisfied += 1;
                        }
                    } else {
                        unsatisfied_term = Some(term);
                    }
                }

                // If all but one term are satisfied, that term must be false
                if satisfied == incompat.terms.len() - 1 {
                    if let Some(term) = unsatisfied_term {
                        // This is a conflict or derivation
                        if solution.assignments.contains_key(&term.package.name) {
                            // Conflict!
                            return Some((**incompat).clone());
                        }

                        // Derive
                        let negated = term.negate();
                        if negated.positive {
                            solution.add_positive(
                                term.package.name.clone(),
                                negated.constraint.clone(),
                            );
                        } else {
                            solution.add_negative(
                                term.package.name.clone(),
                                negated.constraint.clone(),
                            );
                        }
                        changed = true;
                    }
                }

                // If all terms are satisfied, we have a conflict
                if satisfied == incompat.terms.len() {
                    return Some((**incompat).clone());
                }
            }

            if !changed {
                break;
            }
        }

        None
    }

    /// Resolve a conflict using conflict-driven clause learning.
    fn resolve_conflict(
        &self,
        solution: &PartialSolution,
        conflict: &Incompatibility,
    ) -> ConflictResult {
        let mut current = conflict.clone();
        let mut current_level = solution.decision_level;

        loop {
            // Find the term at the current decision level
            let term_at_level = current.terms.iter().find(|t| {
                solution
                    .assignments
                    .get(&t.package.name)
                    .map_or(false, |a| a.decision_level == current_level)
            });

            if let Some(term) = term_at_level {
                if let Some(assignment) = solution.assignments.get(&term.package.name) {
                    if assignment.is_decision {
                        // Decision - we need to backtrack
                        let level = current_level.saturating_sub(1);
                        return ConflictResult::Backtrack {
                            incompatibility: current,
                            level,
                        };
                    }

                    // Derivation - resolve with its cause
                    if let Some(cause_id) = assignment.cause {
                        let incompats = self.incompatibilities.read();
                        if let Some(cause) = incompats.iter().find(|i| i.id == cause_id) {
                            current = self.resolve_incompatibilities(&current, cause);
                            continue;
                        }
                    }
                }
            }

            // Can't find term at current level, try lower level
            if current_level == 0 {
                return ConflictResult::RootConflict(current);
            }
            current_level -= 1;
        }
    }

    /// Resolve two incompatibilities into one.
    fn resolve_incompatibilities(
        &self,
        left: &Incompatibility,
        right: &Incompatibility,
    ) -> Incompatibility {
        let mut terms: SmallVec<[Term; 2]> = SmallVec::new();

        // Find the pivot (package that appears in both)
        let mut pivot = None;
        for lt in &left.terms {
            for rt in &right.terms {
                if lt.package.name == rt.package.name && lt.positive != rt.positive {
                    pivot = Some(lt.package.name.clone());
                    break;
                }
            }
            if pivot.is_some() {
                break;
            }
        }

        // Collect terms, excluding pivot
        for term in &left.terms {
            if Some(&*term.package.name) != pivot.as_ref().map(|s| &**s) {
                terms.push(term.clone());
            }
        }
        for term in &right.terms {
            if Some(&*term.package.name) != pivot.as_ref().map(|s| &**s) {
                if !terms
                    .iter()
                    .any(|t| t.package.name == term.package.name && t.positive == term.positive)
                {
                    terms.push(term.clone());
                }
            }
        }

        Incompatibility::new(
            terms,
            IncompatibilityCause::ConflictResolution {
                left: left.id,
                right: right.id,
            },
        )
    }

    /// Build the final result from the solution.
    fn build_result(
        &self,
        solution: &PartialSolution,
        known_packages: &[Arc<PackageRef>],
    ) -> Result<ResolutionResult, ResolutionError> {
        let mut selections = AHashMap::new();
        let mut graph: DiGraph<String, ()> = DiGraph::new();
        let mut indices: AHashMap<String, NodeIndex> = AHashMap::new();

        // Add nodes
        for pkg in known_packages {
            if pkg.name.as_ref() == "root" {
                continue;
            }

            if let Some(assignment) = solution.assignments.get(&pkg.name) {
                let name = pkg.name.to_string();
                let idx = graph.add_node(name.clone());
                indices.insert(name.clone(), idx);
                selections.insert(name, assignment.version.clone());
            }
        }

        // Add edges
        for pkg in known_packages {
            if pkg.name.as_ref() == "root" {
                continue;
            }

            if let Some(assignment) = solution.assignments.get(&pkg.name) {
                let deps = self.provider.dependencies(&pkg.name, &assignment.version);

                if let Some(&from_idx) = indices.get(pkg.name.as_ref()) {
                    for (dep_name, _) in deps {
                        if let Some(&to_idx) = indices.get(&dep_name) {
                            graph.add_edge(from_idx, to_idx, ());
                        }
                    }
                }
            }
        }

        Ok(ResolutionResult {
            selections,
            graph,
            indices,
        })
    }

    /// Generate human-readable conflict explanation.
    fn explain_conflict(&self, incompat: &Incompatibility) -> String {
        let mut explanation = String::new();
        explanation.push_str("Resolution failed:\n");
        explanation.push_str(&format!("  {incompat}\n"));

        // Trace back causes
        self.trace_cause(&mut explanation, incompat, 1);

        explanation
    }

    fn trace_cause(&self, explanation: &mut String, incompat: &Incompatibility, depth: usize) {
        if depth > 10 {
            explanation.push_str("  ... (truncated)\n");
            return;
        }

        if let IncompatibilityCause::ConflictResolution { left, right } = &incompat.cause {
            let incompats = self.incompatibilities.read();

            if let Some(left_incompat) = incompats.iter().find(|i| i.id == *left) {
                let indent = "  ".repeat(depth);
                explanation.push_str(&format!("{indent}Because {left_incompat}\n"));
                self.trace_cause(explanation, left_incompat, depth + 1);
            }

            if let Some(right_incompat) = incompats.iter().find(|i| i.id == *right) {
                let indent = "  ".repeat(depth);
                explanation.push_str(&format!("{indent}And {right_incompat}\n"));
                self.trace_cause(explanation, right_incompat, depth + 1);
            }
        }
    }
}

/// Resolution error.
#[derive(Debug, Clone)]
pub enum ResolutionError {
    /// Package not found.
    PackageNotFound {
        /// Package name.
        name: String,
    },
    /// No versions match constraint.
    NoMatchingVersions {
        /// Conflict explanation.
        explanation: String,
    },
    /// Conflict detected.
    Conflict {
        /// Conflict explanation.
        explanation: String,
    },
    /// Too many iterations.
    TooManyIterations,
    /// Circular dependency detected.
    CircularDependency {
        /// Packages in the cycle.
        cycle: Vec<String>,
    },
}

impl fmt::Display for ResolutionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PackageNotFound { name } => {
                write!(f, "package '{name}' not found")
            }
            Self::NoMatchingVersions { explanation } => {
                write!(f, "{explanation}")
            }
            Self::Conflict { explanation } => {
                write!(f, "{explanation}")
            }
            Self::TooManyIterations => {
                write!(f, "resolution exceeded maximum iterations")
            }
            Self::CircularDependency { cycle } => {
                write!(f, "circular dependency: {}", cycle.join(" -> "))
            }
        }
    }
}

impl std::error::Error for ResolutionError {}

/// Check if this is a platform package.
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

    struct TestProvider {
        packages: AHashMap<String, Vec<(Version, Vec<(String, VersionConstraint)>)>>,
    }

    impl TestProvider {
        fn new() -> Self {
            Self {
                packages: AHashMap::new(),
            }
        }

        fn add_package(&mut self, name: &str, version: &str, deps: Vec<(&str, &str)>) {
            let v = Version::parse(version).unwrap();
            let d: Vec<_> = deps
                .into_iter()
                .map(|(n, c)| (n.to_string(), VersionConstraint::parse(c).unwrap()))
                .collect();

            self.packages
                .entry(name.to_string())
                .or_default()
                .push((v, d));
        }
    }

    impl PackageProvider for TestProvider {
        fn versions(&self, package: &str) -> Vec<Version> {
            self.packages
                .get(package)
                .map(|versions| versions.iter().map(|(v, _)| v.clone()).collect())
                .unwrap_or_default()
        }

        fn dependencies(
            &self,
            package: &str,
            version: &Version,
        ) -> Vec<(String, VersionConstraint)> {
            self.packages
                .get(package)
                .and_then(|versions| {
                    versions
                        .iter()
                        .find(|(v, _)| v == version)
                        .map(|(_, deps)| deps.clone())
                })
                .unwrap_or_default()
        }

        fn exists(&self, package: &str) -> bool {
            self.packages.contains_key(package)
        }
    }

    #[test]
    fn test_simple_resolution() {
        let mut provider = TestProvider::new();
        provider.add_package("a", "1.0.0", vec![]);
        provider.add_package("a", "2.0.0", vec![]);

        let solver = PubGrubSolver::new(Arc::new(provider), ResolutionOptions::default());

        let result = solver
            .solve(&[("a".to_string(), VersionConstraint::parse("^1.0").unwrap())])
            .unwrap();

        assert_eq!(
            result.selections.get("a"),
            Some(&Version::parse("1.0.0").unwrap())
        );
    }

    #[test]
    fn test_dependency_chain() {
        let mut provider = TestProvider::new();
        provider.add_package("a", "1.0.0", vec![("b", "^1.0")]);
        provider.add_package("b", "1.0.0", vec![("c", "^1.0")]);
        provider.add_package("c", "1.0.0", vec![]);

        let solver = PubGrubSolver::new(Arc::new(provider), ResolutionOptions::default());

        let result = solver
            .solve(&[("a".to_string(), VersionConstraint::parse("^1.0").unwrap())])
            .unwrap();

        assert!(result.selections.contains_key("a"));
        assert!(result.selections.contains_key("b"));
        assert!(result.selections.contains_key("c"));
    }

    #[test]
    fn test_conflict() {
        let mut provider = TestProvider::new();
        provider.add_package("a", "1.0.0", vec![("c", "^1.0")]);
        provider.add_package("b", "1.0.0", vec![("c", "^2.0")]);
        provider.add_package("c", "1.0.0", vec![]);
        provider.add_package("c", "2.0.0", vec![]);

        let solver = PubGrubSolver::new(Arc::new(provider), ResolutionOptions::default());

        let result = solver.solve(&[
            ("a".to_string(), VersionConstraint::parse("^1.0").unwrap()),
            ("b".to_string(), VersionConstraint::parse("^1.0").unwrap()),
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn test_prefer_lowest() {
        let mut provider = TestProvider::new();
        provider.add_package("a", "1.0.0", vec![]);
        provider.add_package("a", "1.1.0", vec![]);
        provider.add_package("a", "1.2.0", vec![]);

        let options = ResolutionOptions {
            mode: ResolutionMode::PreferLowest,
            ..Default::default()
        };

        let solver = PubGrubSolver::new(Arc::new(provider), options);

        let result = solver
            .solve(&[("a".to_string(), VersionConstraint::parse("^1.0").unwrap())])
            .unwrap();

        assert_eq!(
            result.selections.get("a"),
            Some(&Version::parse("1.0.0").unwrap())
        );
    }
}
