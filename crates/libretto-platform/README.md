# libretto-platform

Comprehensive cross-platform compatibility layer with platform-specific optimizations for Libretto.

## Overview

This crate provides platform abstractions and optimizations for Libretto:

- **OS Detection**: Runtime detection of operating system and architecture
- **SIMD Support**: Runtime SIMD capability detection (SSE4.2, AVX2, AVX-512, NEON)
- **I/O Backends**: Platform-optimized async I/O (io_uring on Linux, IOCP on Windows, kqueue on macOS)
- **TLS/SSL**: Configurable TLS backends (rustls, native-tls)
- **Shell Integration**: Cross-platform shell detection and command execution
- **File System**: Platform-specific file operations and permissions

## Features

- `async-io` - Async I/O with platform-specific optimizations (default)
- `simd` - SIMD support with runtime detection (default)
- `shell` - Shell integration (default)
- `tls` - TLS/SSL support via rustls (default)
- `native-tls-backend` - Native TLS fallback
- `io-uring` - io_uring support for Linux 5.1+

## Platform Support

| Platform | Architecture | I/O Backend | SIMD |
|----------|-------------|-------------|------|
| Linux | x86_64 | io_uring / epoll | SSE4.2, AVX2, AVX-512 |
| Linux | aarch64 | epoll | NEON |
| macOS | x86_64 | kqueue | SSE4.2, AVX2 |
| macOS | aarch64 | kqueue | NEON |
| Windows | x86_64 | IOCP | SSE4.2, AVX2 |

## Usage

```rust
use libretto_platform::{Platform, SIMDCapability};

// Detect current platform
let platform = Platform::detect();
println!("OS: {:?}", platform.os);
println!("Arch: {:?}", platform.arch);

// Check SIMD capabilities
if SIMDCapability::has_avx2() {
    println!("AVX2 available!");
}
```

## License

Licensed under either of Apache License, Version 2.0 or MIT license at your option.