//! Platform detection and OS-specific utilities.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use directories::ProjectDirs;
use libretto_core::{Error, Result};
use once_cell::sync::Lazy;
use std::path::PathBuf;

/// Current platform information.
static PLATFORM: Lazy<Platform> = Lazy::new(Platform::detect);

/// Platform information.
#[derive(Debug, Clone)]
pub struct Platform {
    /// Operating system.
    pub os: Os,
    /// Architecture.
    pub arch: Arch,
    /// Home directory.
    pub home_dir: PathBuf,
    /// Cache directory.
    pub cache_dir: PathBuf,
    /// Config directory.
    pub config_dir: PathBuf,
    /// Data directory.
    pub data_dir: PathBuf,
}

impl Platform {
    /// Detect current platform.
    #[must_use]
    pub fn detect() -> Self {
        let dirs = ProjectDirs::from("", "", "libretto");

        let (home_dir, cache_dir, config_dir, data_dir) = dirs
            .map(|d| {
                (
                    d.cache_dir()
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(std::env::temp_dir),
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

        Self {
            os: Os::current(),
            arch: Arch::current(),
            home_dir,
            cache_dir,
            config_dir,
            data_dir,
        }
    }

    /// Get current platform.
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

    /// Get vendor binary directory.
    #[must_use]
    pub fn vendor_bin_dir(&self, project_root: &std::path::Path) -> PathBuf {
        project_root.join("vendor").join("bin")
    }

    /// Get composer home equivalent.
    #[must_use]
    pub fn composer_home(&self) -> PathBuf {
        std::env::var("COMPOSER_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|_| self.config_dir.clone())
    }

    /// Validate platform is supported.
    ///
    /// # Errors
    /// Returns error if platform is not supported.
    pub fn validate(&self) -> Result<()> {
        match (&self.os, &self.arch) {
            (Os::Linux | Os::MacOs | Os::Windows, Arch::X86_64 | Arch::Aarch64) => Ok(()),
            (os, arch) => Err(Error::UnsupportedPlatform(format!("{os:?}/{arch:?}"))),
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
}

/// CPU architecture.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
    /// x86_64 / AMD64.
    X86_64,
    /// ARM64 / AArch64.
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
}
