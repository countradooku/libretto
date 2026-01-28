//! Error types for Libretto operations.

use std::path::PathBuf;
use thiserror::Error;

/// Main error type for Libretto.
#[derive(Error, Debug)]
pub enum Error {
    /// Package not found.
    #[error("package '{name}' not found")]
    PackageNotFound {
        /// Package name.
        name: String,
    },

    /// Version not satisfiable.
    #[error("no version of '{name}' satisfies '{constraint}'")]
    VersionNotFound {
        /// Package name.
        name: String,
        /// Version constraint.
        constraint: String,
    },

    /// Dependency resolution failed.
    #[error("resolution failed: {0}")]
    Resolution(String),

    /// Circular dependency.
    #[error("circular dependency: {0}")]
    CircularDependency(String),

    /// Network error.
    #[error("network error: {0}")]
    Network(String),

    /// Invalid manifest.
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    /// JSON error.
    #[error("json error: {0}")]
    Json(#[from] sonic_rs::Error),

    /// IO error.
    #[error("io error at {path}: {message}")]
    Io {
        /// File path.
        path: PathBuf,
        /// Error message.
        message: String,
    },

    /// Cache error.
    #[error("cache error: {0}")]
    Cache(String),

    /// Plugin error.
    #[error("plugin error: {0}")]
    Plugin(String),

    /// Archive error.
    #[error("archive error: {0}")]
    Archive(String),

    /// VCS error.
    #[error("vcs error: {0}")]
    Vcs(String),

    /// Checksum mismatch.
    #[error("checksum mismatch for '{name}': expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Package name.
        name: String,
        /// Expected hash.
        expected: String,
        /// Actual hash.
        actual: String,
    },

    /// Security vulnerability.
    #[error("security: {count} vulnerabilities found")]
    Security {
        /// Number of issues.
        count: usize,
    },

    /// Configuration error.
    #[error("config error: {0}")]
    Config(String),

    /// Platform not supported.
    #[error("unsupported platform: {0}")]
    UnsupportedPlatform(String),

    /// Audit error.
    #[error("audit error: {0}")]
    Audit(String),

    /// Integrity verification error.
    #[error("integrity error: {0}")]
    Integrity(String),

    /// Signature verification error.
    #[error("signature error: {0}")]
    Signature(String),
}

impl Error {
    /// Create an IO error with context.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, err: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            message: err.to_string(),
        }
    }
}

/// Result type for Libretto operations.
pub type Result<T> = std::result::Result<T, Error>;
