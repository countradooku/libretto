# libretto-archive

Archive extraction for the Libretto package manager.

## Overview

This crate provides archive handling capabilities for Libretto:

- **ZIP extraction**: Fast extraction of ZIP archives (most common for Composer packages)
- **TAR extraction**: Support for tar, tar.gz, and tar.bz2 archives
- **Safe extraction**: Protection against path traversal attacks (zip slip)
- **Streaming extraction**: Memory-efficient handling of large archives

## Supported Formats

| Format | Extension | Description |
|--------|-----------|-------------|
| ZIP | `.zip` | Standard ZIP archives (deflate, bzip2, zstd) |
| TAR | `.tar` | Uncompressed tar archives |
| Gzip TAR | `.tar.gz`, `.tgz` | Gzip-compressed tar archives |
| Bzip2 TAR | `.tar.bz2` | Bzip2-compressed tar archives |

## Features

- Parallel extraction using `rayon` for multi-file archives
- Atomic extraction with temporary directories
- Preservation of file permissions (Unix)
- Symbolic link handling with security checks
- Progress callbacks for UI integration

## Usage

```rust
use libretto_archive::{extract_archive, ArchiveFormat};
use std::path::Path;

// Extract a ZIP archive
extract_archive(
    Path::new("package.zip"),
    Path::new("./vendor/vendor/package"),
    ArchiveFormat::Zip,
)?;

// Auto-detect format from extension
extract_archive(
    Path::new("package.tar.gz"),
    Path::new("./vendor/vendor/package"),
    ArchiveFormat::detect("package.tar.gz")?,
)?;
```

## Security

This crate implements several security measures:

1. **Path traversal protection**: Rejects entries with `..` components
2. **Symlink validation**: Ensures symlinks don't escape the extraction directory
3. **File size limits**: Optional limits on extracted file sizes
4. **Permission sanitization**: Strips dangerous permissions on Unix

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.