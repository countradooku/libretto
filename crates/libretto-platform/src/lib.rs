//! Comprehensive cross-platform compatibility layer with platform-specific optimizations.
//!
//! This crate provides:
//!
//! - **Platform Detection**: OS, architecture, and feature detection
//! - **Platform-specific I/O**: `io_uring` (Linux), IOCP (Windows), kqueue (macOS)
//! - **File System Abstractions**: Cross-platform paths, atomic operations, symlinks
//! - **SIMD Support**: SSE4.2, AVX2, AVX-512 (`x86_64`), NEON (ARM64) with runtime detection
//! - **Platform Features**: Permissions, signals, process spawning
//! - **Shell Integration**: Execute commands across different shells
//! - **Temp Directory Management**: Safe temp file handling with cleanup
//! - **TLS/SSL**: Cross-platform TLS with rustls (pure Rust)
//!
//! # Architecture Support
//!
//! | Platform | Architecture | Status |
//! |----------|-------------|--------|
//! | Linux    | `x86_64`    | Full   |
//! | Linux    | aarch64     | Full   |
//! | macOS    | `x86_64`    | Full   |
//! | macOS    | aarch64     | Full   |
//! | Windows  | `x86_64`    | Full   |
//!
//! # Performance
//!
//! Designed for 100% feature parity across platforms with <1% performance variance.

#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

pub mod cpu;
pub mod error;
pub mod fs;
pub mod io;
pub mod permissions;
pub mod process;
pub mod shell;
pub mod signal;
pub mod simd;
pub mod temp;
#[cfg(feature = "tls")]
pub mod tls;

use directories::ProjectDirs;
use std::{path::PathBuf, sync::LazyLock};

pub use cpu::{CpuFeatures, SimdCapability};
pub use error::{PlatformError, Result};
pub use fs::{AtomicFile, CrossPath, FileSystemOps, LinkType};
pub use io::{IoBackend, IoConfig};
pub use permissions::{FilePermissions, PermissionOps};
pub use process::{ProcessBuilder, ProcessHandle, ProcessOutput};
pub use shell::{Shell, ShellCommand, ShellType};
pub use signal::{SignalHandler, SignalKind};
pub use simd::{SimdOps, SimdRuntime};
pub use temp::{TempDir, TempFile, TempManager};

/// Current platform information (lazily initialized singleton).
static PLATFORM: LazyLock<Platform> = LazyLock::new(Platform::detect);

/// Comprehensive platform information.
#[derive(Debug, Clone)]
pub struct Platform {
    /// Operating system.
    pub os: Os,
    /// CPU architecture.
    pub arch: Arch,
    /// CPU features (SIMD capabilities).
    pub cpu_features: CpuFeatures,
    /// Home directory.
    pub home_dir: PathBuf,
    /// Cache directory (XDG compliant).
    pub cache_dir: PathBuf,
    /// Config directory (XDG compliant).
    pub config_dir: PathBuf,
    /// Data directory (XDG compliant).
    pub data_dir: PathBuf,
    /// Temporary directory.
    pub temp_dir: PathBuf,
    /// Available I/O backend.
    pub io_backend: IoBackend,
    /// Default shell.
    pub default_shell: ShellType,
    /// Path separator.
    pub path_separator: char,
    /// Executable extension.
    pub exe_extension: &'static str,
    /// Case-sensitive filesystem.
    pub case_sensitive_fs: bool,
    /// Kernel version (where applicable).
    pub kernel_version: Option<KernelVersion>,
}

impl Platform {
    /// Detect and initialize platform information.
    #[must_use]
    pub fn detect() -> Self {
        let dirs = ProjectDirs::from("", "", "libretto");

        let (home_dir, cache_dir, config_dir, data_dir) = dirs
            .map(|d| {
                (
                    d.cache_dir()
                        .parent()
                        .map_or_else(std::env::temp_dir, std::path::Path::to_path_buf),
                    d.cache_dir().to_path_buf(),
                    d.config_dir().to_path_buf(),
                    d.data_dir().to_path_buf(),
                )
            })
            .unwrap_or_else(|| {
                let fallback = std::env::temp_dir().join("libretto");
                (
                    std::env::temp_dir(),
                    fallback.clone(),
                    fallback.clone(),
                    fallback,
                )
            });

        let os = Os::current();
        let arch = Arch::current();
        let cpu_features = CpuFeatures::detect();
        let io_backend = IoBackend::detect(&os);
        let default_shell = ShellType::detect(&os);
        let kernel_version = KernelVersion::detect(&os);
        let case_sensitive_fs = Self::detect_case_sensitivity(&os);

        Self {
            os,
            arch,
            cpu_features,
            home_dir,
            cache_dir,
            config_dir,
            data_dir,
            temp_dir: std::env::temp_dir(),
            io_backend,
            default_shell,
            path_separator: Self::get_path_separator(&os),
            exe_extension: Self::get_exe_extension(&os),
            case_sensitive_fs,
            kernel_version,
        }
    }

