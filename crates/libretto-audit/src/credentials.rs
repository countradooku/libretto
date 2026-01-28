//! Secure credential management with keyring storage.

use dialoguer::{Input, Password};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
        if let Some(host) = url.host_str() {
            if self.has_credential(host) {
                return self.retrieve(host).map(Some);
            }
        }
        Ok(None)
    }

    /// Clear all cached credentials (does not delete from keyring).
    pub fn clear_cache(&self) {
        self.cache.write().clear();
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
