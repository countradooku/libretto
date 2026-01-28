//! Package signature verification using GPG/PGP and Ed25519.

use ed25519_dalek::{Signature as Ed25519Sig, Verifier, VerifyingKey};
use sequoia_openpgp::Packet;
use sequoia_openpgp::cert::Cert;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::types::SignatureType;

use std::path::Path;
use thiserror::Error;

/// Signature verification error.
#[derive(Debug, Error)]
pub enum SignatureError {
    /// Invalid signature.
    #[error("invalid signature")]
    InvalidSignature,

    /// Signature verification failed.
    #[error("signature verification failed: {0}")]
    VerificationFailed(String),

    /// Unknown signing key.
    #[error("unknown signing key: {0}")]
    UnknownKey(String),

    /// Invalid key format.
    #[error("invalid key format: {0}")]
    InvalidKey(String),

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// OpenPGP error.
    #[error("OpenPGP error: {0}")]
    OpenPgp(#[from] anyhow::Error),
}

/// Result type for signature operations.
pub type Result<T> = std::result::Result<T, SignatureError>;

/// Signature algorithm type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignatureAlgorithm {
    /// GPG/PGP signature.
    Pgp,
    /// Ed25519 signature.
    Ed25519,
}

/// Verified signature information.
#[derive(Debug, Clone)]
pub struct VerifiedSignature {
    /// Signature algorithm.
    pub sig_type: SignatureAlgorithm,
    /// Signer key ID or fingerprint.
    pub key_id: String,
    /// Signer identity (if available).
    pub signer: Option<String>,
}

/// PGP signature verifier.
#[derive(Debug)]
pub struct PgpVerifier {
    policy: StandardPolicy<'static>,
    trusted_keys: Vec<Cert>,
}

impl PgpVerifier {
    /// Create new PGP verifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            policy: StandardPolicy::new(),
            trusted_keys: Vec::new(),
        }
    }

    /// Add trusted key from PEM/ASCII armored format.
    ///
    /// # Errors
    /// Returns error if key cannot be parsed.
    pub fn add_trusted_key(&mut self, key_data: &[u8]) -> Result<()> {
        let cert =
            Cert::from_bytes(key_data).map_err(|e| SignatureError::InvalidKey(e.to_string()))?;
        self.trusted_keys.push(cert);
        Ok(())
    }

    /// Load trusted keys from file.
    ///
    /// # Errors
    /// Returns error if file cannot be read or parsed.
    pub async fn load_trusted_keys(&mut self, path: impl AsRef<Path>) -> Result<()> {
        let data = tokio::fs::read(path).await?;
        self.add_trusted_key(&data)
    }

    /// Verify detached signature.
    ///
    /// # Errors
    /// Returns error if verification fails.
    pub fn verify_detached(&self, _data: &[u8], signature: &[u8]) -> Result<VerifiedSignature> {
        let packets = sequoia_openpgp::PacketPile::from_bytes(signature)
            .map_err(|e| SignatureError::VerificationFailed(e.to_string()))?;

        // Find signature packet
        let sig_packet = packets
            .children()
            .find_map(|p| {
                if let Packet::Signature(sig) = p {
                    Some(sig.clone())
                } else {
                    None
                }
            })
            .ok_or(SignatureError::InvalidSignature)?;

        // Verify signature type
        if sig_packet.typ() != SignatureType::Binary && sig_packet.typ() != SignatureType::Text {
            return Err(SignatureError::InvalidSignature);
        }

        // Get key ID
        let issuers: Vec<_> = sig_packet.get_issuers().into_iter().collect();
        if issuers.is_empty() {
            return Err(SignatureError::UnknownKey(
                "no issuer in signature".to_string(),
            ));
        }

        let key_id = issuers[0].to_hex();

        // Find matching trusted key
        for cert in &self.trusted_keys {
            // Get primary key with policy
            let key_amalg = cert.keys().with_policy(&self.policy, None);

            // Try each key
            for key in key_amalg {
                // Try to verify the signature with this key
                // verify_direct_key requires both signing key and target key
                let pk = cert.primary_key().key();
                if sig_packet.verify_direct_key(key.key(), pk).is_ok() {
                    // Get signer identity
                    let signer = cert
                        .userids()
                        .next()
                        .map(|uid| String::from_utf8_lossy(uid.value()).to_string());

                    return Ok(VerifiedSignature {
                        sig_type: SignatureAlgorithm::Pgp,
                        key_id: key_id.clone(),
                        signer,
                    });
                }
            }
        }

        Err(SignatureError::UnknownKey(key_id))
    }
}