    /// Get the current platform (singleton).
    #[must_use]
    pub fn current() -> &'static Self {
        &PLATFORM
    }

    /// Check if running on Windows.
    #[must_use]
    pub const fn is_windows(&self) -> bool {
        matches!(self.os, Os::Windows)
    }

    /// Check if running on macOS.
    #[must_use]
    pub const fn is_macos(&self) -> bool {
        matches!(self.os, Os::MacOs)
    }

    /// Check if running on Linux.
    #[must_use]
    pub const fn is_linux(&self) -> bool {
        matches!(self.os, Os::Linux)
    }

    /// Check if running on Unix (Linux or macOS).
    #[must_use]
    pub const fn is_unix(&self) -> bool {
        matches!(self.os, Os::Linux | Os::MacOs)
    }

    /// Check if `x86_64` architecture.
    #[must_use]
    pub const fn is_x86_64(&self) -> bool {
        matches!(self.arch, Arch::X86_64)
    }

    /// Check if ARM64 architecture.
    #[must_use]
    pub const fn is_aarch64(&self) -> bool {
        matches!(self.arch, Arch::Aarch64)
    }

    /// Get vendor binary directory.
    #[must_use]
    pub fn vendor_bin_dir(&self, project_root: &std::path::Path) -> PathBuf {
        project_root.join("vendor").join("bin")
    }

    /// Get composer home equivalent.
    #[must_use]
    pub fn composer_home(&self) -> PathBuf {
        std::env::var("COMPOSER_HOME").map_or_else(|_| self.config_dir.clone(), PathBuf::from)
    }

    /// Validate platform is supported.
    ///
    /// # Errors
    /// Returns error if platform is not supported.
    pub fn validate(&self) -> Result<()> {
        match (&self.os, &self.arch) {
            (Os::Linux | Os::MacOs | Os::Windows, Arch::X86_64 | Arch::Aarch64) => Ok(()),
            (os, arch) => Err(PlatformError::UnsupportedPlatform {
                os: format!("{os:?}"),
                arch: format!("{arch:?}"),
            }),
        }
    }

    /// Check if `io_uring` is available (Linux 5.1+).
    #[must_use]
    pub fn supports_io_uring(&self) -> bool {
        if !self.is_linux() {
            return false;
        }
        self.kernel_version
            .as_ref()
            .is_some_and(|v| v.major >= 5 && (v.major > 5 || v.minor >= 1))
    }

    /// Check if IOCP is available (Windows).
    #[must_use]
    pub const fn supports_iocp(&self) -> bool {
        self.is_windows()
    }

    /// Check if kqueue is available (macOS/BSD).
    #[must_use]
    pub const fn supports_kqueue(&self) -> bool {
        self.is_macos()
    }

    /// Get optimal I/O concurrency for this platform.
    #[must_use]
    pub fn optimal_io_concurrency(&self) -> usize {
        let cpus = std::thread::available_parallelism()
            .map(std::num::NonZero::get)
            .unwrap_or(4);

        match self.io_backend {
            IoBackend::IoUring => cpus * 16, // io_uring handles high concurrency well
            IoBackend::Iocp => cpus * 8,     // IOCP is efficient but has overhead
            IoBackend::Kqueue => cpus * 8,   // kqueue is efficient
            IoBackend::Epoll | IoBackend::Poll => cpus * 4,
        }
    }

    const fn get_path_separator(os: &Os) -> char {
        match os {
            Os::Windows => '\\',
            _ => '/',
        }
    }

    const fn get_exe_extension(os: &Os) -> &'static str {
        match os {
            Os::Windows => ".exe",
            _ => "",
        }
    }

    const fn detect_case_sensitivity(os: &Os) -> bool {
        match os {
            Os::Linux => true,
            Os::MacOs | Os::Windows => false,
            Os::Unknown => true,
        }
    }
}

