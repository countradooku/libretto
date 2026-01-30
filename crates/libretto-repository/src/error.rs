//! Repository-specific error types.

use libretto_core::Error as CoreError;
use std::fmt;

/// Repository-specific errors.
#[derive(Debug)]
pub enum RepositoryError {
    /// Package not found in any repository.
    PackageNotFound {
        /// Package name.
        name: String,
        /// Repositories searched.
        repositories: Vec<String>,
    },
    /// Version not found for package.
    VersionNotFound {
        /// Package name.
        name: String,
        /// Version constraint.
        constraint: String,
    },
    /// Network error during fetch.
    Network {
        /// URL that failed.
        url: String,
        /// Error message.
        message: String,
        /// HTTP status code if available.
        status: Option<u16>,
    },
    /// Rate limit exceeded.
    RateLimited {
        /// Repository URL.
        url: String,
        /// Retry after seconds (if known).
        retry_after: Option<u64>,
    },
    /// Authentication required.
    AuthRequired {
        /// Repository URL.
        url: String,
    },
    /// Authentication failed.
    AuthFailed {
        /// Repository URL.
        url: String,
        /// Error message.
        message: String,
    },
    /// Invalid repository configuration.
    InvalidConfig {
        /// Error message.
        message: String,
    },
    /// JSON parsing error.
    ParseError {
        /// URL or source of the JSON.
        source: String,
        /// Error message.
        message: String,
    },
    /// Cache error.
    Cache {
        /// Error message.
        message: String,
    },
    /// Repository is unavailable.
    Unavailable {
        /// Repository URL.
        url: String,
        /// Error message.
        message: String,
    },
    /// Invalid URL.
    InvalidUrl {
        /// The invalid URL.
        url: String,
        /// Error message.
        message: String,
    },
    /// Timeout during operation.
    Timeout {
        /// URL that timed out.
        url: String,
        /// Timeout duration in seconds.
        timeout_secs: u64,
    },
    /// VCS operation failed.
    VcsError {
        /// Repository URL.
        url: String,
        /// Error message.
        message: String,
    },
    /// Path repository error.
    PathError {
        /// Local path.
        path: String,
        /// Error message.
        message: String,
    },
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PackageNotFound { name, repositories } => {
                if repositories.is_empty() {
                    write!(f, "Package '{name}' not found in any repository")
                } else {
                    write!(
                        f,
                        "Package '{name}' not found in repositories: {}",
                        repositories.join(", ")
                    )
                }
            }
            Self::VersionNotFound { name, constraint } => {
                write!(f, "No version of '{name}' matching '{constraint}' found")
            }
            Self::Network {
                url,
                message,
                status,
            } => {
                if let Some(code) = status {
                    write!(f, "HTTP {code} from {url}: {message}")
                } else {
                    write!(f, "Network error fetching {url}: {message}")
                }
            }
            Self::RateLimited { url, retry_after } => {
                if let Some(secs) = retry_after {
                    write!(f, "Rate limited by {url}, retry after {secs}s")
                } else {
                    write!(f, "Rate limited by {url}")
                }
            }
            Self::AuthRequired { url } => {
                write!(f, "Authentication required for {url}")
            }
            Self::AuthFailed { url, message } => {
                write!(f, "Authentication failed for {url}: {message}")
            }
            Self::InvalidConfig { message } => {
                write!(f, "Invalid repository configuration: {message}")
            }
            Self::ParseError { source, message } => {
                write!(f, "Failed to parse response from {source}: {message}")
            }
            Self::Cache { message } => {
                write!(f, "Cache error: {message}")
            }
            Self::Unavailable { url, message } => {
                write!(f, "Repository {url} unavailable: {message}")
            }
            Self::InvalidUrl { url, message } => {
                write!(f, "Invalid URL '{url}': {message}")
            }
            Self::Timeout { url, timeout_secs } => {
                write!(f, "Request to {url} timed out after {timeout_secs}s")
            }
            Self::VcsError { url, message } => {
                write!(f, "VCS error for {url}: {message}")
            }
            Self::PathError { path, message } => {
                write!(f, "Path repository error at {path}: {message}")
            }
        }
    }
}

impl std::error::Error for RepositoryError {}

impl From<RepositoryError> for CoreError {
    fn from(err: RepositoryError) -> Self {
        match err {
            RepositoryError::PackageNotFound { name, .. } => Self::PackageNotFound { name },
            RepositoryError::VersionNotFound { name, constraint } => {
                Self::VersionNotFound { name, constraint }
            }
            RepositoryError::Network { message, .. } => Self::Network(message),
            RepositoryError::RateLimited { url, retry_after } => {
                let msg = if let Some(secs) = retry_after {
                    format!("Rate limited by {url}, retry after {secs}s")
                } else {
                    format!("Rate limited by {url}")
                };
                Self::Network(msg)
            }
            RepositoryError::Timeout { url, timeout_secs } => {
                Self::Network(format!("Request to {url} timed out after {timeout_secs}s"))
            }
            RepositoryError::Unavailable { url, message } => {
                Self::Network(format!("Repository {url} unavailable: {message}"))
            }
            RepositoryError::AuthRequired { url } => {
                Self::Config(format!("Authentication required for {url}"))
            }
            RepositoryError::AuthFailed { message, .. } => {
                Self::Config(format!("Authentication failed: {message}"))
            }
            RepositoryError::InvalidConfig { message } => Self::Config(message),
            RepositoryError::ParseError { message, .. } => Self::InvalidManifest(message),
            RepositoryError::Cache { message } => Self::Cache(message),
            RepositoryError::InvalidUrl { message, .. } => Self::Config(message),
            RepositoryError::VcsError { message, .. } => Self::Vcs(message),
            RepositoryError::PathError { path, message } => Self::Io {
                path: path.into(),
                message,
            },
        }
    }
}

/// Result type for repository operations.
pub type Result<T> = std::result::Result<T, RepositoryError>;
