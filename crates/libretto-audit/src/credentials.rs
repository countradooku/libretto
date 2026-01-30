//! Secure credential management with keyring storage and Git credential helper support.

use dialoguer::{Input, Password};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::{Command, Stdio};
use thiserror::Error;
use url::Url;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// Credential error.
#[derive(Debug, Error)]
pub enum CredentialError {
    /// Keyring error.
    #[error("keyring error: {0}")]
    Keyring(String),

    /// Invalid credential format.
    #[error("invalid credential format: {0}")]
    InvalidFormat(String),

    /// User cancelled input.
    #[error("user cancelled credential input")]
    Cancelled,

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result type for credential operations.
pub type Result<T> = std::result::Result<T, CredentialError>;

/// Credential type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CredentialType {
    /// HTTP Basic authentication.
    Basic,
    /// Bearer token.
    Bearer,
    /// API key.
    ApiKey,
}

/// Secure credential storage (zeroized on drop).
#[derive(Debug, Clone, Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Credential {
    /// Credential type.
    #[zeroize(skip)]
    pub cred_type: CredentialType,
    /// Username (for Basic auth).
    pub username: Option<String>,
    /// Password/token (sensitive).
    pub secret: String,
}

impl Credential {
    /// Create Basic auth credential.
    #[must_use]
    pub fn basic(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            cred_type: CredentialType::Basic,
            username: Some(username.into()),
            secret: password.into(),
        }
    }

    /// Create Bearer token credential.
    #[must_use]
    pub fn bearer(token: impl Into<String>) -> Self {
        Self {
            cred_type: CredentialType::Bearer,
            username: None,
            secret: token.into(),
        }
    }

    /// Create API key credential.
    #[must_use]
    pub fn api_key(key: impl Into<String>) -> Self {
        Self {
            cred_type: CredentialType::ApiKey,
            username: None,
            secret: key.into(),
        }
    }

    /// Get HTTP Authorization header value.
    #[must_use]
    pub fn authorization_header(&self) -> String {
        match self.cred_type {
            CredentialType::Basic => {
                let username = self.username.as_deref().unwrap_or("");
                let encoded = base64::Engine::encode(
                    &base64::engine::general_purpose::STANDARD,
                    format!("{}:{}", username, self.secret),
                );
                format!("Basic {encoded}")
            }
            CredentialType::Bearer => format!("Bearer {}", self.secret),
            CredentialType::ApiKey => self.secret.clone(),
        }
    }

    /// Mask secret for display.
    #[must_use]
    pub fn masked(&self) -> String {
        let masked_secret = if self.secret.len() > 8 {
            format!(
                "{}...{}",
                &self.secret[..3],
                &self.secret[self.secret.len() - 3..]
            )
        } else {
            "*".repeat(self.secret.len())
        };

        match self.cred_type {
            CredentialType::Basic => {
                format!(
                    "Basic {}:{}",
                    self.username.as_deref().unwrap_or(""),
                    masked_secret
                )
            }
            CredentialType::Bearer => format!("Bearer {masked_secret}"),
            CredentialType::ApiKey => format!("ApiKey {masked_secret}"),
        }
    }
}

/// Credential manager using system keyring.
#[derive(Debug)]
pub struct CredentialManager {
    service_name: String,
    cache: parking_lot::RwLock<HashMap<String, Credential>>,
}

impl CredentialManager {
    /// Create new credential manager.
    #[must_use]
    pub fn new(service_name: impl Into<String>) -> Self {
        Self {
            service_name: service_name.into(),
            cache: parking_lot::RwLock::new(HashMap::new()),
        }
    }

    /// Get keyring entry for host.
    fn entry(&self, host: &str) -> Entry {
        Entry::new(&self.service_name, host).expect("keyring entry")
    }

    /// Store credential for host.
    ///
    /// # Errors
    /// Returns error if storage fails.
    pub fn store(&self, host: &str, credential: &Credential) -> Result<()> {
        let json = sonic_rs::to_string(credential)
            .map_err(|e| CredentialError::InvalidFormat(e.to_string()))?;

        self.entry(host)
            .set_password(&json)
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;

        // Update cache
        self.cache
            .write()
            .insert(host.to_string(), credential.clone());

        Ok(())
    }

    /// Retrieve credential for host.
    ///
    /// # Errors
    /// Returns error if retrieval fails or credential not found.
    pub fn retrieve(&self, host: &str) -> Result<Credential> {
        // Check cache first
        if let Some(cred) = self.cache.read().get(host) {
            return Ok(cred.clone());
        }

        // Try keyring
        let json = self
            .entry(host)
            .get_password()
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;

        let credential: Credential =
            sonic_rs::from_str(&json).map_err(|e| CredentialError::InvalidFormat(e.to_string()))?;

        // Update cache
        self.cache
            .write()
            .insert(host.to_string(), credential.clone());

        Ok(credential)
    }