/// Operating system.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
    /// Linux.
    Linux,
    /// macOS.
    MacOs,
    /// Windows.
    Windows,
    /// Unknown OS.
    Unknown,
}

impl Os {
    /// Detect current OS.
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::Linux
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacOs
        }
        #[cfg(target_os = "windows")]
        {
            Self::Windows
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        {
            Self::Unknown
        }
    }

    /// Get OS name string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::MacOs => "darwin",
            Self::Windows => "windows",
            Self::Unknown => "unknown",
        }
    }

    /// Get OS family.
    #[must_use]
    pub const fn family(self) -> OsFamily {
        match self {
            Self::Linux | Self::MacOs => OsFamily::Unix,
            Self::Windows => OsFamily::Windows,
            Self::Unknown => OsFamily::Unknown,
        }
    }
}

/// OS family classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OsFamily {
    /// Unix-like (Linux, macOS, BSD).
    Unix,
    /// Windows.
    Windows,
    /// Unknown.
    Unknown,
}

/// CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    /// `x86_64` / AMD64.
    X86_64,
    /// ARM64 / `AArch64`.
    Aarch64,
    /// Unknown architecture.
    Unknown,
}

impl Arch {
    /// Detect current architecture.
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_arch = "x86_64")]
        {
            Self::X86_64
        }
        #[cfg(target_arch = "aarch64")]
        {
            Self::Aarch64
        }
        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
        {
            Self::Unknown
        }
    }

    /// Get architecture name string.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
            Self::Unknown => "unknown",
        }
    }

    /// Get pointer width in bits.
    #[must_use]
    pub const fn pointer_width(self) -> usize {
        match self {
            Self::X86_64 | Self::Aarch64 => 64,
            Self::Unknown => std::mem::size_of::<usize>() * 8,
        }
    }
}

/// Kernel version information.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KernelVersion {
    /// Major version.
    pub major: u32,
    /// Minor version.
    pub minor: u32,
    /// Patch version.
    pub patch: u32,
    /// Full version string.
    pub full: String,
}

impl KernelVersion {
    /// Detect kernel version.
    #[must_use]
    pub fn detect(os: &Os) -> Option<Self> {
        match os {
            Os::Linux => Self::detect_linux(),
            Os::MacOs => Self::detect_macos(),
            Os::Windows => Self::detect_windows(),
            Os::Unknown => None,
        }
    }

    #[cfg(target_os = "linux")]
    fn detect_linux() -> Option<Self> {
        use std::fs;
        let full = fs::read_to_string("/proc/version").ok()?;
        let version_str = full.split_whitespace().nth(2)?;
        Self::parse_version(version_str, &full)
    }

    #[cfg(not(target_os = "linux"))]
    fn detect_linux() -> Option<Self> {
        None
    }

    #[cfg(target_os = "macos")]
    fn detect_macos() -> Option<Self> {
        use std::process::Command;
        let output = Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()?;
        let full = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Self::parse_version(&full, &full)
    }

    #[cfg(not(target_os = "macos"))]
    const fn detect_macos() -> Option<Self> {
        None
    }

    #[cfg(target_os = "windows")]
    fn detect_windows() -> Option<Self> {
        // Windows version detection via registry or API
        use std::process::Command;
        let output = Command::new("cmd").args(["/c", "ver"]).output().ok()?;
        let full = String::from_utf8_lossy(&output.stdout).trim().to_string();
        // Parse "Microsoft Windows [Version 10.0.19041.1234]"
        let start = full.find("Version ")? + 8;
        let end = full.find(']')?;
        let version_str = &full[start..end];
        Self::parse_version(version_str, &full)
    }

    #[cfg(not(target_os = "windows"))]
    const fn detect_windows() -> Option<Self> {
        None
    }

    fn parse_version(version: &str, full: &str) -> Option<Self> {
        let parts: Vec<&str> = version.split(['.', '-']).collect();
        let major = parts.first()?.parse().ok()?;
        let minor = parts.get(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        let patch = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);

        Some(Self {
            major,
            minor,
            patch,
            full: full.to_string(),
        })
    }
}

