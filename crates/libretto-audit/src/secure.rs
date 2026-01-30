//! Secure operations: HTTPS enforcement, certificate validation, safe paths.

use reqwest::{Certificate, Client, ClientBuilder};
use std::path::{Component, Path, PathBuf};
use thiserror::Error;
use tokio::fs;
use url::Url;

/// Security error.
#[derive(Debug, Error)]
pub enum SecurityError {
    /// Insecure operation attempted.
    #[error("insecure operation: {0}")]
    InsecureOperation(String),

    /// Path traversal attempt.
    #[error("path traversal detected: {0}")]
    PathTraversal(String),

    /// Invalid URL.
    #[error("invalid URL: {0}")]
    InvalidUrl(String),

    /// Certificate error.
    #[error("certificate error: {0}")]
    Certificate(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// HTTP error.
    #[error("HTTP error: {0}")]
    Http(String),
}

/// Result type for security operations.
pub type Result<T> = std::result::Result<T, SecurityError>;

/// Secure HTTP client builder.
#[derive(Debug)]
pub struct SecureClientBuilder {
    https_only: bool,
    custom_ca: Vec<Certificate>,
    timeout: std::time::Duration,
}

impl SecureClientBuilder {
    /// Create new secure client builder.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            https_only: true,
            custom_ca: Vec::new(),
            timeout: std::time::Duration::from_secs(30),
        }
    }

    /// Set HTTPS-only mode (default: true).
    #[must_use]
    pub const fn https_only(mut self, enabled: bool) -> Self {
        self.https_only = enabled;
        self
    }

    /// Add custom CA certificate (PEM format).
    ///
    /// # Errors
    /// Returns error if certificate is invalid.
    pub fn add_ca_certificate(mut self, pem: &[u8]) -> Result<Self> {
        let cert =
            Certificate::from_pem(pem).map_err(|e| SecurityError::Certificate(e.to_string()))?;
        self.custom_ca.push(cert);
        Ok(self)
    }

    /// Load CA certificate from file.
    ///
    /// # Errors
    /// Returns error if file cannot be read or certificate is invalid.
    pub async fn load_ca_file(self, path: impl AsRef<Path>) -> Result<Self> {
        let pem = fs::read(path).await?;
        self.add_ca_certificate(&pem)
    }

    /// Set request timeout.
    #[must_use]
    pub const fn timeout(mut self, timeout: std::time::Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Build HTTP client.
    ///
    /// # Errors
    /// Returns error if client cannot be built.
    pub fn build(self) -> Result<Client> {
        let mut builder = ClientBuilder::new()
            .timeout(self.timeout)
            .use_rustls_tls()
            .https_only(self.https_only)
            .http2_prior_knowledge()
            .gzip(true)
            .brotli(true)
            .deflate(true);

        for cert in self.custom_ca {
            builder = builder.add_root_certificate(cert);
        }

        builder
            .build()
            .map_err(|e| SecurityError::Http(e.to_string()))
    }
}

impl Default for SecureClientBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate URL is HTTPS (or allow HTTP if explicitly configured).
pub fn validate_url(url: &Url, allow_http: bool) -> Result<()> {
    if url.scheme() == "https" {
        return Ok(());
    }

    if url.scheme() == "http" && allow_http {
        return Ok(());
    }

    Err(SecurityError::InsecureOperation(format!(
        "insecure URL scheme: {}",
        url.scheme()
    )))
}

/// Validate and sanitize package name (no path traversal).
pub fn validate_package_name(name: &str) -> Result<()> {
    // Check for path traversal characters
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(SecurityError::PathTraversal(name.to_string()));
    }

    // Check for null bytes
    if name.contains('\0') {
        return Err(SecurityError::PathTraversal(
            "null byte in package name".to_string(),
        ));
    }

    // Check for control characters
    if name.chars().any(char::is_control) {
        return Err(SecurityError::PathTraversal(
            "control character in package name".to_string(),
        ));
    }

    Ok(())
}

