//! Content-addressable hashing using BLAKE3.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::io::Read;

/// A BLAKE3 content hash (32 bytes).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Hash bytes.
    #[must_use]
    pub fn from_bytes(data: &[u8]) -> Self {
        Self(*blake3::hash(data).as_bytes())
    }

    /// Hash from a reader.
    ///
    /// # Errors
    /// Returns IO error if reading fails.
    pub fn from_reader<R: Read>(mut reader: R) -> std::io::Result<Self> {
        let mut hasher = blake3::Hasher::new();
        let mut buf = [0u8; 16384];
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
        }
        Ok(Self(*hasher.finalize().as_bytes()))
    }

    /// Hash a file.
    ///
    /// # Errors
    /// Returns IO error if file cannot be read.
    pub fn from_file(path: impl AsRef<std::path::Path>) -> std::io::Result<Self> {
        let file = std::fs::File::open(path)?;
        Self::from_reader(std::io::BufReader::new(file))
    }

    /// Parse from hex string.
    #[must_use]
    pub fn from_hex(hex: &str) -> Option<Self> {
        if hex.len() != 64 {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
            let s = std::str::from_utf8(chunk).ok()?;
            bytes[i] = u8::from_str_radix(s, 16).ok()?;
        }
        Some(Self(bytes))
    }

    /// Convert to hex string.
    #[must_use]
    pub fn to_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut s = String::with_capacity(64);
        for byte in &self.0 {
            s.push(HEX[(byte >> 4) as usize] as char);
            s.push(HEX[(byte & 0x0f) as usize] as char);
        }
        s
    }

    /// Get raw bytes.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Short display (8 chars).
    #[must_use]
    pub fn short(&self) -> String {
        self.to_hex()[..8].to_string()
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentHash({})", self.short())
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Incremental hasher.
#[derive(Default)]
pub struct ContentHasher {
    inner: blake3::Hasher,
}

impl ContentHasher {
    /// Create new hasher.
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: blake3::Hasher::new(),
        }
    }

    /// Update with data.
    pub fn update(&mut self, data: &[u8]) {
        self.inner.update(data);
    }

    /// Finalize hash.
    #[must_use]
    pub fn finalize(self) -> ContentHash {
        ContentHash(*self.inner.finalize().as_bytes())
    }
}

impl fmt::Debug for ContentHasher {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContentHasher").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_consistency() {
        let h1 = ContentHash::from_bytes(b"test");
        let h2 = ContentHash::from_bytes(b"test");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hex_roundtrip() {
        let hash = ContentHash::from_bytes(b"test");
        let hex = hash.to_hex();
        let recovered = ContentHash::from_hex(&hex).unwrap();
        assert_eq!(hash, recovered);
    }

    #[test]
    fn incremental() {
        let direct = ContentHash::from_bytes(b"hello world");
        let mut hasher = ContentHasher::new();
        hasher.update(b"hello ");
        hasher.update(b"world");
        assert_eq!(direct, hasher.finalize());
    }
}
