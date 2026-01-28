//! Package integrity verification using multiple hash algorithms.

use blake3::Hasher as Blake3Hasher;
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest as Sha256Digest, Sha256};
use std::path::Path;
use subtle::ConstantTimeEq;
use thiserror::Error;
use tokio::io::AsyncReadExt;

/// Integrity verification error.
#[derive(Debug, Error)]
pub enum IntegrityError {
    /// Checksum mismatch.
    #[error("checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Expected checksum.
        expected: String,
        /// Actual checksum.
        actual: String,
    },

    /// I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Invalid checksum format.
    #[error("invalid checksum format: {0}")]
    InvalidChecksum(String),
}

/// Result type for integrity operations.
pub type Result<T> = std::result::Result<T, IntegrityError>;

/// Hash algorithm for integrity verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlgorithm {
    /// SHA-256 (recommended).
    Sha256,
    /// SHA-1 (legacy support).
    Sha1,
    /// BLAKE3 (fastest, SIMD-accelerated).
    Blake3,
}

/// Computed hash with algorithm.
#[derive(Debug, Clone)]
pub struct Hash {
    /// Algorithm used.
    pub algorithm: HashAlgorithm,
    /// Hex-encoded hash value.
    pub value: String,
}

impl Hash {
    /// Create new hash.
    #[must_use]
    pub fn new(algorithm: HashAlgorithm, value: String) -> Self {
        Self { algorithm, value }
    }

    /// Verify against expected hash (constant-time comparison).
    pub fn verify(&self, expected: &str) -> Result<()> {
        let expected_bytes = hex::decode(expected)
            .map_err(|_| IntegrityError::InvalidChecksum(format!("invalid hex: {expected}")))?;

        let actual_bytes = hex::decode(&self.value)
            .map_err(|_| IntegrityError::InvalidChecksum(format!("invalid hex: {}", self.value)))?;

        if expected_bytes.len() != actual_bytes.len() {
            return Err(IntegrityError::ChecksumMismatch {
                expected: expected.to_string(),
                actual: self.value.clone(),
            });
        }

        // Constant-time comparison to prevent timing attacks
        if bool::from(expected_bytes.ct_eq(&actual_bytes)) {
            Ok(())
        } else {
            Err(IntegrityError::ChecksumMismatch {
                expected: expected.to_string(),
                actual: self.value.clone(),
            })
        }
    }
}

/// Multi-algorithm hash computer.
#[derive(Debug, Default)]
pub struct IntegrityVerifier {
    sha256: Option<Sha256>,
    sha1: Option<Sha1>,
    blake3: Option<Blake3Hasher>,
}

impl IntegrityVerifier {
    /// Create new verifier with all algorithms.
    #[must_use]
    pub fn new() -> Self {
        Self {
            sha256: Some(Sha256::new()),
            sha1: Some(Sha1::new()),
            blake3: Some(Blake3Hasher::new()),
        }
    }

    /// Create verifier with specific algorithms.
    #[must_use]
    pub fn with_algorithms(algorithms: &[HashAlgorithm]) -> Self {
        let mut verifier = Self::default();
        for algo in algorithms {
            match algo {
                HashAlgorithm::Sha256 => verifier.sha256 = Some(Sha256::new()),
                HashAlgorithm::Sha1 => verifier.sha1 = Some(Sha1::new()),
                HashAlgorithm::Blake3 => verifier.blake3 = Some(Blake3Hasher::new()),
            }
        }
        verifier
    }

    /// Update all hashers with data.
    pub fn update(&mut self, data: &[u8]) {
        if let Some(ref mut h) = self.sha256 {
            Sha256Digest::update(h, data);
        }
        if let Some(ref mut h) = self.sha1 {
            Sha1Digest::update(h, data);
        }
        if let Some(ref mut h) = self.blake3 {
            h.update(data);
        }
    }

    /// Finalize and return all computed hashes.
    pub fn finalize(self) -> Vec<Hash> {
        let mut hashes = Vec::with_capacity(3);

        if let Some(h) = self.sha256 {
            hashes.push(Hash::new(HashAlgorithm::Sha256, hex::encode(h.finalize())));
        }

        if let Some(h) = self.sha1 {
            hashes.push(Hash::new(HashAlgorithm::Sha1, hex::encode(h.finalize())));
        }

        if let Some(h) = self.blake3 {
            hashes.push(Hash::new(
                HashAlgorithm::Blake3,
                hex::encode(h.finalize().as_bytes()),
            ));
        }

        hashes
    }
}