    /// Delete credential for host.
    ///
    /// # Errors
    /// Returns error if deletion fails.
    pub fn delete(&self, host: &str) -> Result<()> {
        self.entry(host)
            .delete_password()
            .map_err(|e| CredentialError::Keyring(e.to_string()))?;

        // Remove from cache
        self.cache.write().remove(host);

        Ok(())
    }

    /// Check if credential exists for host.
    #[must_use]
    pub fn has_credential(&self, host: &str) -> bool {
        if self.cache.read().contains_key(host) {
            return true;
        }

        self.entry(host).get_password().is_ok()
    }

    /// Prompt user for credential interactively.
    ///
    /// # Errors
    /// Returns error if input fails.
    pub fn prompt_for_credential(&self, host: &str, store: bool) -> Result<Credential> {
        println!("Authentication required for {host}");

        let cred_type: String = Input::new()
            .with_prompt("Credential type (basic/bearer/apikey)")
            .default("basic".to_string())
            .interact_text()
            .map_err(|e| CredentialError::Io(std::io::Error::other(e.to_string())))?;

        let credential = match cred_type.to_lowercase().as_str() {
            "basic" => {
                let username: String = Input::new()
                    .with_prompt("Username")
                    .interact_text()
                    .map_err(|e| CredentialError::Io(std::io::Error::other(e.to_string())))?;

                let password: String = Password::new()
                    .with_prompt("Password")
                    .interact()
                    .map_err(|e| CredentialError::Io(std::io::Error::other(e.to_string())))?;

                Credential::basic(username, password)
            }
            "bearer" => {
                let token: String = Password::new()
                    .with_prompt("Bearer token")
                    .interact()
                    .map_err(|e| CredentialError::Io(std::io::Error::other(e.to_string())))?;

                Credential::bearer(token)
            }
            "apikey" => {
                let key: String = Password::new()
                    .with_prompt("API key")
                    .interact()
                    .map_err(|e| CredentialError::Io(std::io::Error::other(e.to_string())))?;

                Credential::api_key(key)
            }
            _ => {
                return Err(CredentialError::InvalidFormat(format!(
                    "unknown credential type: {cred_type}"
                )));
            }
        };

        if store {
            self.store(host, &credential)?;
        }

        Ok(credential)
    }

    /// Get or prompt for credential.
    ///
    /// # Errors
    /// Returns error if credential cannot be obtained.
    pub fn get_or_prompt(&self, host: &str) -> Result<Credential> {
        if let Ok(cred) = self.retrieve(host) {
            return Ok(cred);
        }

        self.prompt_for_credential(host, true)
    }

    /// Get credential for URL.
    ///
    /// # Errors
    /// Returns error if credential cannot be obtained.
    pub fn get_for_url(&self, url: &Url) -> Result<Option<Credential>> {
        if let Some(host) = url.host_str()
            && self.has_credential(host)
        {
            return self.retrieve(host).map(Some);
        }
        Ok(None)
    }

    /// Clear all cached credentials (does not delete from keyring).
    pub fn clear_cache(&self) {
        self.cache.write().clear();
    }

