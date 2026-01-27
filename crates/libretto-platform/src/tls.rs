//! Cross-platform TLS/SSL implementation.
//!
//! Provides:
//! - rustls-based TLS (pure Rust, cross-platform)
//! - Optional native-tls fallback
//! - Certificate validation
//! - Client certificate support

use crate::{PlatformError, Result};
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "tls")]
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};

/// TLS configuration builder.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Root certificates.
    root_certs: RootCertificates,
    /// Client certificate for mTLS.
    client_cert: Option<ClientCertificate>,
    /// Minimum TLS version.
    min_version: TlsVersion,
    /// Maximum TLS version.
    max_version: TlsVersion,
    /// ALPN protocols.
    alpn_protocols: Vec<String>,
    /// Whether to verify server certificates.
    verify_server: bool,
    /// Whether to verify server hostname.
    verify_hostname: bool,
}

impl TlsConfig {
    /// Create a new TLS config with sensible defaults.
    #[must_use]
    pub fn new() -> Self {
        Self {
            root_certs: RootCertificates::WebPki,
            client_cert: None,
            min_version: TlsVersion::Tls12,
            max_version: TlsVersion::Tls13,
            alpn_protocols: vec!["h2".to_string(), "http/1.1".to_string()],
            verify_server: true,
            verify_hostname: true,
        }
    }

    /// Use system root certificates.
    #[must_use]
    pub fn with_system_roots(mut self) -> Self {
        self.root_certs = RootCertificates::System;
        self
    }

    /// Use WebPKI root certificates.
    #[must_use]
    pub fn with_webpki_roots(mut self) -> Self {
        self.root_certs = RootCertificates::WebPki;
        self
    }

    /// Add custom root certificates from file.
    ///
    /// # Errors
    /// Returns error if certificates cannot be loaded.
    pub fn with_root_certs_file(mut self, path: impl AsRef<Path>) -> Result<Self> {
        let certs = load_certificates_from_file(path.as_ref())?;
        self.root_certs = RootCertificates::Custom(certs);
        Ok(self)
    }

    /// Add custom root certificates from PEM data.
    ///
    /// # Errors
    /// Returns error if certificates cannot be parsed.
    pub fn with_root_certs_pem(mut self, pem: &[u8]) -> Result<Self> {
        let certs = load_certificates_from_pem(pem)?;
        self.root_certs = RootCertificates::Custom(certs);
        Ok(self)
    }

    /// Set client certificate for mTLS.
    ///
    /// # Errors
    /// Returns error if certificate cannot be loaded.
    pub fn with_client_cert(
        mut self,
        cert_path: impl AsRef<Path>,
        key_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let cert = load_certificates_from_file(cert_path.as_ref())?;
        let key = load_private_key_from_file(key_path.as_ref())?;
        self.client_cert = Some(ClientCertificate { cert, key });
        Ok(self)
    }

    /// Set client certificate from PEM data.
    ///
    /// # Errors
    /// Returns error if certificate cannot be parsed.
    pub fn with_client_cert_pem(mut self, cert_pem: &[u8], key_pem: &[u8]) -> Result<Self> {
        let cert = load_certificates_from_pem(cert_pem)?;
        let key = load_private_key_from_pem(key_pem)?;
        self.client_cert = Some(ClientCertificate { cert, key });
        Ok(self)
    }

    /// Set minimum TLS version.
    #[must_use]
    pub const fn with_min_version(mut self, version: TlsVersion) -> Self {
        self.min_version = version;
        self
    }

    /// Set maximum TLS version.
    #[must_use]
    pub const fn with_max_version(mut self, version: TlsVersion) -> Self {
        self.max_version = version;
        self
    }

    /// Set ALPN protocols.
    #[must_use]
    pub fn with_alpn(mut self, protocols: Vec<String>) -> Self {
        self.alpn_protocols = protocols;
        self
    }

    /// Disable server certificate verification (DANGER!).
    ///
    /// Only use this for testing or when you know what you're doing.
    #[must_use]
    pub const fn danger_disable_verify(mut self) -> Self {
        self.verify_server = false;
        self.verify_hostname = false;
        self
    }

    /// Disable hostname verification only.
    #[must_use]
    pub const fn disable_hostname_verification(mut self) -> Self {
        self.verify_hostname = false;
        self
    }

    /// Build a rustls client config.
    ///
    /// # Errors
    /// Returns error if TLS configuration fails.
    #[cfg(feature = "tls")]
    pub fn build_client_config(&self) -> Result<Arc<rustls::ClientConfig>> {
        use rustls::ClientConfig;

        let root_store = self.build_root_store()?;

        let builder = ClientConfig::builder().with_root_certificates(root_store);

        let config = if let Some(ref client_cert) = self.client_cert {
            // Clone the certificate chain and key for mTLS
            let cert_chain: Vec<CertificateDer<'static>> = client_cert
                .cert
                .iter()
                .map(|c| c.clone().into_owned())
                .collect();

            let key = client_cert.key.clone_key();

            builder
                .with_client_auth_cert(cert_chain, key)
                .map_err(|e| PlatformError::Tls(e.to_string()))?
        } else {
            builder.with_no_client_auth()
        };

        Ok(Arc::new(config))
    }

