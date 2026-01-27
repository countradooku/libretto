//! Platform-specific I/O backend detection and configuration.
//!
//! Provides optimal I/O backend selection:
//! - Linux: io_uring (kernel 5.1+) or epoll
//! - macOS: kqueue
//! - Windows: IOCP

#![allow(unsafe_code)]

use crate::{Os, PlatformError, Result};

/// Available I/O backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IoBackend {
    /// Linux io_uring (kernel 5.1+).
    IoUring,
    /// Windows I/O Completion Ports.
    Iocp,
    /// BSD/macOS kqueue.
    Kqueue,
    /// Linux epoll (fallback).
    Epoll,
    /// POSIX poll (universal fallback).
    Poll,
}

impl IoBackend {
    /// Detect the best available I/O backend for the current platform.
    #[must_use]
    pub fn detect(os: &Os) -> Self {
        match os {
            Os::Linux => Self::detect_linux(),
            Os::Windows => Self::Iocp,
            Os::MacOs => Self::Kqueue,
            Os::Unknown => Self::Poll,
        }
    }

    #[cfg(target_os = "linux")]
    fn detect_linux() -> Self {
        // Check for io_uring support (kernel 5.1+)
        if Self::check_io_uring_support() {
            Self::IoUring
        } else {
            Self::Epoll
        }
    }

    #[cfg(not(target_os = "linux"))]
    fn detect_linux() -> Self {
        Self::Epoll
    }

    #[cfg(target_os = "linux")]
    fn check_io_uring_support() -> bool {
        use std::fs;

        // Check kernel version
        if let Ok(version) = fs::read_to_string("/proc/sys/kernel/osrelease") {
            if let Some((major, minor)) = Self::parse_kernel_version(&version) {
                // io_uring requires kernel 5.1+
                // But for good performance, recommend 5.6+
                if major > 5 || (major == 5 && minor >= 1) {
                    // Also verify io_uring syscalls are available
                    return Self::probe_io_uring();
                }
            }
        }
        false
    }

    #[cfg(target_os = "linux")]
    fn parse_kernel_version(version: &str) -> Option<(u32, u32)> {
        let parts: Vec<&str> = version.trim().split('.').collect();
        let major = parts.first()?.parse().ok()?;
        let minor = parts.get(1)?.split('-').next()?.parse().ok()?;
        Some((major, minor))
    }

    #[cfg(target_os = "linux")]
    fn probe_io_uring() -> bool {
        // Try to probe io_uring availability
        // This checks if the syscalls exist and work
        unsafe {
            // io_uring_setup syscall number on x86_64 is 425
            // On aarch64 it's 425 as well
            let result = libc::syscall(
                libc::SYS_io_uring_setup,
                1u32,                                 // entries
                std::ptr::null_mut::<libc::c_void>(), // params (will fail but that's ok)
            );
            // If syscall returns EFAULT or EINVAL, io_uring exists
            // If it returns ENOSYS, io_uring is not available
            let errno = *libc::__errno_location();
            result != -1 || errno != libc::ENOSYS
        }
    }

    /// Get human-readable name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::IoUring => "io_uring",
            Self::Iocp => "IOCP",
            Self::Kqueue => "kqueue",
            Self::Epoll => "epoll",
            Self::Poll => "poll",
        }
    }

    /// Check if this backend supports zero-copy I/O.
    #[must_use]
    pub const fn supports_zero_copy(self) -> bool {
        matches!(self, Self::IoUring)
    }

    /// Check if this backend supports registered buffers.
    #[must_use]
    pub const fn supports_registered_buffers(self) -> bool {
        matches!(self, Self::IoUring)
    }

    /// Check if this backend supports I/O polling.
    #[must_use]
    pub const fn supports_polling(self) -> bool {
        matches!(
            self,
            Self::IoUring | Self::Iocp | Self::Kqueue | Self::Epoll
        )
    }

    /// Get optimal buffer size for this backend.
    #[must_use]
    pub const fn optimal_buffer_size(self) -> usize {
        match self {
            Self::IoUring => 128 * 1024, // 128KB for io_uring
            Self::Iocp => 64 * 1024,     // 64KB for IOCP
            Self::Kqueue => 64 * 1024,   // 64KB for kqueue
            Self::Epoll => 64 * 1024,    // 64KB for epoll
            Self::Poll => 32 * 1024,     // 32KB for poll
        }
    }

    /// Get optimal submission queue depth for this backend.
    #[must_use]
    pub const fn optimal_queue_depth(self) -> usize {
        match self {
            Self::IoUring => 256, // io_uring handles deep queues well
            Self::Iocp => 64,     // IOCP with reasonable depth
            Self::Kqueue => 64,   // kqueue with reasonable depth
            Self::Epoll => 32,    // epoll with moderate depth
            Self::Poll => 16,     // poll with small depth
        }
    }
}

