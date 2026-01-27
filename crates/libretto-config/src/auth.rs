//! Authentication configuration and credential management.

use crate::error::{ConfigError, Result};
use crate::types::{
    BearerToken, BitbucketOAuthCredentials, GitLabOAuthToken, GitLabToken, HttpBasicCredentials,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

/// Authentication configuration from auth.json.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case", default)]
pub struct AuthConfig {
    /// HTTP Basic auth credentials by domain.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub http_basic: BTreeMap<String, HttpBasicCredentials>,

    /// Bearer tokens by domain.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub bearer: BTreeMap<String, BearerToken>,

    /// GitHub OAuth tokens by domain.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub github_oauth: BTreeMap<String, String>,

    /// GitLab OAuth tokens by domain.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub gitlab_oauth: BTreeMap<String, GitLabOAuthToken>,

    /// GitLab private tokens by domain.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub gitlab_token: BTreeMap<String, GitLabToken>,

    /// Bitbucket OAuth credentials by domain.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub bitbucket_oauth: BTreeMap<String, BitbucketOAuthCredentials>,
}

impl AuthConfig {
    /// Load auth config from file.
    ///
    /// # Errors
    /// Returns error if file cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| ConfigError::io(path, e))?;
        sonic_rs::from_str(&content).map_err(|e| ConfigError::json(path, &e))
    }

    /// Load auth config from file, returning default if not found.
    #[must_use]
    pub fn load_or_default(path: &Path) -> Self {
        Self::load(path).unwrap_or_default()
    }

    /// Save auth config to file.
    ///
    /// # Errors
    /// Returns error if file cannot be written.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::io(parent, e))?;
        }
        let content = sonic_rs::to_string_pretty(self)?;
        std::fs::write(path, content).map_err(|e| ConfigError::io(path, e))
    }

    /// Merge another auth config into this one.
    pub fn merge(&mut self, other: &Self) {
        for (k, v) in &other.http_basic {
            self.http_basic.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.bearer {
            self.bearer.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.github_oauth {
            self.github_oauth.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.gitlab_oauth {
            self.gitlab_oauth.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.gitlab_token {
            self.gitlab_token.insert(k.clone(), v.clone());
        }
        for (k, v) in &other.bitbucket_oauth {
            self.bitbucket_oauth.insert(k.clone(), v.clone());
        }
    }

    /// Check if config is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.http_basic.is_empty()
            && self.bearer.is_empty()
            && self.github_oauth.is_empty()
            && self.gitlab_oauth.is_empty()
            && self.gitlab_token.is_empty()
            && self.bitbucket_oauth.is_empty()
    }

    /// Get HTTP Basic credentials for a domain.
    #[must_use]
    pub fn get_http_basic(&self, domain: &str) -> Option<&HttpBasicCredentials> {
        self.http_basic.get(domain).or_else(|| {
            // Try without www prefix
            let stripped = domain.strip_prefix("www.").unwrap_or(domain);
            self.http_basic.get(stripped)
        })
    }

    /// Get bearer token for a domain.
    #[must_use]
    pub fn get_bearer(&self, domain: &str) -> Option<&str> {
        self.bearer
            .get(domain)
            .or_else(|| {
                let stripped = domain.strip_prefix("www.").unwrap_or(domain);
                self.bearer.get(stripped)
            })
            .map(|b| match b {
                BearerToken::Simple(s) => s.as_str(),
                BearerToken::Extended { token } => token.as_str(),
            })
    }

    /// Get GitHub OAuth token for a domain.
    #[must_use]
    pub fn get_github_oauth(&self, domain: &str) -> Option<&str> {
        self.github_oauth
            .get(domain)
            .or_else(|| {
                let stripped = domain.strip_prefix("www.").unwrap_or(domain);
                self.github_oauth.get(stripped)
            })
            .map(String::as_str)
    }

    /// Get GitLab OAuth token for a domain.
    #[must_use]
    pub fn get_gitlab_oauth(&self, domain: &str) -> Option<&str> {
        self.gitlab_oauth
            .get(domain)
            .or_else(|| {
                let stripped = domain.strip_prefix("www.").unwrap_or(domain);
                self.gitlab_oauth.get(stripped)
            })
            .map(|t| t.token.as_str())
    }

    /// Get GitLab private token for a domain.
    #[must_use]
    pub fn get_gitlab_token(&self, domain: &str) -> Option<&str> {
        self.gitlab_token
            .get(domain)
            .or_else(|| {
                let stripped = domain.strip_prefix("www.").unwrap_or(domain);
                self.gitlab_token.get(stripped)
            })
            .map(|t| match t {
                GitLabToken::Simple(s) => s.as_str(),
                GitLabToken::Extended { token, .. } => token.as_str(),
            })
    }

    /// Get Bitbucket OAuth credentials for a domain.
    #[must_use]
    pub fn get_bitbucket_oauth(&self, domain: &str) -> Option<&BitbucketOAuthCredentials> {
        self.bitbucket_oauth.get(domain).or_else(|| {
            let stripped = domain.strip_prefix("www.").unwrap_or(domain);
            self.bitbucket_oauth.get(stripped)
        })
    }

    /// Set HTTP Basic credentials for a domain.
    pub fn set_http_basic(
        &mut self,
        domain: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) {
        self.http_basic.insert(
            domain.into(),
            HttpBasicCredentials {
                username: username.into(),
                password: password.into(),
            },
        );
    }

    /// Set bearer token for a domain.
    pub fn set_bearer(&mut self, domain: impl Into<String>, token: impl Into<String>) {
        self.bearer
            .insert(domain.into(), BearerToken::Simple(token.into()));
    }

    /// Set GitHub OAuth token for a domain.
    pub fn set_github_oauth(&mut self, domain: impl Into<String>, token: impl Into<String>) {
        self.github_oauth.insert(domain.into(), token.into());
    }

    /// Set GitLab OAuth token for a domain.
    pub fn set_gitlab_oauth(&mut self, domain: impl Into<String>, token: impl Into<String>) {
        self.gitlab_oauth.insert(
            domain.into(),
            GitLabOAuthToken {
                token: token.into(),
            },
        );
    }

    /// Set GitLab private token for a domain.
    pub fn set_gitlab_token(&mut self, domain: impl Into<String>, token: impl Into<String>) {
        self.gitlab_token
            .insert(domain.into(), GitLabToken::Simple(token.into()));
    }

    /// Set Bitbucket OAuth credentials for a domain.
    pub fn set_bitbucket_oauth(
        &mut self,
        domain: impl Into<String>,
        consumer_key: impl Into<String>,
        consumer_secret: impl Into<String>,
    ) {
        self.bitbucket_oauth.insert(
            domain.into(),
            BitbucketOAuthCredentials {
                consumer_key: consumer_key.into(),
                consumer_secret: consumer_secret.into(),
            },
        );
    }

    /// Remove all credentials for a domain.
    pub fn remove_domain(&mut self, domain: &str) {
        self.http_basic.remove(domain);
        self.bearer.remove(domain);
        self.github_oauth.remove(domain);
        self.gitlab_oauth.remove(domain);
        self.gitlab_token.remove(domain);
        self.bitbucket_oauth.remove(domain);
    }
}

/// Credential type for authentication.
#[derive(Debug, Clone)]
pub enum Credential {
    /// HTTP Basic authentication.
    HttpBasic {
        /// Username.
        username: String,
        /// Password.
        password: String,
    },
    /// Bearer token.
    Bearer(String),
    /// GitHub OAuth token.
    GitHubOAuth(String),
    /// GitLab OAuth token.
    GitLabOAuth(String),
    /// GitLab private token.
    GitLabToken(String),
    /// Bitbucket OAuth.
    BitbucketOAuth {
        /// Consumer key.
        consumer_key: String,
        /// Consumer secret.
        consumer_secret: String,
    },
}

impl Credential {
    /// Get credential as HTTP Authorization header value.
    #[must_use]
    pub fn as_authorization_header(&self) -> String {
        match self {
            Self::HttpBasic { username, password } => {
                use std::io::Write;
                let mut buf = Vec::new();
                write!(buf, "{username}:{password}").ok();
                let encoded = base64_encode(&buf);
                format!("Basic {encoded}")
            }
            Self::Bearer(token)
            | Self::GitHubOAuth(token)
            | Self::GitLabOAuth(token)
            | Self::GitLabToken(token) => {
                format!("Bearer {token}")
            }
            Self::BitbucketOAuth {
                consumer_key,
                consumer_secret,
            } => {
                use std::io::Write;
                let mut buf = Vec::new();
                write!(buf, "{consumer_key}:{consumer_secret}").ok();
                let encoded = base64_encode(&buf);
                format!("Basic {encoded}")
            }
        }
    }
}

/// Simple base64 encoding without external dependency.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut result = String::with_capacity((data.len() + 2) / 3 * 4);

    for chunk in data.chunks(3) {
        let b0 = chunk[0] as usize;
        let b1 = chunk.get(1).copied().unwrap_or(0) as usize;
        let b2 = chunk.get(2).copied().unwrap_or(0) as usize;

        result.push(ALPHABET[b0 >> 2] as char);
        result.push(ALPHABET[((b0 & 0x03) << 4) | (b1 >> 4)] as char);

        if chunk.len() > 1 {
            result.push(ALPHABET[((b1 & 0x0f) << 2) | (b2 >> 6)] as char);
        } else {
            result.push('=');
        }

        if chunk.len() > 2 {
            result.push(ALPHABET[b2 & 0x3f] as char);
        } else {
            result.push('=');
        }
    }

    result
}

/// Credential store with keyring integration.
#[derive(Debug)]
pub struct CredentialStore {
    /// In-memory auth config.
    auth: AuthConfig,
    /// Path to auth.json.
    auth_path: std::path::PathBuf,
    /// Use keyring for secure storage.
    #[cfg(feature = "keyring")]
    use_keyring: bool,
}

impl CredentialStore {
    /// Create a new credential store.
    #[must_use]
    pub fn new(auth_path: impl Into<std::path::PathBuf>) -> Self {
        let auth_path = auth_path.into();
        let auth = AuthConfig::load_or_default(&auth_path);

        Self {
            auth,
            auth_path,
            #[cfg(feature = "keyring")]
            use_keyring: true,
        }
    }

    /// Get credential for a domain.
    #[must_use]
    pub fn get(&self, domain: &str) -> Option<Credential> {
        // Check in-memory config first
        if let Some(cred) = self.auth.get_http_basic(domain) {
            return Some(Credential::HttpBasic {
                username: cred.username.clone(),
                password: cred.password.clone(),
            });
        }

        if let Some(token) = self.auth.get_bearer(domain) {
            return Some(Credential::Bearer(token.to_string()));
        }

        if let Some(token) = self.auth.get_github_oauth(domain) {
            return Some(Credential::GitHubOAuth(token.to_string()));
        }

        if let Some(token) = self.auth.get_gitlab_oauth(domain) {
            return Some(Credential::GitLabOAuth(token.to_string()));
        }

        if let Some(token) = self.auth.get_gitlab_token(domain) {
            return Some(Credential::GitLabToken(token.to_string()));
        }

        if let Some(cred) = self.auth.get_bitbucket_oauth(domain) {
            return Some(Credential::BitbucketOAuth {
                consumer_key: cred.consumer_key.clone(),
                consumer_secret: cred.consumer_secret.clone(),
            });
        }

        // Try keyring if enabled
        #[cfg(feature = "keyring")]
        if self.use_keyring {
            if let Some(cred) = self.get_from_keyring(domain) {
                return Some(cred);
            }
        }

        None
    }

    /// Store credential for a domain.
    ///
    /// # Errors
    /// Returns error if credential cannot be stored.
    pub fn store(&mut self, domain: &str, credential: Credential) -> Result<()> {
        match &credential {
            Credential::HttpBasic { username, password } => {
                self.auth.set_http_basic(domain, username, password);
            }
            Credential::Bearer(token) => {
                self.auth.set_bearer(domain, token);
            }
            Credential::GitHubOAuth(token) => {
                self.auth.set_github_oauth(domain, token);
            }
            Credential::GitLabOAuth(token) => {
                self.auth.set_gitlab_oauth(domain, token);
            }
            Credential::GitLabToken(token) => {
                self.auth.set_gitlab_token(domain, token);
            }
            Credential::BitbucketOAuth {
                consumer_key,
                consumer_secret,
            } => {
                self.auth
                    .set_bitbucket_oauth(domain, consumer_key, consumer_secret);
            }
        }

        // Try to store in keyring
        #[cfg(feature = "keyring")]
        if self.use_keyring {
            let _ = self.store_in_keyring(domain, &credential);
        }

        // Save to auth.json
        self.auth.save(&self.auth_path)
    }

    /// Remove credential for a domain.
    ///
    /// # Errors
    /// Returns error if credential cannot be removed.
    pub fn remove(&mut self, domain: &str) -> Result<()> {
        self.auth.remove_domain(domain);

        #[cfg(feature = "keyring")]
        if self.use_keyring {
            let _ = self.remove_from_keyring(domain);
        }

        self.auth.save(&self.auth_path)
    }

    /// Get underlying auth config.
    #[must_use]
    pub fn auth(&self) -> &AuthConfig {
        &self.auth
    }

    /// Reload auth config from disk.
    pub fn reload(&mut self) {
        self.auth = AuthConfig::load_or_default(&self.auth_path);
    }

    #[cfg(feature = "keyring")]
    fn get_from_keyring(&self, domain: &str) -> Option<Credential> {
        let entry = keyring::Entry::new("libretto", domain).ok()?;
        let password = entry.get_password().ok()?;

        // Try to parse as JSON credential
        if let Ok(cred) = sonic_rs::from_str::<KeyringCredential>(&password) {
            return Some(cred.into());
        }

        // Fall back to treating as bearer token
        Some(Credential::Bearer(password))
    }

    #[cfg(feature = "keyring")]
    fn store_in_keyring(
        &self,
        domain: &str,
        credential: &Credential,
    ) -> std::result::Result<(), keyring::Error> {
        let entry = keyring::Entry::new("libretto", domain)?;
        let cred = KeyringCredential::from(credential.clone());
        let json = sonic_rs::to_string(&cred)
            .map_err(|_| keyring::Error::Invalid("json".into(), "serialize failed".into()))?;
        entry.set_password(&json)
    }

    #[cfg(feature = "keyring")]
    fn remove_from_keyring(&self, domain: &str) -> std::result::Result<(), keyring::Error> {
        let entry = keyring::Entry::new("libretto", domain)?;
        entry.delete_credential()
    }
}

#[cfg(feature = "keyring")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KeyringCredential {
    HttpBasic {
        username: String,
        password: String,
    },
    Bearer {
        token: String,
    },
    GithubOauth {
        token: String,
    },
    GitlabOauth {
        token: String,
    },
    GitlabToken {
        token: String,
    },
    BitbucketOauth {
        consumer_key: String,
        consumer_secret: String,
    },
}

