//! PubGrub-based dependency resolution for Libretto.
//!
//! This crate provides a high-performance dependency resolver for Composer packages
//! using the PubGrub algorithm. It supports all Composer version constraint formats
//! and provides clear conflict explanations when resolution fails.
//!
//! # Features
//!
//! - **PubGrub algorithm**: Uses the battle-tested `astral-pubgrub` crate (from the
//!   makers of `uv`) for efficient version solving with conflict-driven clause learning.
//!
//! - **Full Composer compatibility**: Supports all version constraint formats including
//!   exact, range, hyphen, wildcard, tilde, caret, and OR constraints.
//!
//! - **Stability flags**: Supports `@dev`, `@alpha`, `@beta`, `@RC`, `@stable` flags
//!   and `minimum-stability` settings.
//!
//! - **Dev branches**: Full support for `dev-*` branches and `*-dev` suffixes.
//!
//! - **Concurrent caching**: Uses DashMap for lock-free concurrent access to
//!   package metadata and constraint evaluations.
//!
//! - **Parallel prefetching**: Uses Rayon for parallel package metadata fetching.
//!
//! - **Replace and provide**: Tracks virtual packages and replacements.
//!
//! # Example
//!
//! ```rust,ignore
//! use libretto_resolver::{
//!     Resolver, PackageIndex, MemorySource, ResolveOptions,
//!     Dependency, PackageName, ComposerConstraint,
//! };
//! use std::sync::Arc;
//!
//! // Create a package source
//! let source = MemorySource::new();
//! source.add_version("vendor/pkg", "1.0.0", vec![]);
//! source.add_version("vendor/pkg", "2.0.0", vec![]);
//!
//! // Create the index and resolver
//! let index = Arc::new(PackageIndex::new(source));
//! let resolver = Resolver::new(index);
//!
//! // Define root dependencies
//! let deps = vec![
//!     Dependency::new(
//!         PackageName::parse("vendor/pkg").unwrap(),
//!         ComposerConstraint::parse("^1.0").unwrap(),
//!     ),
//! ];
//!
//! // Resolve
//! let resolution = resolver.resolve(&deps, &ResolveOptions::default()).unwrap();
//!
//! for pkg in &resolution.packages {
//!     println!("{} @ {}", pkg.name, pkg.version);
//! }
//! ```
//!
//! # Architecture
//!
//! The resolver is organized into several modules:
//!
//! - [`version`]: Version parsing and constraint handling
//! - [`package`]: Package types and dependency definitions
//! - [`index`]: Package index with concurrent caching
//! - [`provider`]: PubGrub `DependencyProvider` implementation
//! - [`resolver`]: High-level resolver interface
//! - [`composer`]: composer.json parsing

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

pub mod composer;
pub mod fast;
pub mod index;
pub mod package;
pub mod provider;
pub mod remote;
pub mod resolver;
pub mod turbo;
pub mod version;

// Re-export main types
pub use composer::{ComposerManifest, ManifestError};
pub use index::{CacheSummary, IndexConfig, MemorySource, PackageIndex, PackageSource};
pub use package::{Dependency, PackageEntry, PackageName, PackageNameError, PackageVersion};
pub use provider::{
    ComposerProvider, IncompatibilityReason, ProviderConfig, ProviderError, ResolutionMode,
};
pub use resolver::{Resolution, ResolveError, ResolveOptions, ResolvedPackage, Resolver};
pub use version::{
    ComposerConstraint, ComposerVersion, ConstraintParseError, Stability, VersionParseError,
    clear_caches,
};

// Re-export remote fetching types
pub use remote::{AsyncPackageFetcher, RemotePackage, RemotePackageSource, RemoteStats};

// Re-export turbo resolver types
pub use turbo::{TurboConfig, TurboResolver, TurboStats};

// Re-export fast resolver types
pub use fast::{FastConfig, FastResolver, FastStats};

