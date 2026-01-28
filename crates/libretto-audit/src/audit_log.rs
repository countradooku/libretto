//! Audit logging for security-relevant operations.

use chrono::{DateTime, Utc};
use libretto_core::PackageId;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;

/// Audit log error.
#[derive(Debug, Error)]
pub enum AuditLogError {
    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Serialization error.
    #[error("serialization error: {0}")]
    Serialization(String),
}

/// Result type for audit log operations.
pub type Result<T> = std::result::Result<T, AuditLogError>;

/// Audit log operation type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Operation {
    /// Package installation.
    Install,
    /// Package update.
    Update,
    /// Package removal.
    Remove,
    /// Configuration change.
    ConfigChange,
    /// Credential access.
    CredentialAccess,
    /// Security scan.
    SecurityScan,
    /// Integrity verification.
    IntegrityCheck,
    /// Signature verification.
    SignatureVerification,
}

impl std::fmt::Display for Operation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Install => write!(f, "install"),
            Self::Update => write!(f, "update"),
            Self::Remove => write!(f, "remove"),
            Self::ConfigChange => write!(f, "config_change"),
            Self::CredentialAccess => write!(f, "credential_access"),
            Self::SecurityScan => write!(f, "security_scan"),
            Self::IntegrityCheck => write!(f, "integrity_check"),
            Self::SignatureVerification => write!(f, "signature_verification"),
        }
    }
}

/// Audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Timestamp (UTC).
    pub timestamp: DateTime<Utc>,
    /// Operation type.
    pub operation: Operation,
    /// User who performed operation.
    pub user: String,
    /// Package (if applicable).
    pub package: Option<PackageId>,
    /// Additional details.
    pub details: String,
    /// Success/failure.
    pub success: bool,
}

impl AuditEntry {
    /// Create new audit entry.
    #[must_use]
    pub fn new(operation: Operation, user: impl Into<String>) -> Self {
        Self {
            timestamp: Utc::now(),
            operation,
            user: user.into(),
            package: None,
            details: String::new(),
            success: true,
        }
    }

    /// Set package.
    #[must_use]
    pub fn with_package(mut self, package: PackageId) -> Self {
        self.package = Some(package);
        self
    }

    /// Set details.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = details.into();
        self
    }

    /// Set success status.
    #[must_use]
    pub fn with_success(mut self, success: bool) -> Self {
        self.success = success;
        self
    }

    /// Format as JSON line.
    fn to_json_line(&self) -> Result<String> {
        let json =
            sonic_rs::to_string(self).map_err(|e| AuditLogError::Serialization(e.to_string()))?;
        Ok(format!("{json}\n"))
    }
}

/// Audit logger.
#[derive(Debug)]
pub struct AuditLogger {
    log_path: Option<PathBuf>,
    buffer: Mutex<Vec<AuditEntry>>,
    max_buffer_size: usize,
}