/// Sanitize path to prevent directory traversal.
///
/// # Errors
/// Returns error if path contains traversal attempts.
pub fn sanitize_path(path: impl AsRef<Path>, base: impl AsRef<Path>) -> Result<PathBuf> {
    let path = path.as_ref();
    let base = base.as_ref();

    // Resolve path components
    let mut safe_path = base.to_path_buf();

    for component in path.components() {
        match component {
            Component::Normal(part) => {
                // Check for null bytes in path component
                if part.to_string_lossy().contains('\0') {
                    return Err(SecurityError::PathTraversal(
                        "null byte in path".to_string(),
                    ));
                }
                safe_path.push(part);
            }
            Component::CurDir => {
                // Allow current directory references
            }
            Component::ParentDir => {
                // Prevent traversal outside base
                return Err(SecurityError::PathTraversal(format!(
                    "parent directory reference in path: {}",
                    path.display()
                )));
            }
            Component::RootDir | Component::Prefix(_) => {
                // Prevent absolute paths
                return Err(SecurityError::PathTraversal(format!(
                    "absolute path not allowed: {}",
                    path.display()
                )));
            }
        }
    }

    // Verify final path is within base
    let canonical_base = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
    if let Ok(canonical_path) = safe_path.canonicalize()
        && !canonical_path.starts_with(&canonical_base)
    {
        return Err(SecurityError::PathTraversal(format!(
            "path escapes base directory: {}",
            path.display()
        )));
    }

    Ok(safe_path)
}

/// Create secure temporary file with restrictive permissions.
///
/// # Errors
/// Returns error if file cannot be created.
pub fn create_secure_temp() -> Result<tempfile::NamedTempFile> {
    let temp = tempfile::NamedTempFile::new().map_err(SecurityError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o600);
        std::fs::set_permissions(temp.path(), perms)?;
    }

    Ok(temp)
}

/// Create secure temporary directory with restrictive permissions.
///
/// # Errors
/// Returns error if directory cannot be created.
pub fn create_secure_temp_dir() -> Result<tempfile::TempDir> {
    let temp = tempfile::TempDir::new().map_err(SecurityError::Io)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o700);
        std::fs::set_permissions(temp.path(), perms)?;
    }

    Ok(temp)
}

/// Mask sensitive data in strings (credentials, tokens).
#[must_use]
pub fn mask_sensitive(s: &str, show_chars: usize) -> String {
    if s.len() <= show_chars * 2 {
        return "*".repeat(s.len());
    }

    let start = &s[..show_chars];
    let end = &s[s.len() - show_chars..];
    format!("{start}...{end}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_package_name() {
        assert!(validate_package_name("symfony-console").is_ok());
        assert!(validate_package_name("vendor-name").is_ok());

        assert!(validate_package_name("../etc/passwd").is_err());
        assert!(validate_package_name("path/to/file").is_err());
        assert!(validate_package_name("null\0byte").is_err());
    }

    #[test]
    fn test_sanitize_path() {
        let base = PathBuf::from("/safe/base");

        // Safe path
        let result = sanitize_path("subdir/file.txt", &base);
        assert!(result.is_ok());

        // Traversal attempt
        let result = sanitize_path("../etc/passwd", &base);
        assert!(result.is_err());

        // Absolute path
        let result = sanitize_path("/etc/passwd", &base);
        assert!(result.is_err());
    }

    #[test]
    fn test_mask_sensitive() {
        assert_eq!(mask_sensitive("secret123token456", 3), "sec...456");
        assert_eq!(mask_sensitive("short", 3), "*****");
        assert_eq!(mask_sensitive("abcdefgh", 2), "ab...gh");
    }

    #[test]
    fn test_validate_url() {
        let https_url = Url::parse("https://packagist.org").unwrap();
        assert!(validate_url(&https_url, false).is_ok());

        let http_url = Url::parse("http://packagist.org").unwrap();
        assert!(validate_url(&http_url, false).is_err());
        assert!(validate_url(&http_url, true).is_ok());
    }

    #[tokio::test]
    async fn test_secure_client_builder() {
        let client = SecureClientBuilder::new()
            .https_only(true)
            .timeout(std::time::Duration::from_secs(10))
            .build();

        assert!(client.is_ok());
    }

    #[test]
    fn test_secure_temp_creation() {
        let temp = create_secure_temp();
        assert!(temp.is_ok());

        let temp_dir = create_secure_temp_dir();
        assert!(temp_dir.is_ok());
    }
}
