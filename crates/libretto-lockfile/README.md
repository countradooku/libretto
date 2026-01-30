# libretto-lockfile

Ultra-high-performance composer.lock file management with atomic updates for the Libretto package manager.

## Overview

This crate provides comprehensive `composer.lock` file handling:

- **Atomic updates**: Safe file updates using temporary files and renames
- **Fast parsing**: High-performance JSON parsing with `sonic-rs`
- **Hash verification**: Content hash computation and verification
- **Lock file generation**: Create lock files from resolution results
- **Compatibility**: Full compatibility with Composer's lock file format

## Features

- **Atomic writes**: Prevents corruption during concurrent access or system crashes
- **Content hashing**: BLAKE3/MD5/SHA-256 hashes for integrity verification
- **Memory mapping**: Efficient reading of large lock files via `memmap2`
- **Parallel processing**: Multi-threaded package metadata handling with `rayon`
- **File locking**: Cross-platform file locking via `fs2`

## Lock File Structure

The crate handles the complete Composer lock file structure:

```json
{
    "_readme": [...],
    "content-hash": "abc123...",
    "packages": [...],
    "packages-dev": [...],
    "aliases": [...],
    "minimum-stability": "stable",
    "stability-flags": {...},
    "prefer-stable": true,
    "prefer-lowest": false,
    "platform": {...},
    "platform-dev": {...},
    "plugin-api-version": "2.6.0"
}
```

## Usage

```rust
use libretto_lockfile::{LockFile, LockFileWriter};

// Read an existing lock file
let lock = LockFile::read("composer.lock")?;

// Access packages
for package in &lock.packages {
    println!("{} @ {}", package.name, package.version);
}

// Compute content hash
let hash = lock.compute_content_hash()?;

// Write a lock file atomically
let writer = LockFileWriter::new();
writer.write(&lock, "composer.lock")?;
```

## Content Hash

The content hash is computed from the `composer.json` file to detect when
the lock file needs regeneration:

```rust
use libretto_lockfile::compute_content_hash;

let hash = compute_content_hash("composer.json")?;
if hash != lock.content_hash {
    println!("Lock file is out of date!");
}
```

## Atomic Updates

All write operations are atomic to prevent corruption:

1. Write to a temporary file in the same directory
2. Sync the file to disk (fsync)
3. Atomically rename to the target path

```rust
use libretto_lockfile::LockFileWriter;

let writer = LockFileWriter::new()
    .with_pretty_print(true)
    .with_backup(true);  // Create .bak file

writer.write(&lock, "composer.lock")?;
```

## Performance

Compared to PHP's native JSON handling:

| Operation | Composer (PHP) | Libretto |
|-----------|----------------|----------|
| Parse 1000 packages | ~50ms | ~5ms |
| Write 1000 packages | ~30ms | ~3ms |
| Content hash | ~10ms | ~1ms |

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.