/// Compute hash of a file.
///
/// # Errors
/// Returns error if file cannot be read.
pub async fn hash_file(path: impl AsRef<Path>, algorithm: HashAlgorithm) -> Result<Hash> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut verifier = IntegrityVerifier::with_algorithms(&[algorithm]);

    let mut buffer = vec![0u8; 65536]; // 64 KB buffer
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        verifier.update(&buffer[..n]);
    }

    Ok(verifier
        .finalize()
        .into_iter()
        .next()
        .expect("hash must exist"))
}

/// Compute all hashes of a file.
///
/// # Errors
/// Returns error if file cannot be read.
pub async fn hash_file_all(path: impl AsRef<Path>) -> Result<Vec<Hash>> {
    let mut file = tokio::fs::File::open(path).await?;
    let mut verifier = IntegrityVerifier::new();

    let mut buffer = vec![0u8; 65536]; // 64 KB buffer
    loop {
        let n = file.read(&mut buffer).await?;
        if n == 0 {
            break;
        }
        verifier.update(&buffer[..n]);
    }

    Ok(verifier.finalize())
}

/// Verify file integrity against expected checksum.
///
/// # Errors
/// Returns error if verification fails.
pub async fn verify_file(
    path: impl AsRef<Path>,
    algorithm: HashAlgorithm,
    expected: &str,
) -> Result<()> {
    let hash = hash_file(path, algorithm).await?;
    hash.verify(expected)
}

/// Verify file with SHA-256 checksum.
///
/// # Errors
/// Returns error if verification fails.
pub async fn verify_sha256(path: impl AsRef<Path>, expected: &str) -> Result<()> {
    verify_file(path, HashAlgorithm::Sha256, expected).await
}

/// Verify file with SHA-1 checksum (legacy).
///
/// # Errors
/// Returns error if verification fails.
pub async fn verify_sha1(path: impl AsRef<Path>, expected: &str) -> Result<()> {
    verify_file(path, HashAlgorithm::Sha1, expected).await
}

/// Verify file with BLAKE3 checksum.
///
/// # Errors
/// Returns error if verification fails.
pub async fn verify_blake3(path: impl AsRef<Path>, expected: &str) -> Result<()> {
    verify_file(path, HashAlgorithm::Blake3, expected).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn test_sha256_hash() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        tokio::fs::write(&path, b"hello world").await.unwrap();

        let hash = hash_file(&path, HashAlgorithm::Sha256).await.unwrap();

        // echo -n "hello world" | sha256sum
        let expected = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        hash.verify(expected).unwrap();
    }

    #[tokio::test]
    async fn test_sha1_hash() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        tokio::fs::write(&path, b"hello world").await.unwrap();

        let hash = hash_file(&path, HashAlgorithm::Sha1).await.unwrap();

        // echo -n "hello world" | sha1sum
        let expected = "2aae6c35c94fcfb415dbe95f408b9ce91ee846ed";
        hash.verify(expected).unwrap();
    }

    #[tokio::test]
    async fn test_blake3_hash() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        tokio::fs::write(&path, b"hello world").await.unwrap();

        let hash = hash_file(&path, HashAlgorithm::Blake3).await.unwrap();

        // echo -n "hello world" | b3sum
        let expected = "d74981efa70a0c880b8d8c1985d075dbcbf679b99a5f9914e5aaf96b831a9e24";
        hash.verify(expected).unwrap();
    }

    #[tokio::test]
    async fn test_checksum_mismatch() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        tokio::fs::write(&path, b"hello world").await.unwrap();

        let hash = hash_file(&path, HashAlgorithm::Sha256).await.unwrap();

        let result = hash.verify("deadbeef");
        assert!(matches!(
            result,
            Err(IntegrityError::ChecksumMismatch { .. })
        ));
    }

    #[tokio::test]
    async fn test_multi_hash() {
        let temp = NamedTempFile::new().unwrap();
        let path = temp.path().to_path_buf();
        tokio::fs::write(&path, b"test").await.unwrap();

        let hashes = hash_file_all(&path).await.unwrap();

        assert_eq!(hashes.len(), 3);
        assert!(hashes.iter().any(|h| h.algorithm == HashAlgorithm::Sha256));
        assert!(hashes.iter().any(|h| h.algorithm == HashAlgorithm::Sha1));
        assert!(hashes.iter().any(|h| h.algorithm == HashAlgorithm::Blake3));
    }

    #[test]
    fn test_constant_time_comparison() {
        let hash = Hash::new(
            HashAlgorithm::Sha256,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string(),
        );

        // Correct checksum
        assert!(
            hash.verify("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9")
                .is_ok()
        );

        // Wrong checksum
        assert!(
            hash.verify("b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde8")
                .is_err()
        );
    }
}
