//! High-performance dependency resolver for Composer packages.
//!
//! This crate provides a fast, correct dependency resolver using the `PubGrub`
//! algorithm. It supports all Composer version constraint formats and provides
//! clear conflict explanations when resolution fails.
//!
//! # Features
//!
//! - **`PubGrub` algorithm**: Uses `astral-pubgrub` for efficient version solving
//!   with conflict-driven clause learning
//! - **Streaming parallel fetch**: Fetches package metadata in parallel as
//!   dependencies are discovered
//! - **Full Composer compatibility**: Supports all version constraint formats
//!   including exact, range, wildcard, tilde, caret, and OR constraints
//! - **Stability flags**: Supports `@dev`, `@alpha`, `@beta`, `@RC`, `@stable`
//! - **HTTP/2 multiplexing**: Efficient network utilization
//!
//! # Example
//!
//! ```rust,ignore
//! use libretto_resolver::{
//!     Resolver, ResolverConfig, Dependency, PackageName, ComposerConstraint,
//! };
//! use std::sync::Arc;
//!
//! // Create your fetcher (implements PackageFetcher)
//! let fetcher = Arc::new(MyPackagistFetcher::new());
//!
//! // Configure and create resolver
//! let config = ResolverConfig::default();
//! let resolver = Resolver::new(fetcher, config);
//!
//! // Define dependencies
//! let deps = vec![
//!     Dependency::new(
//!         PackageName::parse("symfony/console").unwrap(),
//!         ComposerConstraint::parse("^6.0").unwrap(),
//!     ),
//! ];
//!
//! // Resolve
//! let resolution = resolver.resolve(&deps, &[]).await?;
//! for pkg in &resolution.packages {
//!     println!("{} @ {}", pkg.name, pkg.version);
//! }
//! ```
//!
//! # Architecture
//!
//! The resolver is organized into several modules:
//!
//! - [`resolver`]: Main resolver implementation
//! - [`fetcher`]: Package fetching trait
//! - [`types`]: Resolution result types
//! - [`package`]: Package and dependency types
//! - [`version`]: Version parsing and constraints
//! - [`provider`]: `PubGrub` provider configuration
//! - [`composer`]: composer.json parsing

#![deny(unsafe_code)]
#![warn(missing_docs)]
#![warn(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::module_name_repetitions)]

// Core modules
pub mod composer;
pub mod fetcher;
pub mod index;
pub mod package;
pub mod provider;
pub mod resolver;
pub mod types;
pub mod version;

// Re-export main types
pub use composer::{ComposerManifest, ManifestError};
pub use fetcher::{FetchedPackage, FetchedVersion, PackageFetcher};
pub use index::{CacheSummary, IndexConfig, MemorySource, PackageIndex, PackageSource};
pub use package::{Dependency, PackageEntry, PackageName, PackageNameError, PackageVersion};
pub use provider::{
    ComposerProvider, IncompatibilityReason, ProviderConfig, ProviderError, ResolutionMode,
};
pub use resolver::{Resolver, ResolverConfig, ResolverStats};
pub use types::{Resolution, ResolveError, ResolvedPackage};
pub use version::{
    ComposerConstraint, ComposerVersion, ConstraintParseError, Stability, VersionParseError,
    clear_caches,
};

// Backward compatibility aliases (turbo -> resolver)
pub use fetcher::PackageFetcher as TurboFetcher;
pub use resolver::{
    Resolver as TurboResolver, ResolverConfig as TurboConfig, ResolverStats as TurboStats,
};

/// Module for backward compatibility with turbo naming.
///
/// This module re-exports types with their old "Turbo" names for
/// backward compatibility. New code should use the main exports.
pub mod turbo {
    pub use crate::fetcher::{FetchedPackage, FetchedVersion, PackageFetcher as TurboFetcher};
    pub use crate::resolver::{
        Resolver as TurboResolver, ResolverConfig as TurboConfig, ResolverStats as TurboStats,
    };
}

/// Prelude module for convenient imports.
///
/// Contains the most commonly used types for easy importing.
pub mod prelude {
    pub use crate::{
        ComposerConstraint, ComposerManifest, ComposerVersion, Dependency, FetchedPackage,
        FetchedVersion, PackageFetcher, PackageName, Resolution, ResolutionMode, ResolveError,
        ResolvedPackage, Resolver, ResolverConfig, Stability,
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

        // Create index for testing
        let _index = Arc::new(PackageIndex::new(source));

        // The async resolver is tested in integration tests
        // Here we just verify the index was created
    }

    #[test]
    fn test_version_parsing() {
        assert!(ComposerVersion::parse("1.0.0").is_some());
        assert!(ComposerVersion::parse("v2.1.3").is_some());
        assert!(ComposerVersion::parse("1.0.0-beta").is_some());
        assert!(ComposerVersion::parse("dev-main").is_some());
    }

    #[test]
    fn test_constraint_parsing() {
        assert!(ComposerConstraint::parse("^1.0").is_some());
        assert!(ComposerConstraint::parse("~2.0").is_some());
        assert!(ComposerConstraint::parse(">=1.0 <2.0").is_some());
        assert!(ComposerConstraint::parse("1.0.*").is_some());
        assert!(ComposerConstraint::parse("1.0 || 2.0").is_some());
    }

    #[test]
    fn test_package_name_parsing() {
        assert!(PackageName::parse("vendor/package").is_some());
        assert!(PackageName::parse("symfony/console").is_some());
        assert!(PackageName::parse("invalid").is_none());
    }
}
