//! Error types for the plugin system.

use std::path::PathBuf;
use thiserror::Error;

/// Plugin system errors.
#[derive(Error, Debug)]
pub enum PluginError {
    /// Plugin not found.
    #[error("plugin not found: {0}")]
    NotFound(String),

    /// Plugin already loaded.
    #[error("plugin already loaded: {0}")]
    AlreadyLoaded(String),

    /// Plugin not loaded.
    #[error("plugin not loaded: {0}")]
    NotLoaded(String),

    /// Plugin disabled.
    #[error("plugin is disabled: {0}")]
    Disabled(String),

    /// Failed to load native plugin library.
    #[error("failed to load plugin library at {path}: {message}")]
    LibraryLoad { path: PathBuf, message: String },

    /// Symbol not found in plugin library.
    #[error("symbol '{symbol}' not found in plugin {plugin}")]
    SymbolNotFound { plugin: String, symbol: String },

    /// Invalid plugin metadata.
    #[error("invalid plugin metadata: {0}")]
    InvalidMetadata(String),

    /// Plugin API version mismatch.
    #[error("plugin API version mismatch: required {required}, found {found}")]
    ApiVersionMismatch { required: String, found: String },

    /// Plugin initialization failed.
    #[error("plugin initialization failed: {0}")]
    InitializationFailed(String),

    /// Plugin execution failed.
    #[error("plugin execution failed: {0}")]
    ExecutionFailed(String),

    /// Plugin timeout.
    #[error("plugin timed out after {seconds}s: {plugin}")]
    Timeout { plugin: String, seconds: u64 },

    /// Sandbox violation.
    #[error("sandbox violation in plugin {plugin}: {violation}")]
    SandboxViolation { plugin: String, violation: String },

    /// IPC communication error.
    #[error("IPC error with plugin {plugin}: {message}")]
    Ipc { plugin: String, message: String },

    /// PHP runtime error.
    #[error("PHP error in plugin {plugin}: {message}")]
    PhpRuntime { plugin: String, message: String },

    /// Hot reload is disabled.
    #[error("hot reload is not enabled")]
    HotReloadDisabled,

    /// Invalid operation for plugin type.
    #[error("invalid operation: {0}")]
    InvalidOperation(String),

    /// Discovery error.
    #[error("plugin discovery failed: {0}")]
    Discovery(String),

    /// Hook execution error.
    #[error("hook execution failed: {0}")]
    HookExecution(String),

    /// Configuration error.
    #[error("plugin configuration error: {0}")]
    Configuration(String),

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON parsing error.
    #[error("JSON error: {0}")]
    Json(#[from] sonic_rs::Error),

    /// Semver parsing error.
    #[error("version parsing error: {0}")]
    Version(#[from] semver::Error),

    /// Memory limit exceeded.
    #[error("plugin {plugin} exceeded memory limit of {limit_mb}MB")]
    MemoryLimitExceeded { plugin: String, limit_mb: usize },

    /// Network access violation.
    #[error("plugin {plugin} attempted unauthorized network access to {target}")]
    NetworkViolation { plugin: String, target: String },

    /// Filesystem access violation.
    #[error("plugin {plugin} attempted unauthorized filesystem access to {path}")]
    FilesystemViolation { plugin: String, path: PathBuf },

    /// Plugin crashed.
    #[error("plugin {plugin} crashed: {message}")]
    Crashed { plugin: String, message: String },

    /// Channel send error.
    #[error("event bus send error: {0}")]
    ChannelSend(String),

    /// Channel receive error.
    #[error("event bus receive error: {0}")]
    ChannelReceive(String),
}

impl PluginError {
    /// Create a library load error.
    #[must_use]
    pub fn library_load(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::LibraryLoad {
            path: path.into(),
            message: message.into(),
        }
    }

    /// Create a symbol not found error.
    #[must_use]
    pub fn symbol_not_found(plugin: impl Into<String>, symbol: impl Into<String>) -> Self {
        Self::SymbolNotFound {
            plugin: plugin.into(),
            symbol: symbol.into(),
        }
    }

    /// Create a timeout error.
    #[must_use]
    pub fn timeout(plugin: impl Into<String>, seconds: u64) -> Self {
        Self::Timeout {
            plugin: plugin.into(),
            seconds,
        }
    }

    /// Create a sandbox violation error.
    #[must_use]
    pub fn sandbox_violation(plugin: impl Into<String>, violation: impl Into<String>) -> Self {
        Self::SandboxViolation {
            plugin: plugin.into(),
            violation: violation.into(),
        }
    }

    /// Create an IPC error.
    #[must_use]
    pub fn ipc(plugin: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Ipc {
            plugin: plugin.into(),
            message: message.into(),
        }
    }

    /// Create a PHP runtime error.
    #[must_use]
    pub fn php_runtime(plugin: impl Into<String>, message: impl Into<String>) -> Self {
        Self::PhpRuntime {
            plugin: plugin.into(),
            message: message.into(),
        }
    }

    /// Create a memory limit error.
    #[must_use]
    pub fn memory_limit(plugin: impl Into<String>, limit_mb: usize) -> Self {
        Self::MemoryLimitExceeded {
            plugin: plugin.into(),
            limit_mb,
        }
    }

    /// Create a network violation error.
    #[must_use]
    pub fn network_violation(plugin: impl Into<String>, target: impl Into<String>) -> Self {
        Self::NetworkViolation {
            plugin: plugin.into(),
            target: target.into(),
        }
    }

    /// Create a filesystem violation error.
    #[must_use]
    pub fn filesystem_violation(plugin: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self::FilesystemViolation {
            plugin: plugin.into(),
            path: path.into(),
        }
    }

    /// Create a crash error.
    #[must_use]
    pub fn crashed(plugin: impl Into<String>, message: impl Into<String>) -> Self {
        Self::Crashed {
            plugin: plugin.into(),
            message: message.into(),
        }
    }

    /// Check if this is a timeout error.
    #[must_use]
    pub const fn is_timeout(&self) -> bool {
        matches!(self, Self::Timeout { .. })
    }

    /// Check if this is a sandbox violation.
    #[must_use]
    pub const fn is_sandbox_violation(&self) -> bool {
        matches!(
            self,
            Self::SandboxViolation { .. }
                | Self::NetworkViolation { .. }
                | Self::FilesystemViolation { .. }
                | Self::MemoryLimitExceeded { .. }
        )
    }

    /// Check if this is a recoverable error.
    #[must_use]
    pub const fn is_recoverable(&self) -> bool {
        matches!(
            self,
            Self::Timeout { .. }
                | Self::Ipc { .. }
                | Self::ExecutionFailed(_)
                | Self::HookExecution(_)
        )
    }
}

/// Result type for plugin operations.
pub type Result<T> = std::result::Result<T, PluginError>;

impl From<PluginError> for libretto_core::Error {
    fn from(err: PluginError) -> Self {
        Self::Plugin(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_display() {
        let err = PluginError::NotFound("test-plugin".into());
        assert!(err.to_string().contains("test-plugin"));
    }

    #[test]
    fn error_helpers() {
        let err = PluginError::timeout("test", 30);
        assert!(err.is_timeout());
        assert!(err.is_recoverable());

        let err = PluginError::sandbox_violation("test", "file access");
        assert!(err.is_sandbox_violation());
        assert!(!err.is_recoverable());
    }

    #[test]
    fn error_conversion() {
        let plugin_err = PluginError::NotFound("test".into());
        let core_err: libretto_core::Error = plugin_err.into();
        assert!(matches!(core_err, libretto_core::Error::Plugin(_)));
    }
}