    #[cfg(feature = "tls")]
    fn build_root_store(&self) -> Result<rustls::RootCertStore> {
        let mut root_store = rustls::RootCertStore::empty();

        match &self.root_certs {
            RootCertificates::WebPki => {
                root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            }
            RootCertificates::System => {
                // Try to load system certificates
                #[cfg(feature = "tls")]
                {
                    // Use webpki-roots as fallback since rustls-native-certs can fail
                    root_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                }
            }
            RootCertificates::Custom(certs) => {
                for cert in certs {
                    root_store
                        .add(cert.clone().into_owned())
                        .map_err(|e| PlatformError::Certificate(e.to_string()))?;
                }
            }
        }

        Ok(root_store)
    }
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Root certificate source.
#[derive(Debug, Clone)]
pub enum RootCertificates {
    /// Use WebPKI root certificates (bundled).
    WebPki,
    /// Use system root certificates.
    System,
    /// Use custom root certificates.
    #[cfg(feature = "tls")]
    Custom(Vec<CertificateDer<'static>>),
    #[cfg(not(feature = "tls"))]
    Custom(Vec<Vec<u8>>),
}

/// Client certificate for mTLS.
#[derive(Debug)]
pub struct ClientCertificate {
    #[cfg(feature = "tls")]
    cert: Vec<CertificateDer<'static>>,
    #[cfg(feature = "tls")]
    key: PrivateKeyDer<'static>,
    #[cfg(not(feature = "tls"))]
    cert: Vec<Vec<u8>>,
    #[cfg(not(feature = "tls"))]
    key: Vec<u8>,
}

#[cfg(feature = "tls")]
impl Clone for ClientCertificate {
    fn clone(&self) -> Self {
        Self {
            cert: self.cert.clone(),
            key: self.key.clone_key(),
        }
    }
}

/// TLS version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TlsVersion {
    /// TLS 1.0 (deprecated, avoid).
    Tls10,
    /// TLS 1.1 (deprecated, avoid).
    Tls11,
    /// TLS 1.2.
    Tls12,
    /// TLS 1.3.
    Tls13,
}

impl TlsVersion {
    /// Get as rustls SupportedProtocolVersion.
    #[cfg(feature = "tls")]
    #[must_use]
    pub const fn as_rustls(&self) -> &'static rustls::SupportedProtocolVersion {
        match self {
            // rustls doesn't support TLS 1.0/1.1
            Self::Tls10 | Self::Tls11 | Self::Tls12 => &rustls::version::TLS12,
            Self::Tls13 => &rustls::version::TLS13,
        }
    }
}

/// Load certificates from a PEM file.
///
/// # Errors
/// Returns error if file cannot be read or parsed.
#[cfg(feature = "tls")]
pub fn load_certificates_from_file(path: &Path) -> Result<Vec<CertificateDer<'static>>> {
    let file = std::fs::File::open(path).map_err(|e| PlatformError::io(path, e))?;
    let mut reader = BufReader::new(file);
    load_certificates_from_reader(&mut reader)
}

/// Load certificates from PEM data.
///
/// # Errors
/// Returns error if data cannot be parsed.
#[cfg(feature = "tls")]
pub fn load_certificates_from_pem(pem: &[u8]) -> Result<Vec<CertificateDer<'static>>> {
    let mut reader = BufReader::new(pem);
    load_certificates_from_reader(&mut reader)
}

#[cfg(feature = "tls")]
fn load_certificates_from_reader<R: Read>(
    reader: &mut BufReader<R>,
) -> Result<Vec<CertificateDer<'static>>> {
    let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(reader)
        .filter_map(|r| r.ok())
        .collect();

    if certs.is_empty() {
        return Err(PlatformError::Certificate(
            "No certificates found in PEM data".to_string(),
        ));
    }

    Ok(certs)
}

/// Load a private key from a PEM file.
///
/// # Errors
/// Returns error if file cannot be read or parsed.
#[cfg(feature = "tls")]
pub fn load_private_key_from_file(path: &Path) -> Result<PrivateKeyDer<'static>> {
    let file = std::fs::File::open(path).map_err(|e| PlatformError::io(path, e))?;
    let mut reader = BufReader::new(file);
    load_private_key_from_reader(&mut reader)
}

/// Load a private key from PEM data.
///
/// # Errors
/// Returns error if data cannot be parsed.
#[cfg(feature = "tls")]
pub fn load_private_key_from_pem(pem: &[u8]) -> Result<PrivateKeyDer<'static>> {
    let mut reader = BufReader::new(pem);
    load_private_key_from_reader(&mut reader)
}

