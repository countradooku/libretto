//! High-performance JSON operations using sonic-rs.

use crate::{Error, Result};
use serde::{de::DeserializeOwned, Serialize};

/// Deserialize JSON string.
///
/// # Errors
/// Returns error if JSON is invalid.
pub fn from_json<T: DeserializeOwned>(s: &str) -> Result<T> {
    sonic_rs::from_str(s).map_err(Error::from)
}

/// Deserialize JSON bytes.
///
/// # Errors
/// Returns error if JSON is invalid.
pub fn from_json_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    sonic_rs::from_slice(bytes).map_err(Error::from)
}

/// Serialize to compact JSON.
///
/// # Errors
/// Returns error if serialization fails.
pub fn to_json<T: Serialize>(value: &T) -> Result<String> {
    sonic_rs::to_string(value).map_err(Error::from)
}

/// Serialize to pretty JSON.
///
/// # Errors
/// Returns error if serialization fails.
pub fn to_json_pretty<T: Serialize>(value: &T) -> Result<String> {
    sonic_rs::to_string_pretty(value).map_err(Error::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize, PartialEq)]
    struct Test {
        name: String,
        value: i32,
    }

    #[test]
    fn roundtrip() {
        let orig = Test {
            name: "test".into(),
            value: 42,
        };
        let json = to_json(&orig).unwrap();
        let parsed: Test = from_json(&json).unwrap();
        assert_eq!(orig, parsed);
    }

    #[test]
    fn pretty() {
        let val = Test {
            name: "x".into(),
            value: 1,
        };
        let pretty = to_json_pretty(&val).unwrap();
        assert!(pretty.contains('\n'));
    }
}
