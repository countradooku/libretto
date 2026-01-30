//! Platform-specific error types.

use std::path::PathBuf;
use thiserror::Error;

/// Platform operation errors.
#[derive(Error, Debug)]
pub enum PlatformError {
    /// Unsupported platform.
    #[error("unsupported platform: {os}/{arch}")]
    UnsupportedPlatform {
        /// Operating system.
        os: String,
        /// Architecture.
        arch: String,
    },

    /// I/O error with path context.
    #[error("I/O error at '{path}': {message}")]
    Io {
        /// File path.
        path: PathBuf,
        /// Error message.
        message: String,
    },

    /// Permission denied.
    #[error("permission denied: {path}")]
    PermissionDenied {
        /// File path.
        path: PathBuf,
    },

    /// File not found.
    #[error("file not found: {path}")]
    NotFound {
        /// File path.
        path: PathBuf,
    },

    /// File already exists.
    #[error("file already exists: {path}")]
    AlreadyExists {
        /// File path.
        path: PathBuf,
    },

    /// Invalid path.
    #[error("invalid path: {path} - {reason}")]
    InvalidPath {
        /// File path.
        path: String,
        /// Reason for invalidity.
        reason: String,
    },

    /// Atomic operation failed.
    #[error("atomic operation failed: {operation} - {reason}")]
    AtomicFailed {
        /// Operation name.
        operation: String,
        /// Failure reason.
        reason: String,
    },

    /// Lock acquisition timeout.
    #[error("lock timeout on '{path}' after {timeout_secs}s")]
    LockTimeout {
        /// File path.
        path: PathBuf,
        /// Timeout in seconds.
        timeout_secs: u64,
    },

    /// Signal handling error.
    #[error("signal error: {0}")]
    Signal(String),

    /// Process error.
    #[error("process error: {0}")]
    Process(String),

    /// Process spawn failed.
    #[error("failed to spawn process '{command}': {reason}")]
    SpawnFailed {
        /// Command that failed.
        command: String,
        /// Failure reason.
        reason: String,
    },

    /// Process exited with non-zero status.
    #[error("process exited with code {code}: {stderr}")]
    ProcessFailed {
        /// Exit code.
        code: i32,
        /// Standard error output.
        stderr: String,
    },

    /// Shell not found.
    #[error("shell not found: {shell}")]
    ShellNotFound {
        /// Shell name.
        shell: String,
    },

    /// Shell command error.
    #[error("shell command failed: {0}")]
    ShellCommand(String),

    /// TLS error.
    #[error("TLS error: {0}")]
    Tls(String),

    /// Certificate error.
    #[error("certificate error: {0}")]
    Certificate(String),

    /// SIMD operation error.
    #[error("SIMD error: {0}")]
    Simd(String),

    /// Feature not available.
    #[error("feature not available on this platform: {feature}")]
    FeatureUnavailable {
        /// Feature name.
        feature: String,
    },

    /// Temporary file error.
    #[error("temp file error: {0}")]
    TempFile(String),

    /// Environment variable error.
    #[error("environment variable error: {0}")]
    EnvVar(String),

    /// Symlink error.
    #[error("symlink error at '{path}': {reason}")]
    Symlink {
        /// Path involved.
        path: PathBuf,
        /// Error reason.
        reason: String,
    },

    /// Platform-specific error.
    #[error("{platform} error: {message}")]
    PlatformSpecific {
        /// Platform name.
        platform: String,
        /// Error message.
        message: String,
    },

    /// Internal error (should not happen).
    #[error("internal error: {0}")]
    Internal(String),
}

impl PlatformError {
    /// Create an I/O error with path context.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, err: &std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            message: err.to_string(),
        }
    }

    /// Create a permission denied error.
    #[must_use]
    pub fn permission_denied(path: impl Into<PathBuf>) -> Self {
        Self::PermissionDenied { path: path.into() }
    }

    /// Create a not found error.
    #[must_use]
    pub fn not_found(path: impl Into<PathBuf>) -> Self {
        Self::NotFound { path: path.into() }
    }

    /// Create an already exists error.
    #[must_use]
    pub fn already_exists(path: impl Into<PathBuf>) -> Self {
        Self::AlreadyExists { path: path.into() }
    }

    /// Create an invalid path error.
    #[must_use]
    pub fn invalid_path(path: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::InvalidPath {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a spawn failed error.
    #[must_use]
    pub fn spawn_failed(command: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::SpawnFailed {
            command: command.into(),
            reason: reason.into(),
        }
    }

    /// Create a feature unavailable error.
    #[must_use]
    pub fn feature_unavailable(feature: impl Into<String>) -> Self {
        Self::FeatureUnavailable {
            feature: feature.into(),
        }
    }

    /// Create a platform-specific error.
    #[must_use]
    pub fn platform_specific(platform: impl Into<String>, message: impl Into<String>) -> Self {
        Self::PlatformSpecific {
            platform: platform.into(),
            message: message.into(),
        }
    }

    /// Check if this is a permission error.
    #[must_use]
    pub const fn is_permission_error(&self) -> bool {
        matches!(self, Self::PermissionDenied { .. })
    }

    /// Check if this is a not found error.
    #[must_use]
    pub const fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound { .. })
    }

    /// Check if this is a transient error (retryable).
    #[must_use]
    pub const fn is_transient(&self) -> bool {
        matches!(
            self,
            Self::LockTimeout { .. } | Self::Io { .. } | Self::TempFile(_)
        )
    }
}

impl From<std::io::Error> for PlatformError {
    fn from(err: std::io::Error) -> Self {
        match err.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound {
                path: PathBuf::new(),
            },
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied {
                path: PathBuf::new(),
            },
            std::io::ErrorKind::AlreadyExists => Self::AlreadyExists {
                path: PathBuf::new(),
            },
            _ => Self::Io {
                path: PathBuf::new(),
                message: err.to_string(),
            },
        }
    }
}

/// Result type for platform operations.
pub type Result<T> = std::result::Result<T, PlatformError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = PlatformError::io("/test/path", &std::io::Error::from_raw_os_error(2));
        assert!(err.to_string().contains("/test/path"));
    }

    #[test]
    fn error_classification() {
        let err = PlatformError::permission_denied("/test");
        assert!(err.is_permission_error());
        assert!(!err.is_not_found());

        let err = PlatformError::not_found("/test");
        assert!(err.is_not_found());
        assert!(!err.is_permission_error());
    }

    #[test]
    fn error_transient() {
        let err = PlatformError::LockTimeout {
            path: PathBuf::from("/test"),
            timeout_secs: 30,
        };
        assert!(err.is_transient());

        let err = PlatformError::not_found("/test");
        assert!(!err.is_transient());
    }
}