#[cfg(feature = "keyring")]
impl From<Credential> for KeyringCredential {
    fn from(cred: Credential) -> Self {
        match cred {
            Credential::HttpBasic { username, password } => Self::HttpBasic { username, password },
            Credential::Bearer(token) => Self::Bearer { token },
            Credential::GitHubOAuth(token) => Self::GithubOauth { token },
            Credential::GitLabOAuth(token) => Self::GitlabOauth { token },
            Credential::GitLabToken(token) => Self::GitlabToken { token },
            Credential::BitbucketOAuth {
                consumer_key,
                consumer_secret,
            } => Self::BitbucketOauth {
                consumer_key,
                consumer_secret,
            },
        }
    }
}

#[cfg(feature = "keyring")]
impl From<KeyringCredential> for Credential {
    fn from(cred: KeyringCredential) -> Self {
        match cred {
            KeyringCredential::HttpBasic { username, password } => {
                Self::HttpBasic { username, password }
            }
            KeyringCredential::Bearer { token } => Self::Bearer(token),
            KeyringCredential::GithubOauth { token } => Self::GitHubOAuth(token),
            KeyringCredential::GitlabOauth { token } => Self::GitLabOAuth(token),
            KeyringCredential::GitlabToken { token } => Self::GitLabToken(token),
            KeyringCredential::BitbucketOauth {
                consumer_key,
                consumer_secret,
            } => Self::BitbucketOAuth {
                consumer_key,
                consumer_secret,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auth_config_empty() {
        let config = AuthConfig::default();
        assert!(config.is_empty());
    }

    #[test]
    fn auth_config_set_get() {
        let mut config = AuthConfig::default();
        config.set_http_basic("example.com", "user", "pass");

        let cred = config.get_http_basic("example.com").unwrap();
        assert_eq!(cred.username, "user");
        assert_eq!(cred.password, "pass");
    }

    #[test]
    fn auth_config_merge() {
        let mut config1 = AuthConfig::default();
        config1.set_github_oauth("github.com", "token1");

        let mut config2 = AuthConfig::default();
        config2.set_github_oauth("github.com", "token2");
        config2.set_gitlab_token("gitlab.com", "token3");

        config1.merge(&config2);

        assert_eq!(config1.get_github_oauth("github.com"), Some("token2"));
        assert_eq!(config1.get_gitlab_token("gitlab.com"), Some("token3"));
    }

    #[test]
    fn credential_authorization_header() {
        let bearer = Credential::Bearer("mytoken".to_string());
        assert_eq!(bearer.as_authorization_header(), "Bearer mytoken");

        let basic = Credential::HttpBasic {
            username: "user".to_string(),
            password: "pass".to_string(),
        };
        assert!(basic.as_authorization_header().starts_with("Basic "));
    }

    #[test]
    fn base64_encode_basic() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }
}