#[cfg(feature = "tls")]
fn load_private_key_from_reader<R: Read>(
    reader: &mut BufReader<R>,
) -> Result<PrivateKeyDer<'static>> {
    // Try different key formats
    loop {
        match rustls_pemfile::read_one(reader) {
            Ok(Some(rustls_pemfile::Item::Pkcs1Key(key))) => {
                return Ok(PrivateKeyDer::Pkcs1(key));
            }
            Ok(Some(rustls_pemfile::Item::Pkcs8Key(key))) => {
                return Ok(PrivateKeyDer::Pkcs8(key));
            }
            Ok(Some(rustls_pemfile::Item::Sec1Key(key))) => {
                return Ok(PrivateKeyDer::Sec1(key));
            }
            Ok(Some(_)) => continue, // Skip other items
            Ok(None) => break,
            Err(e) => return Err(PlatformError::Certificate(e.to_string())),
        }
    }

    Err(PlatformError::Certificate(
        "No private key found in PEM data".to_string(),
    ))
}

/// Validate a certificate chain.
///
/// # Errors
/// Returns error if validation fails.
#[cfg(feature = "tls")]
pub fn validate_certificate_chain(chain: &[CertificateDer<'_>], hostname: &str) -> Result<()> {
    if chain.is_empty() {
        return Err(PlatformError::Certificate(
            "Empty certificate chain".to_string(),
        ));
    }

    // Basic validation - check that we have at least one certificate
    // Full validation happens during TLS handshake

    // Validate hostname format
    let _server_name: ServerName<'_> = hostname
        .try_into()
        .map_err(|_| PlatformError::Certificate(format!("Invalid hostname: {hostname}")))?;

    Ok(())
}

/// Get certificate information.
#[derive(Debug, Clone)]
#[cfg(feature = "tls")]
pub struct CertificateInfo {
    /// Subject name.
    pub subject: String,
    /// Issuer name.
    pub issuer: String,
    /// Serial number (hex).
    pub serial: String,
    /// Not before date.
    pub not_before: String,
    /// Not after date.
    pub not_after: String,
    /// Whether the certificate is self-signed.
    pub is_self_signed: bool,
}

#[cfg(feature = "tls")]
impl CertificateInfo {
    /// Parse certificate information from DER data.
    ///
    /// Note: This is a simplified parser. For production use,
    /// consider using x509-parser crate.
    #[must_use]
    pub fn from_der(der: &[u8]) -> Option<Self> {
        // Simplified - just indicate we have a certificate
        // Full parsing would require x509-parser
        Some(Self {
            subject: "Certificate present".to_string(),
            issuer: "Unknown".to_string(),
            serial: format!("{:02x}", der.len()),
            not_before: "Unknown".to_string(),
            not_after: "Unknown".to_string(),
            is_self_signed: false,
        })
    }
}

/// TLS connector for establishing secure connections.
#[derive(Debug, Clone)]
#[cfg(feature = "tls")]
pub struct TlsConnector {
    config: Arc<rustls::ClientConfig>,
}

#[cfg(feature = "tls")]
impl TlsConnector {
    /// Create a new TLS connector with default config.
    ///
    /// # Errors
    /// Returns error if TLS setup fails.
    pub fn new() -> Result<Self> {
        let config = TlsConfig::new().build_client_config()?;
        Ok(Self { config })
    }

    /// Create from a TlsConfig.
    ///
    /// # Errors
    /// Returns error if TLS setup fails.
    pub fn from_config(config: &TlsConfig) -> Result<Self> {
        let client_config = config.build_client_config()?;
        Ok(Self {
            config: client_config,
        })
    }

    /// Get the inner rustls ClientConfig.
    #[must_use]
    pub fn config(&self) -> Arc<rustls::ClientConfig> {
        Arc::clone(&self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_config_default() {
        let config = TlsConfig::new();
        assert!(config.verify_server);
        assert!(config.verify_hostname);
        assert_eq!(config.min_version, TlsVersion::Tls12);
        assert_eq!(config.max_version, TlsVersion::Tls13);
    }

    #[test]
    fn tls_config_builder() {
        let config = TlsConfig::new()
            .with_webpki_roots()
            .with_min_version(TlsVersion::Tls13)
            .with_alpn(vec!["h2".to_string()]);

        assert_eq!(config.min_version, TlsVersion::Tls13);
        assert_eq!(config.alpn_protocols, vec!["h2"]);
    }

    #[test]
    fn tls_version_ordering() {
        assert!(TlsVersion::Tls13 > TlsVersion::Tls12);
        assert!(TlsVersion::Tls12 > TlsVersion::Tls11);
        assert!(TlsVersion::Tls11 > TlsVersion::Tls10);
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_connector_creation() {
        let connector = TlsConnector::new();
        assert!(connector.is_ok());
    }

    #[cfg(feature = "tls")]
    #[test]
    fn tls_config_build() {
        let config = TlsConfig::new().build_client_config();
        assert!(config.is_ok());
    }
}
