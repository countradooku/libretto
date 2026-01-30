# libretto-resolver

PubGrub-based dependency resolution for the Libretto package manager.

## Overview

This crate provides high-performance dependency resolution using the PubGrub algorithm
(via `astral-pubgrub` from the uv project). It supports all Composer version constraint
formats and provides clear conflict explanations when resolution fails.

## Features

- **PubGrub algorithm**: State-of-the-art version solving with conflict-driven clause learning
- **Streaming parallel fetch**: Fetches package metadata in parallel as dependencies are discovered
- **Full Composer compatibility**: Supports all version constraint formats
- **Stability flags**: `@dev`, `@alpha`, `@beta`, `@RC`, `@stable`
- **HTTP/2 multiplexing**: Efficient network utilization
- **Replace/Provide**: Full support for package replacements and virtual packages

## Version Constraint Support

| Format | Example | Description |
|--------|---------|-------------|
| Exact | `1.0.0` | Exact version match |
| Range | `>=1.0 <2.0` | Version range |
| Wildcard | `1.0.*` | Wildcard matching |
| Tilde | `~1.2.3` | Next significant release |
| Caret | `^1.0` | Semver-compatible |
| OR | `^1.0 \|\| ^2.0` | Multiple constraints |
| Hyphen | `1.0 - 2.0` | Inclusive range |
| Stability | `>=1.0@dev` | With stability flag |
| Dev branch | `dev-main` | Development branch |

## Usage

```rust
use libretto_resolver::{
    Resolver, ResolverConfig, Dependency, PackageName, ComposerConstraint,
    PackageFetcher, ResolutionMode,
};
use std::sync::Arc;

// Create your fetcher (implements PackageFetcher)
let fetcher = Arc::new(MyPackagistFetcher::new());

// Configure the resolver
let config = ResolverConfig {
    max_concurrent: 32,
    mode: ResolutionMode::PreferStable,
    ..Default::default()
};

// Create the resolver
let resolver = Resolver::new(fetcher, config);

// Define dependencies
let deps = vec![
    Dependency::new(
        PackageName::parse("symfony/console").unwrap(),
        ComposerConstraint::parse("^6.0").unwrap(),
    ),
];

// Resolve
let resolution = resolver.resolve(&deps, &[]).await?;

// Print results
for pkg in &resolution.packages {
    println!("{} @ {}", pkg.name, pkg.version);
}
```

## Resolution Modes

- `PreferStable` (default): Prefer stable versions over pre-release
- `PreferLowest`: Choose the lowest version satisfying constraints (useful for testing)
- `PreferLatest`: Always choose the latest version

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                    Resolver                              │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────────┐    ┌─────────────────────────────┐ │
│  │ Package Fetcher │───▶│ Streaming Parallel Fetch    │ │
│  │ (HTTP/2)        │    │ - Request deduplication     │ │
│  └─────────────────┘    │ - Prefetch dependencies     │ │
│                         └─────────────────────────────┘ │
│                                    │                     │
│                                    ▼                     │
│  ┌─────────────────────────────────────────────────────┐│
│  │              PubGrub Solver                         ││
│  │  - Conflict-driven clause learning                  ││
│  │  - Incompatibility tracking                         ││
│  │  - Version prioritization                           ││
│  └─────────────────────────────────────────────────────┘│
│                                    │                     │
│                                    ▼                     │
│  ┌─────────────────────────────────────────────────────┐│
│  │              Resolution                             ││
│  │  - Topologically sorted packages                    ││
│  │  - Dev dependency separation                        ││
│  │  - Complete metadata                                ││
│  └─────────────────────────────────────────────────────┘│
└─────────────────────────────────────────────────────────┘
```

## Performance

The resolver uses several optimizations:

- **Streaming fetch**: Process packages as they arrive, don't wait for batches
- **Parallel prefetch**: Start fetching dependencies before parent completes
- **Request deduplication**: Never fetch the same package twice
- **Version caching**: Cache parsed version constraints
- **SmallVec**: Reduce allocations for small dependency lists

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.