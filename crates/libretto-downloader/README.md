# libretto-downloader

Ultra-fast parallel package downloading for the Libretto package manager.

## Overview

This crate provides high-performance package downloading capabilities:

- **HTTP/2 multiplexing**: Multiple requests over single TCP connections
- **Parallel downloads**: Adaptive concurrency based on system resources
- **Streaming extraction**: Extract archives as they download
- **Resume support**: Continue interrupted downloads
- **Checksum verification**: Integrity verification with SHA-1, SHA-256, and BLAKE3

## Features

- **Adaptive concurrency**: Automatically scales based on CPU cores (32-128 concurrent downloads)
- **Connection pooling**: Up to 100 idle connections per host
- **Compression**: Automatic handling of gzip, brotli, deflate, and zstd
- **Rate limiting**: Built-in throttling to avoid overwhelming servers
- **Retry with backoff**: Automatic retry with exponential backoff for transient failures

## Usage

```rust
use libretto_downloader::{Downloader, DownloadConfig, DownloadTask};

// Create a downloader with default configuration
let downloader = Downloader::new(DownloadConfig::default())?;

// Download packages
let tasks = vec![
    DownloadTask::new("symfony/console", "6.0.0", dist_url),
    DownloadTask::new("monolog/monolog", "3.0.0", dist_url),
];

let results = downloader.download_all(tasks, &destination).await?;
```

## HTTP/2 Optimization

The downloader uses aggressive HTTP/2 settings for maximum throughput:

| Setting | Value | Description |
|---------|-------|-------------|
| Stream window | 4 MB | Per-stream flow control window |
| Connection window | 8 MB | Per-connection flow control window |
| Keep-alive interval | 15s | HTTP/2 PING interval |
| Idle connections | 100/host | Connection pool size |

## Archive Support

Supports extraction of:

- **ZIP archives**: Via `async_zip` with deflate, bzip2, zstd, lzma
- **TAR archives**: Via `tokio-tar` with gzip, bzip2, xz, zstd compression
- **Phar archives**: PHP archive format support

## Performance

Compared to Composer's default settings:

| Metric | Composer | Libretto |
|--------|----------|----------|
| Concurrent downloads | 12 | 32-128 (adaptive) |
| HTTP version | HTTP/1.1 | HTTP/2 |
| Connection reuse | Limited | Aggressive pooling |

Expected improvements:
- **Cold cache**: 3-5× faster
- **Warm cache**: 10-100× faster (via hardlinks)

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.