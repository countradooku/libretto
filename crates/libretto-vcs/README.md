# libretto-vcs

High-performance version control operations for the Libretto package manager.

## Overview

This crate provides VCS (Version Control System) operations for Libretto, enabling:

- **Git operations**: Clone, fetch, checkout, and update Git repositories
- **Branch management**: Track dev branches, tags, and specific commits
- **Shallow clones**: Optimized cloning with depth limits for faster downloads
- **Reference caching**: Cache Git references to avoid redundant network requests
- **Parallel operations**: Concurrent VCS operations using `rayon`

## Features

- Pure Rust implementation via `gitoxide` (gix)
- Support for multiple VCS backends (Git primary, with extensibility)
- Smart protocol support (HTTP/HTTPS, SSH, Git protocol)
- Sparse checkouts for large repositories
- Incremental updates with minimal data transfer

## Usage

```rust
use libretto_vcs::{GitRepository, CloneOptions};

// Clone a repository
let options = CloneOptions::new()
    .depth(1)  // Shallow clone
    .branch("main");

let repo = GitRepository::clone(
    "https://github.com/vendor/package.git",
    "/path/to/destination",
    options
).await?;

// Fetch updates
repo.fetch().await?;

// Checkout a specific reference
repo.checkout("v1.0.0").await?;
```

## Supported VCS Types

| Type | Status | Description |
|------|--------|-------------|
| Git | ✅ Full support | Clone, fetch, checkout, tags, branches |
| GitHub API | ✅ Full support | Download release archives directly |
| GitLab API | ✅ Full support | Download release archives directly |
| Bitbucket API | ✅ Full support | Download release archives directly |

## Reference Types

The crate supports various Git reference types as used by Composer:

- **Tags**: `v1.0.0`, `1.0.0`
- **Branches**: `dev-main`, `dev-feature/xyz`
- **Commits**: Full SHA or abbreviated commit hashes
- **Special refs**: `HEAD`, `refs/heads/*`, `refs/tags/*`

## Performance Optimizations

- **Shallow clones**: Fetch only necessary history
- **Reference caching**: Avoid redundant lookups
- **Parallel fetching**: Concurrent operations across repositories
- **Pack file optimization**: Efficient object storage
- **Connection pooling**: Reuse HTTP/SSH connections

## Configuration

```rust
use libretto_vcs::VcsConfig;

let config = VcsConfig {
    max_depth: Some(1),           // Default clone depth
    timeout: Duration::from_secs(300),
    parallel_jobs: 4,
    cache_references: true,
};
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.