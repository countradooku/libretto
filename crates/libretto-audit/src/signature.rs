//! Package signature verification using GPG/PGP and Ed25519 with trust chain support.

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature as Ed25519Sig, Verifier, VerifyingKey};
use sequoia_openpgp::Packet;
use sequoia_openpgp::cert::Cert;
use sequoia_openpgp::parse::Parse;
use sequoia_openpgp::policy::StandardPolicy;
use sequoia_openpgp::types::SignatureType;
use std::collections::HashMap;
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

    /// `OpenPGP` error.
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
    pub const fn new() -> Self {
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
                        .map(|uid| String::from_utf8_lossy(uid.userid().value()).to_string());

                    return Ok(VerifiedSignature {
                        sig_type: SignatureAlgorithm::Pgp,
                        key_id,
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
    pub const fn new() -> Self {
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
    pub const fn new() -> Self {
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
        if signature.len() == 64
            && let Ok(verified) = self.ed25519.verify(data, signature)
        {
            return Ok(verified);
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

/// Trust level for keys in the trust chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum TrustLevel {
    /// Unknown trust - key is not in trust store.
    #[default]
    Unknown,
    /// Untrusted - explicitly marked as not trusted.
    Untrusted,
    /// Marginally trusted - some confidence.
    Marginal,
    /// Fully trusted - high confidence.
    Full,
    /// Ultimate trust - root of trust (your own keys).
    Ultimate,
}

/// A trusted key entry in the trust store.
#[derive(Debug, Clone)]
pub struct TrustedKey {
    /// Key identifier (fingerprint or key ID).
    pub key_id: String,
    /// Human-readable name/description.
    pub name: Option<String>,
    /// Trust level assigned to this key.
    pub trust_level: TrustLevel,
    /// When this key was added to the trust store.
    pub added_at: DateTime<Utc>,
    /// Optional expiration date.
    pub expires_at: Option<DateTime<Utc>>,
    /// Keys that have signed this key (for web of trust).
    pub signed_by: Vec<String>,
}

impl TrustedKey {
    /// Create a new trusted key entry.
    #[must_use]
    pub fn new(key_id: impl Into<String>, trust_level: TrustLevel) -> Self {
        Self {
            key_id: key_id.into(),
            name: None,
            trust_level,
            added_at: Utc::now(),
            expires_at: None,
            signed_by: Vec::new(),
        }
    }

    /// Set the key name.
    #[must_use]
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set expiration date.
    #[must_use]
    pub const fn with_expiration(mut self, expires_at: DateTime<Utc>) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Check if the key is expired.
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|exp| exp < Utc::now())
    }

    /// Check if the key is valid (trusted and not expired).
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.trust_level >= TrustLevel::Marginal && !self.is_expired()
    }
}

/// Trust chain validator for package signatures.
///
/// Implements a simplified PKI/Web of Trust model where:
/// - Root keys have Ultimate trust
/// - Keys signed by trusted keys inherit trust
/// - Trust is transitive with decreasing levels
#[derive(Debug, Default)]
pub struct TrustChain {
    /// Map of key ID to trusted key entry.
    keys: HashMap<String, TrustedKey>,
    /// Root keys (ultimate trust).
    root_keys: Vec<String>,
    /// Maximum chain depth for trust propagation.
    max_chain_depth: usize,
}

impl TrustChain {
    /// Create a new trust chain validator.
    #[must_use]
    pub fn new() -> Self {
        Self {
            keys: HashMap::new(),
            root_keys: Vec::new(),
            max_chain_depth: 5,
        }
    }

    /// Set maximum chain depth.
    #[must_use]
    pub const fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_chain_depth = depth;
        self
    }

    /// Add a root key with ultimate trust.
    pub fn add_root_key(&mut self, key_id: impl Into<String>, name: Option<&str>) {
        let key_id = key_id.into();
        let mut key = TrustedKey::new(&key_id, TrustLevel::Ultimate);
        if let Some(n) = name {
            key = key.with_name(n);
        }
        self.keys.insert(key_id.clone(), key);
        self.root_keys.push(key_id);
    }

    /// Add a trusted key.
    pub fn add_key(&mut self, key: TrustedKey) {
        self.keys.insert(key.key_id.clone(), key);
    }

    /// Get trust level for a key.
    #[must_use]
    pub fn get_trust_level(&self, key_id: &str) -> TrustLevel {
        self.keys
            .get(key_id)
            .filter(|k| !k.is_expired())
            .map_or(TrustLevel::Unknown, |k| k.trust_level)
    }

    /// Check if a key is trusted (at least marginally).
    #[must_use]
    pub fn is_trusted(&self, key_id: &str) -> bool {
        self.get_trust_level(key_id) >= TrustLevel::Marginal
    }

    /// Validate a signature against the trust chain.
    ///
    /// Returns the computed trust level based on the signer key.
    #[must_use]
    pub fn validate_signature(&self, verified: &VerifiedSignature) -> TrustLevel {
        // Check direct trust
        if let Some(key) = self.keys.get(&verified.key_id)
            && key.is_valid()
        {
            return key.trust_level;
        }

        // Check web of trust - see if any trusted key signed this key
        self.compute_chain_trust(&verified.key_id, 0)
    }

    /// Compute trust level through the chain.
    fn compute_chain_trust(&self, key_id: &str, depth: usize) -> TrustLevel {
        if depth >= self.max_chain_depth {
            return TrustLevel::Unknown;
        }

        if let Some(key) = self.keys.get(key_id) {
            if key.is_expired() {
                return TrustLevel::Unknown;
            }

            // Direct trust
            if key.trust_level >= TrustLevel::Marginal {
                return key.trust_level;
            }

            // Check signers (web of trust)
            let mut max_trust = TrustLevel::Unknown;
            for signer_id in &key.signed_by {
                let signer_trust = self.compute_chain_trust(signer_id, depth + 1);
                // Trust degrades as it propagates
                let derived_trust = match signer_trust {
                    TrustLevel::Ultimate => TrustLevel::Full,
                    TrustLevel::Full => TrustLevel::Marginal,
                    _ => TrustLevel::Unknown,
                };
                if derived_trust > max_trust {
                    max_trust = derived_trust;
                }
            }
            return max_trust;
        }

        TrustLevel::Unknown
    }

    /// Record that one key has signed another.
    #[allow(clippy::similar_names)] // signed_key_id and signer_key_id are semantically different
    pub fn add_key_signature(&mut self, signed_key_id: &str, signer_key_id: &str) {
        if let Some(key) = self.keys.get_mut(signed_key_id)
            && !key.signed_by.contains(&signer_key_id.to_string())
        {
            key.signed_by.push(signer_key_id.to_string());
        }
    }

    /// Get all trusted keys.
    #[must_use]
    pub fn trusted_keys(&self) -> Vec<&TrustedKey> {
        self.keys.values().filter(|k| k.is_valid()).collect()
    }

    /// Remove expired keys from the trust store.
    pub fn prune_expired(&mut self) {
        self.keys.retain(|_, v| !v.is_expired());
        self.root_keys.retain(|id| self.keys.contains_key(id));
    }
}

/// Extended signature verifier with trust chain support.
#[derive(Debug)]
pub struct TrustedSignatureVerifier {
    verifier: SignatureVerifier,
    trust_chain: TrustChain,
}

impl TrustedSignatureVerifier {
    /// Create a new trusted signature verifier.
    #[must_use]
    pub fn new() -> Self {
        Self {
            verifier: SignatureVerifier::new(),
            trust_chain: TrustChain::new(),
        }
    }

    /// Get mutable reference to trust chain.
    #[must_use]
    pub const fn trust_chain_mut(&mut self) -> &mut TrustChain {
        &mut self.trust_chain
    }

    /// Get reference to trust chain.
    #[must_use]
    pub const fn trust_chain(&self) -> &TrustChain {
        &self.trust_chain
    }

    /// Add a PGP key and register it in the trust chain.
    ///
    /// # Errors
    /// Returns error if key cannot be parsed.
    pub fn add_pgp_key(&mut self, key_data: &[u8], trust_level: TrustLevel) -> Result<String> {
        self.verifier.add_pgp_key(key_data)?;

        // Extract key ID from the certificate
        let cert =
            Cert::from_bytes(key_data).map_err(|e| SignatureError::InvalidKey(e.to_string()))?;
        let key_id = cert.fingerprint().to_hex();

        let name = cert
            .userids()
            .next()
            .map(|uid| String::from_utf8_lossy(uid.userid().value()).to_string());

        let mut trusted_key = TrustedKey::new(&key_id, trust_level);
        if let Some(n) = name {
            trusted_key = trusted_key.with_name(n);
        }

        self.trust_chain.add_key(trusted_key);

        Ok(key_id)
    }

    /// Add an Ed25519 key and register it in the trust chain.
    ///
    /// # Errors
    /// Returns error if key is invalid.
    pub fn add_ed25519_key(&mut self, key_bytes: &[u8], trust_level: TrustLevel) -> Result<String> {
        self.verifier.add_ed25519_key(key_bytes)?;

        let key_id = hex::encode(key_bytes);
        let trusted_key = TrustedKey::new(&key_id, trust_level);
        self.trust_chain.add_key(trusted_key);

        Ok(key_id)
    }

    /// Verify signature and return trust level.
    ///
    /// # Errors
    /// Returns error if verification fails.
    pub fn verify_with_trust(
        &self,
        data: &[u8],
        signature: &[u8],
    ) -> Result<(VerifiedSignature, TrustLevel)> {
        let verified = self.verifier.verify(data, signature)?;
        let trust_level = self.trust_chain.validate_signature(&verified);
        Ok((verified, trust_level))
    }

    /// Verify signature and ensure minimum trust level.
    ///
    /// # Errors
    /// Returns error if verification fails or trust level is insufficient.
    pub fn verify_with_min_trust(
        &self,
        data: &[u8],
        signature: &[u8],
        min_trust: TrustLevel,
    ) -> Result<VerifiedSignature> {
        let (verified, trust_level) = self.verify_with_trust(data, signature)?;

        if trust_level < min_trust {
            return Err(SignatureError::VerificationFailed(format!(
                "insufficient trust level: {trust_level:?} < {min_trust:?}"
            )));
        }

        Ok(verified)
    }
}

impl Default for TrustedSignatureVerifier {
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
