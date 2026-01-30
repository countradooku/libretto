# libretto-repository

Package repository clients for the Libretto package manager.

## Overview

This crate provides clients for accessing package repositories:

- **Packagist**: The official PHP package repository
- **Private repositories**: Support for Satis, Private Packagist, and custom repositories
- **VCS repositories**: Direct access to Git, Mercurial, and SVN repositories
- **Path repositories**: Local filesystem package sources

## Features

- **HTTP/2 multiplexing**: Efficient parallel metadata fetching
- **Rate limiting**: Built-in rate limiting with `governor`
- **Retry with backoff**: Automatic retry with exponential backoff using `backon`
- **Concurrent caching**: Thread-safe caching with `dashmap` and `moka`
- **Streaming responses**: Memory-efficient handling of large repository metadata

## Usage

```rust
use libretto_repository::{PackagistClient, RepositoryConfig};

// Create a Packagist client
let client = PackagistClient::new(RepositoryConfig::default())?;

// Fetch package metadata
let package = client.get_package("symfony/console").await?;

// Search for packages
let results = client.search("http client").await?;
```

## Repository Types

### Packagist (composer)

```json
{
    "type": "composer",
    "url": "https://packagist.org"
}
```

### Private Packagist

```json
{
    "type": "composer",
    "url": "https://repo.packagist.com/my-company/"
}
```

### VCS (Git, etc.)

```json
{
    "type": "vcs",
    "url": "https://github.com/vendor/package.git"
}
```

### Path (local)

```json
{
    "type": "path",
    "url": "../my-local-package"
}
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.