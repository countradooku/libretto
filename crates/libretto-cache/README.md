# libretto-cache

Advanced multi-tier, content-addressable cache system for the Libretto package manager.

## Overview

This crate provides a high-performance caching layer for Libretto with the following features:

- **Multi-tier caching**: In-memory (moka) + disk-based storage
- **Content-addressable storage**: Files identified by their BLAKE3 hash
- **Zero-copy deserialization**: Using `rkyv` for cached data structures
- **Zstd compression**: Efficient storage with fast decompression
- **Concurrent access**: Thread-safe operations with `DashMap` and `parking_lot`

## Features

- TTL-based cache expiration
- LRU eviction policies
- Atomic file operations
- Memory-mapped file access
- Streaming compression/decompression

## Usage

```rust
use libretto_cache::Cache;

// Create a new cache instance
let cache = Cache::new()?;

// Store data
cache.put("key", &data)?;

// Retrieve data
if let Some(data) = cache.get("key")? {
    // Use cached data
}

// Clear the cache
cache.clear()?;

// Garbage collect expired entries
cache.prune(30)?; // Remove entries older than 30 days
```

## Architecture

```
┌─────────────────────────────────────┐
│          Application Layer          │
├─────────────────────────────────────┤
│         In-Memory Cache (moka)      │
│         - TTL-based expiration      │
│         - LRU eviction              │
├─────────────────────────────────────┤
│         Disk Cache Layer            │
│         - Content-addressable       │
│         - Zstd compressed           │
│         - Memory-mapped access      │
└─────────────────────────────────────┘
```

## Cache Directory Structure

```
~/.libretto/cache/
├── packages/       # Downloaded package archives
├── repo/           # Repository metadata cache
├── vcs/            # VCS (git) cache
└── cas/            # Content-addressable storage
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.