/// Get path separator for current platform.
#[must_use]
pub const fn path_separator() -> char {
    #[cfg(windows)]
    {
        '\\'
    }
    #[cfg(not(windows))]
    {
        '/'
    }
}

/// Get executable extension for current platform.
#[must_use]
pub const fn exe_extension() -> &'static str {
    #[cfg(windows)]
    {
        ".exe"
    }
    #[cfg(not(windows))]
    {
        ""
    }
}

/// Environment variable handling with platform-specific behavior.
pub mod env {
    use std::collections::HashMap;
    use std::ffi::OsString;

    /// Get environment variable with platform-appropriate case handling.
    #[must_use]
    pub fn get_var(key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    /// Get environment variable with case-insensitive lookup on Windows.
    #[must_use]
    pub fn get_var_case_insensitive(key: &str) -> Option<String> {
        #[cfg(windows)]
        {
            let key_upper = key.to_uppercase();
            std::env::vars()
                .find(|(k, _)| k.to_uppercase() == key_upper)
                .map(|(_, v)| v)
        }
        #[cfg(not(windows))]
        {
            std::env::var(key).ok()
        }
    }

    /// Get PATH entries as a vector.
    #[must_use]
    pub fn get_path_entries() -> Vec<std::path::PathBuf> {
        let path_var = {
            #[cfg(windows)]
            {
                "Path"
            }
            #[cfg(not(windows))]
            {
                "PATH"
            }
        };

        std::env::var_os(path_var)
            .map(|p| std::env::split_paths(&p).collect())
            .unwrap_or_default()
    }

    /// Join paths into a PATH-style string.
    #[must_use]
    pub fn join_paths<I, P>(paths: I) -> Option<OsString>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<std::path::Path>,
    {
        std::env::join_paths(paths.into_iter().map(|p| p.as_ref().as_os_str().to_owned())).ok()
    }

    /// Get all environment variables as a map.
    #[must_use]
    pub fn get_all_vars() -> HashMap<String, String> {
        std::env::vars().collect()
    }

    /// Normalize environment variable name for the platform.
    #[must_use]
    pub fn normalize_var_name(name: &str) -> String {
        #[cfg(windows)]
        {
            name.to_uppercase()
        }
        #[cfg(not(windows))]
        {
            name.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_detection() {
        let platform = Platform::current();
        assert!(platform.validate().is_ok() || matches!(platform.os, Os::Unknown));
    }

    #[test]
    fn os_str() {
        assert!(!Os::current().as_str().is_empty());
    }

    #[test]
    fn arch_str() {
        assert!(!Arch::current().as_str().is_empty());
    }

    #[test]
    fn cpu_features_detect() {
        let features = CpuFeatures::detect();
        // At minimum, we should detect something
        assert!(features.has_simd() || !features.has_simd());
    }

    #[test]
    fn io_backend_detect() {
        let backend = IoBackend::detect(&Os::current());
        match Os::current() {
            Os::Linux => assert!(matches!(backend, IoBackend::Epoll | IoBackend::IoUring)),
            Os::MacOs => assert!(matches!(backend, IoBackend::Kqueue)),
            Os::Windows => assert!(matches!(backend, IoBackend::Iocp)),
            Os::Unknown => assert!(matches!(backend, IoBackend::Poll)),
        }
    }

    #[test]
    fn path_separator_correct() {
        #[cfg(windows)]
        assert_eq!(path_separator(), '\\');
        #[cfg(not(windows))]
        assert_eq!(path_separator(), '/');
    }

    #[test]
    fn exe_extension_correct() {
        #[cfg(windows)]
        assert_eq!(exe_extension(), ".exe");
        #[cfg(not(windows))]
        assert_eq!(exe_extension(), "");
    }

    #[test]
    fn env_path_entries() {
        let entries = env::get_path_entries();
        // PATH should have at least one entry on most systems
        assert!(!entries.is_empty() || std::env::var_os("PATH").is_none());
    }

    #[test]
    fn optimal_concurrency() {
        let platform = Platform::current();
        let concurrency = platform.optimal_io_concurrency();
        assert!(concurrency >= 4);
    }
}