impl std::fmt::Display for IoBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// I/O configuration optimized for the current platform.
#[derive(Debug, Clone)]
pub struct IoConfig {
    /// Selected I/O backend.
    pub backend: IoBackend,
    /// Read buffer size.
    pub read_buffer_size: usize,
    /// Write buffer size.
    pub write_buffer_size: usize,
    /// Maximum concurrent I/O operations.
    pub max_concurrent_ops: usize,
    /// Queue depth for async I/O.
    pub queue_depth: usize,
    /// Whether to use direct I/O (bypass page cache).
    pub direct_io: bool,
    /// Whether to use memory-mapped I/O for large files.
    pub use_mmap: bool,
    /// Threshold for memory-mapped I/O (bytes).
    pub mmap_threshold: usize,
    /// Whether to pre-allocate files.
    pub preallocate: bool,
    /// File system sync strategy.
    pub sync_strategy: SyncStrategy,
}

impl IoConfig {
    /// Create optimal configuration for the current platform.
    #[must_use]
    pub fn optimal() -> Self {
        let backend = IoBackend::detect(&crate::Os::current());
        Self::for_backend(backend)
    }

    /// Create configuration for a specific backend.
    #[must_use]
    pub fn for_backend(backend: IoBackend) -> Self {
        let cpu_count = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(4);

        let max_concurrent = match backend {
            IoBackend::IoUring => cpu_count * 16,
            IoBackend::Iocp => cpu_count * 8,
            IoBackend::Kqueue | IoBackend::Epoll => cpu_count * 8,
            IoBackend::Poll => cpu_count * 4,
        };

        Self {
            backend,
            read_buffer_size: backend.optimal_buffer_size(),
            write_buffer_size: backend.optimal_buffer_size(),
            max_concurrent_ops: max_concurrent.min(256),
            queue_depth: backend.optimal_queue_depth(),
            direct_io: false, // Usually not needed
            use_mmap: true,
            mmap_threshold: 100 * 1024 * 1024,          // 100MB
            preallocate: backend == IoBackend::IoUring, // io_uring benefits from preallocate
            sync_strategy: SyncStrategy::default_for_backend(backend),
        }
    }

    /// Create configuration optimized for throughput.
    #[must_use]
    pub fn high_throughput() -> Self {
        let mut config = Self::optimal();
        config.read_buffer_size *= 2;
        config.write_buffer_size *= 2;
        config.max_concurrent_ops = config.max_concurrent_ops.saturating_mul(2).min(512);
        config.queue_depth *= 2;
        config.preallocate = true;
        config
    }

    /// Create configuration optimized for low latency.
    #[must_use]
    pub fn low_latency() -> Self {
        let mut config = Self::optimal();
        config.read_buffer_size = 16 * 1024; // Smaller buffers
        config.write_buffer_size = 16 * 1024;
        config.max_concurrent_ops = config.max_concurrent_ops.min(64);
        config.queue_depth = config.queue_depth.min(32);
        config.sync_strategy = SyncStrategy::Immediate;
        config
    }

    /// Builder-style method to set read buffer size.
    #[must_use]
    pub const fn with_read_buffer_size(mut self, size: usize) -> Self {
        self.read_buffer_size = size;
        self
    }

    /// Builder-style method to set write buffer size.
    #[must_use]
    pub const fn with_write_buffer_size(mut self, size: usize) -> Self {
        self.write_buffer_size = size;
        self
    }

    /// Builder-style method to set max concurrent ops.
    #[must_use]
    pub const fn with_max_concurrent_ops(mut self, count: usize) -> Self {
        self.max_concurrent_ops = count;
        self
    }

    /// Builder-style method to enable/disable direct I/O.
    #[must_use]
    pub const fn with_direct_io(mut self, enabled: bool) -> Self {
        self.direct_io = enabled;
        self
    }

    /// Builder-style method to enable/disable mmap.
    #[must_use]
    pub const fn with_mmap(mut self, enabled: bool, threshold: usize) -> Self {
        self.use_mmap = enabled;
        self.mmap_threshold = threshold;
        self
    }
}

impl Default for IoConfig {
    fn default() -> Self {
        Self::optimal()
    }
}

/// File synchronization strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SyncStrategy {
    /// No explicit sync (rely on OS).
    None,
    /// Sync on close.
    OnClose,
    /// Sync after each write.
    Immediate,
    /// Sync periodically.
    Periodic {
        /// Bytes written between syncs.
        bytes_threshold: usize,
    },
    /// Sync on transaction boundaries.
    Transactional,
}

impl SyncStrategy {
    /// Get default strategy for a backend.
    #[must_use]
    pub const fn default_for_backend(backend: IoBackend) -> Self {
        match backend {
            IoBackend::IoUring => Self::Transactional,
            IoBackend::Iocp | IoBackend::Kqueue | IoBackend::Epoll => Self::OnClose,
            IoBackend::Poll => Self::OnClose,
        }
    }
}

/// Async I/O handle abstraction.
#[cfg(feature = "async-io")]
pub mod async_io {
    use super::*;
    use std::path::Path;

    /// Async file operations trait.
    #[allow(async_fn_in_trait)]
    pub trait AsyncFileOps {
        /// Read entire file to bytes.
        async fn read_file(path: &Path) -> Result<Vec<u8>>;