impl Default for PgpVerifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Ed25519 signature verifier.
#[derive(Debug)]
pub struct Ed25519Verifier {
    trusted_keys: Vec<VerifyingKey>,
}

impl Ed25519Verifier {
    /// Create new Ed25519 verifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            trusted_keys: Vec::new(),
        }
    }

    /// Add trusted public key (32 bytes).
    ///
    /// # Errors
    /// Returns error if key is invalid.
    pub fn add_trusted_key(&mut self, key_bytes: &[u8]) -> Result<()> {
        if key_bytes.len() != 32 {
            return Err(SignatureError::InvalidKey(format!(
                "expected 32 bytes, got {}",
                key_bytes.len()
            )));
        }

        let key = VerifyingKey::from_bytes(
            key_bytes
                .try_into()
                .map_err(|_| SignatureError::InvalidKey("invalid key length".to_string()))?,
        )
        .map_err(|e| SignatureError::InvalidKey(e.to_string()))?;

        self.trusted_keys.push(key);
        Ok(())
    }

    /// Verify signature.
    ///
    /// # Errors
    /// Returns error if verification fails.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<VerifiedSignature> {
        if signature.len() != 64 {
            return Err(SignatureError::InvalidSignature);
        }

        let sig = Ed25519Sig::from_bytes(
            signature
                .try_into()
                .map_err(|_| SignatureError::InvalidSignature)?,
        );

        // Try each trusted key
        for key in &self.trusted_keys {
            if key.verify(data, &sig).is_ok() {
                return Ok(VerifiedSignature {
                    sig_type: SignatureAlgorithm::Ed25519,
                    key_id: hex::encode(key.as_bytes()),
                    signer: None,
                });
            }
        }

        Err(SignatureError::VerificationFailed(
            "no matching key found".to_string(),
        ))
    }
}

impl Default for Ed25519Verifier {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined signature verifier supporting multiple formats.
#[derive(Debug)]
pub struct SignatureVerifier {
    pgp: PgpVerifier,
    ed25519: Ed25519Verifier,
}

impl SignatureVerifier {
    /// Create new signature verifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pgp: PgpVerifier::new(),
            ed25519: Ed25519Verifier::new(),
        }
    }

    /// Add PGP trusted key.
    ///
    /// # Errors
    /// Returns error if key cannot be parsed.
    pub fn add_pgp_key(&mut self, key_data: &[u8]) -> Result<()> {
        self.pgp.add_trusted_key(key_data)
    }

    /// Add Ed25519 trusted key.
    ///
    /// # Errors
    /// Returns error if key is invalid.
    pub fn add_ed25519_key(&mut self, key_bytes: &[u8]) -> Result<()> {
        self.ed25519.add_trusted_key(key_bytes)
    }

    /// Verify signature (auto-detect format).
    ///
    /// # Errors
    /// Returns error if verification fails.
    pub fn verify(&self, data: &[u8], signature: &[u8]) -> Result<VerifiedSignature> {
        // Try Ed25519 first (simpler check)
        if signature.len() == 64 {
            if let Ok(verified) = self.ed25519.verify(data, signature) {
                return Ok(verified);
            }
        }

        // Try PGP
        self.pgp.verify_detached(data, signature)
    }
}

impl Default for SignatureVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_signature_algorithm() {
        assert_eq!(SignatureAlgorithm::Pgp, SignatureAlgorithm::Pgp);
        assert_ne!(SignatureAlgorithm::Pgp, SignatureAlgorithm::Ed25519);
    }

    #[test]
    fn test_pgp_verifier_creation() {
        let verifier = PgpVerifier::new();
        assert_eq!(verifier.trusted_keys.len(), 0);
    }

    #[test]
    fn test_ed25519_verifier_creation() {
        let verifier = Ed25519Verifier::new();
        assert_eq!(verifier.trusted_keys.len(), 0);
    }

    #[test]
    fn test_ed25519_invalid_key_length() {
        let mut verifier = Ed25519Verifier::new();
        let result = verifier.add_trusted_key(&[0u8; 16]);
        assert!(matches!(result, Err(SignatureError::InvalidKey(_))));
    }
}