    /// Get credential using Git credential helper.
    ///
    /// This integrates with Git's credential helper system, allowing Libretto
    /// to use credentials stored by Git credential managers like:
    /// - git-credential-manager
    /// - git-credential-osxkeychain
    /// - git-credential-gnome-keyring
    /// - git-credential-store
    ///
    /// # Errors
    /// Returns error if Git credential helper fails.
    pub fn get_from_git_helper(&self, url: &Url) -> Result<Option<Credential>> {
        let host = url.host_str().unwrap_or("");
        let protocol = url.scheme();
        let path = url.path();

        // First check our cache
        if let Some(cred) = self.cache.read().get(host) {
            return Ok(Some(cred.clone()));
        }

        // Try git credential helper
        let mut child = Command::new("git")
            .args(["credential", "fill"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CredentialError::Io)?;

        // Write credential request
        if let Some(ref mut stdin) = child.stdin {
            writeln!(stdin, "protocol={protocol}").map_err(CredentialError::Io)?;
            writeln!(stdin, "host={host}").map_err(CredentialError::Io)?;
            if !path.is_empty() && path != "/" {
                writeln!(stdin, "path={}", path.trim_start_matches('/'))
                    .map_err(CredentialError::Io)?;
            }
            writeln!(stdin).map_err(CredentialError::Io)?;
        }

        let output = child.wait_with_output().map_err(CredentialError::Io)?;

        if !output.status.success() {
            return Ok(None);
        }

        // Parse response
        let mut username = None;
        let mut password = None;

        for line in output.stdout.lines() {
            let line = line.map_err(CredentialError::Io)?;
            if let Some((key, value)) = line.split_once('=') {
                match key {
                    "username" => username = Some(value.to_string()),
                    "password" => password = Some(value.to_string()),
                    _ => {}
                }
            }
        }

        if let (Some(user), Some(pass)) = (username, password) {
            let credential = Credential::basic(user, pass);
            // Cache it
            self.cache
                .write()
                .insert(host.to_string(), credential.clone());
            return Ok(Some(credential));
        }

        Ok(None)
    }

    /// Store credential in Git credential helper.
    ///
    /// # Errors
    /// Returns error if Git credential helper fails.
    pub fn store_in_git_helper(&self, url: &Url, credential: &Credential) -> Result<()> {
        let host = url.host_str().unwrap_or("");
        let protocol = url.scheme();
        let path = url.path();

        let mut child = Command::new("git")
            .args(["credential", "approve"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CredentialError::Io)?;

        if let Some(ref mut stdin) = child.stdin {
            writeln!(stdin, "protocol={protocol}").map_err(CredentialError::Io)?;
            writeln!(stdin, "host={host}").map_err(CredentialError::Io)?;
            if !path.is_empty() && path != "/" {
                writeln!(stdin, "path={}", path.trim_start_matches('/'))
                    .map_err(CredentialError::Io)?;
            }
            if let Some(ref user) = credential.username {
                writeln!(stdin, "username={user}").map_err(CredentialError::Io)?;
            }
            writeln!(stdin, "password={}", credential.secret).map_err(CredentialError::Io)?;
            writeln!(stdin).map_err(CredentialError::Io)?;
        }

        let _ = child.wait();

        // Also store in our cache
        self.cache
            .write()
            .insert(host.to_string(), credential.clone());

        Ok(())
    }

    /// Reject credential in Git credential helper (e.g., after auth failure).
    ///
    /// # Errors
    /// Returns error if Git credential helper fails.
    pub fn reject_in_git_helper(&self, url: &Url) -> Result<()> {
        let host = url.host_str().unwrap_or("");
        let protocol = url.scheme();

        let mut child = Command::new("git")
            .args(["credential", "reject"])
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(CredentialError::Io)?;

        if let Some(ref mut stdin) = child.stdin {
            writeln!(stdin, "protocol={protocol}").map_err(CredentialError::Io)?;
            writeln!(stdin, "host={host}").map_err(CredentialError::Io)?;
            writeln!(stdin).map_err(CredentialError::Io)?;
        }

        let _ = child.wait();

        // Remove from cache
        self.cache.write().remove(host);

        Ok(())
    }

    /// Get credential for URL, trying multiple sources in order:
    /// 1. In-memory cache
    /// 2. Keyring
    /// 3. Git credential helper
    ///
    /// # Errors
    /// Returns error if credential cannot be obtained.
    pub fn get_for_url_with_git(&self, url: &Url) -> Result<Option<Credential>> {
        // First try our existing methods
        if let Ok(Some(cred)) = self.get_for_url(url) {
            return Ok(Some(cred));
        }

        // Fall back to git credential helper
        self.get_from_git_helper(url)
    }
}

impl Default for CredentialManager {
    fn default() -> Self {
        Self::new("libretto")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credential_basic() {
        let cred = Credential::basic("user", "pass");
        assert_eq!(cred.cred_type, CredentialType::Basic);
        assert_eq!(cred.username.as_deref(), Some("user"));
    }

    #[test]
    fn test_credential_bearer() {
        let cred = Credential::bearer("token123");
        assert_eq!(cred.cred_type, CredentialType::Bearer);
        assert!(cred.username.is_none());
    }

    #[test]
    fn test_authorization_header() {
        let cred = Credential::basic("user", "pass");
        let header = cred.authorization_header();
        assert!(header.starts_with("Basic "));

        let cred = Credential::bearer("token123");
        let header = cred.authorization_header();
        assert_eq!(header, "Bearer token123");
    }

    #[test]
    fn test_credential_masking() {
        let cred = Credential::basic("user", "verylongpassword");
        let masked = cred.masked();
        assert!(masked.contains("..."));
        assert!(!masked.contains("verylongpassword"));

        let cred = Credential::bearer("short");
        let masked = cred.masked();
        assert!(!masked.contains("short"));
    }

    #[test]
    fn test_credential_manager_creation() {
        let manager = CredentialManager::new("test-service");
        assert_eq!(manager.service_name, "test-service");
    }

    #[test]
    fn test_zeroize() {
        let cred = Credential::basic("user", "password");
        let _secret_ptr = cred.secret.as_ptr();

        drop(cred);

        // Credential should be zeroized on drop (can't easily verify in safe Rust)
    }
}