/// Prelude module for convenient imports.
pub mod prelude {
    pub use crate::{
        ComposerConstraint, ComposerManifest, ComposerProvider, ComposerVersion, Dependency,
        MemorySource, PackageIndex, PackageName, ProviderConfig, Resolution, ResolutionMode,
        ResolveError, ResolveOptions, ResolvedPackage, Resolver, Stability,
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn test_full_resolution_flow() {
        // Create a package source with some packages
        let source = MemorySource::new();

        // Add packages
        source.add_version("vendor/a", "1.0.0", vec![]);
        source.add_version("vendor/a", "1.1.0", vec![]);
        source.add_version("vendor/a", "2.0.0", vec![]);

        source.add_version("vendor/b", "1.0.0", vec![("vendor/a", "^1.0")]);
        source.add_version("vendor/b", "2.0.0", vec![("vendor/a", "^2.0")]);

        source.add_version(
            "vendor/c",
            "1.0.0",
            vec![("vendor/a", "^1.0"), ("vendor/b", "^1.0")],
        );

        // Create index and resolver
        let index = Arc::new(PackageIndex::new(source));
        let resolver = Resolver::new(index);

        // Define root dependencies
        let deps = vec![Dependency::new(
            PackageName::parse("vendor/c").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        // Resolve
        let resolution = resolver.resolve(&deps, &ResolveOptions::default()).unwrap();

        // Verify resolution
        assert_eq!(resolution.len(), 3);
        assert!(resolution.contains("vendor/a"));
        assert!(resolution.contains("vendor/b"));
        assert!(resolution.contains("vendor/c"));

        // Verify version selection (should pick highest compatible)
        let a = resolution.get("vendor/a").unwrap();
        assert_eq!(a.version.major, 1);
        assert_eq!(a.version.minor, 1); // 1.1.0

        // Verify installation order (dependencies first)
        let a_pos = resolution
            .packages
            .iter()
            .position(|p| p.name.as_str() == "vendor/a")
            .unwrap();
        let b_pos = resolution
            .packages
            .iter()
            .position(|p| p.name.as_str() == "vendor/b")
            .unwrap();
        let c_pos = resolution
            .packages
            .iter()
            .position(|p| p.name.as_str() == "vendor/c")
            .unwrap();

        assert!(a_pos < c_pos, "vendor/a should come before vendor/c");
        assert!(b_pos < c_pos, "vendor/b should come before vendor/c");
    }

    #[test]
    fn test_conflict_detection() {
        let source = MemorySource::new();

        // Create a conflict: a requires c ^1.0, b requires c ^2.0
        source.add_version("vendor/a", "1.0.0", vec![("vendor/c", "^1.0")]);
        source.add_version("vendor/b", "1.0.0", vec![("vendor/c", "^2.0")]);
        source.add_version("vendor/c", "1.0.0", vec![]);
        source.add_version("vendor/c", "2.0.0", vec![]);

        let index = Arc::new(PackageIndex::new(source));
        let resolver = Resolver::new(index);

        let deps = vec![
            Dependency::new(
                PackageName::parse("vendor/a").unwrap(),
                ComposerConstraint::parse("^1.0").unwrap(),
            ),
            Dependency::new(
                PackageName::parse("vendor/b").unwrap(),
                ComposerConstraint::parse("^1.0").unwrap(),
            ),
        ];

        let result = resolver.resolve(&deps, &ResolveOptions::default());

        // Should fail with a conflict
        assert!(result.is_err());
        if let Err(ResolveError::Conflict { explanation }) = result {
            assert!(!explanation.is_empty());
        }
    }

    #[test]
    fn test_prefer_lowest() {
        let source = MemorySource::new();
        source.add_version("vendor/a", "1.0.0", vec![]);
        source.add_version("vendor/a", "1.5.0", vec![]);
        source.add_version("vendor/a", "2.0.0", vec![]);

        let index = Arc::new(PackageIndex::new(source));
        let resolver = Resolver::new(index);

        let deps = vec![Dependency::new(
            PackageName::parse("vendor/a").unwrap(),
            ComposerConstraint::parse("^1.0").unwrap(),
        )];

        let options = ResolveOptions {
            mode: ResolutionMode::PreferLowest,
            ..Default::default()
        };

        let resolution = resolver.resolve(&deps, &options).unwrap();

        let a = resolution.get("vendor/a").unwrap();
        assert_eq!(a.version.minor, 0); // Should pick 1.0.0
    }

    #[test]
    fn test_stability_filtering() {
        let source = MemorySource::new();
        source.add_version("vendor/a", "1.0.0", vec![]);
        source.add_version("vendor/a", "2.0.0-beta", vec![]);
        source.add_version("vendor/a", "2.0.0-RC1", vec![]);

        let index = Arc::new(PackageIndex::new(source));
        let resolver = Resolver::new(index);

        // With stable minimum stability, should only get 1.0.0
        let deps = vec![Dependency::new(
            PackageName::parse("vendor/a").unwrap(),
            ComposerConstraint::parse(">=1.0").unwrap(),
        )];

        let resolution = resolver.resolve(&deps, &ResolveOptions::default()).unwrap();

        let a = resolution.get("vendor/a").unwrap();
        assert_eq!(a.version.major, 1);

        // With dev minimum stability, should get 2.0.0-RC1
        let options = ResolveOptions {
            min_stability: Stability::Dev,
            ..Default::default()
        };

        let resolution = resolver.resolve(&deps, &options).unwrap();

        let a = resolution.get("vendor/a").unwrap();
        assert_eq!(a.version.major, 2);
    }
}