        /// Write bytes to file.
        async fn write_file(path: &Path, contents: &[u8]) -> Result<()>;

        /// Read file with streaming.
        async fn read_file_streaming(
            path: &Path,
            buffer_size: usize,
            callback: impl FnMut(&[u8]) -> Result<()>,
        ) -> Result<u64>;

        /// Copy file with optimal method.
        async fn copy_file(src: &Path, dst: &Path) -> Result<u64>;
    }

    /// Standard tokio-based async file operations.
    #[derive(Debug, Clone, Copy)]
    pub struct TokioFileOps;

    #[cfg(feature = "async-io")]
    impl AsyncFileOps for TokioFileOps {
        async fn read_file(path: &Path) -> Result<Vec<u8>> {
            tokio::fs::read(path)
                .await
                .map_err(|e| PlatformError::io(path, e))
        }

        async fn write_file(path: &Path, contents: &[u8]) -> Result<()> {
            tokio::fs::write(path, contents)
                .await
                .map_err(|e| PlatformError::io(path, e))
        }

        async fn read_file_streaming(
            path: &Path,
            buffer_size: usize,
            mut callback: impl FnMut(&[u8]) -> Result<()>,
        ) -> Result<u64> {
            use tokio::io::AsyncReadExt;

            let mut file = tokio::fs::File::open(path)
                .await
                .map_err(|e| PlatformError::io(path, e))?;

            let mut buffer = vec![0u8; buffer_size];
            let mut total = 0u64;

            loop {
                let n = file
                    .read(&mut buffer)
                    .await
                    .map_err(|e| PlatformError::io(path, e))?;

                if n == 0 {
                    break;
                }

                callback(&buffer[..n])?;
                total += n as u64;
            }

            Ok(total)
        }

        async fn copy_file(src: &Path, dst: &Path) -> Result<u64> {
            tokio::fs::copy(src, dst)
                .await
                .map_err(|e| PlatformError::io(src, e))
        }
    }
}

/// io_uring specific operations (Linux only).
#[cfg(all(target_os = "linux", feature = "io-uring"))]
pub mod io_uring_ops {
    /// io_uring configuration.
    #[derive(Debug, Clone)]
    pub struct IoUringConfig {
        /// Submission queue entries.
        pub sq_entries: u32,
        /// Completion queue entries (usually 2x sq_entries).
        pub cq_entries: u32,
        /// Use SQPOLL for kernel-side submission.
        pub sqpoll: bool,
        /// SQPOLL idle time before sleeping (ms).
        pub sqpoll_idle_ms: u32,
        /// Use IOPOLL for busy-waiting completions.
        pub iopoll: bool,
        /// Number of registered buffers.
        pub registered_buffers: usize,
        /// Size of each registered buffer.
        pub buffer_size: usize,
    }

    impl Default for IoUringConfig {
        fn default() -> Self {
            Self {
                sq_entries: 256,
                cq_entries: 512,
                sqpoll: false, // Requires CAP_SYS_ADMIN
                sqpoll_idle_ms: 1000,
                iopoll: false, // Only for O_DIRECT
                registered_buffers: 32,
                buffer_size: 64 * 1024,
            }
        }
    }

    impl IoUringConfig {
        /// Configuration optimized for high throughput.
        #[must_use]
        pub fn high_throughput() -> Self {
            Self {
                sq_entries: 512,
                cq_entries: 1024,
                sqpoll: false,
                sqpoll_idle_ms: 1000,
                iopoll: false,
                registered_buffers: 64,
                buffer_size: 128 * 1024,
            }
        }

        /// Configuration optimized for low latency.
        #[must_use]
        pub fn low_latency() -> Self {
            Self {
                sq_entries: 64,
                cq_entries: 128,
                sqpoll: false,
                sqpoll_idle_ms: 100,
                iopoll: false,
                registered_buffers: 16,
                buffer_size: 32 * 1024,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_detection() {
        let backend = IoBackend::detect(&Os::current());
        assert!(backend.supports_polling() || backend == IoBackend::Poll);
    }

    #[test]
    fn io_config_optimal() {
        let config = IoConfig::optimal();
        assert!(config.read_buffer_size >= 16 * 1024);
        assert!(config.max_concurrent_ops >= 4);
    }

    #[test]
    fn io_config_high_throughput() {
        let standard = IoConfig::optimal();
        let high = IoConfig::high_throughput();
        assert!(high.read_buffer_size >= standard.read_buffer_size);
        assert!(high.max_concurrent_ops >= standard.max_concurrent_ops);
    }

    #[test]
    fn io_config_low_latency() {
        let standard = IoConfig::optimal();
        let low = IoConfig::low_latency();
        assert!(low.read_buffer_size <= standard.read_buffer_size);
    }

    #[test]
    fn backend_buffer_sizes() {
        assert!(IoBackend::IoUring.optimal_buffer_size() >= IoBackend::Epoll.optimal_buffer_size());
    }

    #[test]
    fn sync_strategy_defaults() {
        let strategy = SyncStrategy::default_for_backend(IoBackend::IoUring);
        assert_eq!(strategy, SyncStrategy::Transactional);
    }
}