impl AuditLogger {
    /// Create new audit logger (disabled).
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            log_path: None,
            buffer: Mutex::new(Vec::new()),
            max_buffer_size: 100,
        }
    }

    /// Create audit logger with file output.
    #[must_use]
    pub fn with_file(path: impl Into<PathBuf>) -> Self {
        Self {
            log_path: Some(path.into()),
            buffer: Mutex::new(Vec::new()),
            max_buffer_size: 100,
        }
    }

    /// Set max buffer size before flush.
    #[must_use]
    pub fn with_buffer_size(mut self, size: usize) -> Self {
        self.max_buffer_size = size;
        self
    }

    /// Check if logging is enabled.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.log_path.is_some()
    }

    /// Log an entry.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log(&self, entry: AuditEntry) -> Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }

        // Add to buffer
        let should_flush = {
            let mut buffer = self.buffer.lock();
            buffer.push(entry);
            buffer.len() >= self.max_buffer_size
        };

        // Flush if buffer is full
        if should_flush {
            self.flush().await?;
        }

        Ok(())
    }

    /// Log package installation.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log_install(
        &self,
        package: PackageId,
        version: &str,
        success: bool,
    ) -> Result<()> {
        let entry = AuditEntry::new(Operation::Install, Self::current_user())
            .with_package(package)
            .with_details(format!("version {version}"))
            .with_success(success);

        self.log(entry).await
    }

    /// Log package update.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log_update(
        &self,
        package: PackageId,
        from_version: &str,
        to_version: &str,
        success: bool,
    ) -> Result<()> {
        let entry = AuditEntry::new(Operation::Update, Self::current_user())
            .with_package(package)
            .with_details(format!("{from_version} -> {to_version}"))
            .with_success(success);

        self.log(entry).await
    }

    /// Log package removal.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log_remove(&self, package: PackageId, version: &str) -> Result<()> {
        let entry = AuditEntry::new(Operation::Remove, Self::current_user())
            .with_package(package)
            .with_details(format!("version {version}"));

        self.log(entry).await
    }

    /// Log configuration change.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log_config_change(
        &self,
        key: &str,
        old_value: &str,
        new_value: &str,
    ) -> Result<()> {
        let entry = AuditEntry::new(Operation::ConfigChange, Self::current_user())
            .with_details(format!("{key}: {old_value} -> {new_value}"));

        self.log(entry).await
    }

    /// Log credential access.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log_credential_access(&self, host: &str) -> Result<()> {
        let entry = AuditEntry::new(Operation::CredentialAccess, Self::current_user())
            .with_details(host.to_string());

        self.log(entry).await
    }

    /// Log security scan.
    ///
    /// # Errors
    /// Returns error if logging fails.
    pub async fn log_security_scan(
        &self,
        packages_scanned: usize,
        vulnerabilities_found: usize,
    ) -> Result<()> {
        let entry = AuditEntry::new(Operation::SecurityScan, Self::current_user()).with_details(
            format!("scanned {packages_scanned}, found {vulnerabilities_found} vulnerabilities"),
        );

        self.log(entry).await
    }

    /// Flush buffered entries to file.
    ///
    /// # Errors
    /// Returns error if flush fails.
    pub async fn flush(&self) -> Result<()> {
        let entries = {
            let mut buffer = self.buffer.lock();
            std::mem::take(&mut *buffer)
        };

        if entries.is_empty() {
            return Ok(());
        }

        if let Some(ref path) = self.log_path {
            // Ensure parent directory exists
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }

            // Open file in append mode
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .await?;

            // Write entries
            for entry in entries {
                let line = entry.to_json_line()?;
                file.write_all(line.as_bytes()).await?;
            }

            file.flush().await?;
        }

        Ok(())
    }

    /// Read audit log entries from file.
    ///
    /// # Errors
    /// Returns error if reading fails.
    pub async fn read_entries(&self) -> Result<Vec<AuditEntry>> {
        let Some(ref path) = self.log_path else {
            return Ok(Vec::new());
        };

        if !path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(path).await?;
        let entries: Vec<AuditEntry> = content
            .lines()
            .filter_map(|line| sonic_rs::from_str(line).ok())
            .collect();

        Ok(entries)
    }

    /// Get current system user.
    fn current_user() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "unknown".to_string())
    }
}

impl Drop for AuditLogger {
    fn drop(&mut self) {
        // Best effort flush on drop (blocking)
        if !self.buffer.lock().is_empty() {
            if let Some(ref path) = self.log_path {
                let _ = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)
                    .and_then(|mut file| {
                        use std::io::Write;
                        let buffer = self.buffer.lock();
                        for entry in buffer.iter() {
                            if let Ok(line) = entry.to_json_line() {
                                let _ = file.write_all(line.as_bytes());
                            }
                        }
                        file.flush()
                    });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_audit_entry_creation() {
        let entry = AuditEntry::new(Operation::Install, "testuser")
            .with_package(PackageId::parse("vendor/package").unwrap())
            .with_details("test install")
            .with_success(true);

        assert_eq!(entry.operation, Operation::Install);
        assert_eq!(entry.user, "testuser");
        assert!(entry.success);
    }

    #[tokio::test]
    async fn test_audit_logger_disabled() {
        let logger = AuditLogger::disabled();
        assert!(!logger.is_enabled());

        let result = logger
            .log(AuditEntry::new(Operation::Install, "user"))
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_audit_logger_with_file() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let logger = AuditLogger::with_file(&log_path);
        assert!(logger.is_enabled());

        logger
            .log_install(PackageId::parse("vendor/package").unwrap(), "1.0.0", true)
            .await
            .unwrap();

        logger.flush().await.unwrap();

        assert!(log_path.exists());

        let entries = logger.read_entries().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].operation, Operation::Install);
    }

    #[tokio::test]
    async fn test_audit_logger_buffer() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("audit.log");

        let logger = AuditLogger::with_file(&log_path).with_buffer_size(2);

        // Log 3 entries (should auto-flush after 2)
        for i in 0..3 {
            logger
                .log_install(
                    PackageId::parse("vendor/package").unwrap(),
                    &format!("1.0.{i}"),
                    true,
                )
                .await
                .unwrap();
        }

        // Should have flushed 2, 1 remaining in buffer
        let entries = logger.read_entries().await.unwrap();
        assert!(entries.len() >= 2);
    }
}
