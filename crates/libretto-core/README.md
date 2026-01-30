# libretto-core

Core types and utilities for the Libretto package manager.

## Overview

This crate provides the foundational types and utilities used throughout Libretto:

- **Package types**: `PackageId`, `Package`, `Dependency`, `Version`
- **Error handling**: Unified `Error` type and `Result` alias
- **JSON utilities**: High-performance JSON serialization using `sonic-rs`
- **Content hashing**: BLAKE3-based content-addressable hashing
- **Version constraints**: Composer-compatible version constraint parsing

## Features

- Zero-cost abstractions for package metadata
- SIMD-accelerated JSON parsing via `sonic-rs`
- Fast content hashing with BLAKE3
- Semver-compatible version handling with Composer extensions

## Usage

```rust
use libretto_core::{PackageId, Version, VersionConstraint, Error, Result};

// Parse a package identifier
let id = PackageId::parse("symfony/console").unwrap();
assert_eq!(id.vendor(), "symfony");
assert_eq!(id.name(), "console");

// Parse version constraints
let constraint = VersionConstraint::parse("^5.0");
```

## Global Allocator

This crate sets `mimalloc` as the global allocator for improved performance:

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.