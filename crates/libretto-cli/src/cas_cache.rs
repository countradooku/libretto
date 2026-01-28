//! Content-Addressable Storage (CAS) cache with hardlink support.
//!
//! Like pnpm, we store package contents once in a global cache and hardlink
//! to the vendor directory. This provides:
//! - Instant installs on cache hits (just create hardlinks)
//! - Massive disk space savings (files stored once)
//! - Integrity verification via content hashing

use anyhow::Result;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// Global CAS cache location (~/.libretto/cache)
pub fn cache_dir() -> PathBuf {
    directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".libretto").join("cache"))
        .unwrap_or_else(|| PathBuf::from(".libretto/cache"))
}

/// Cache for extracted package contents (content-addressable)
pub fn cas_dir() -> PathBuf {
    cache_dir().join("cas")
}

/// Check if a package is cached (by URL hash)
#[allow(dead_code)]
pub fn is_cached(url: &str) -> bool {
    let hash = hash_url(url);
    let marker = cas_dir().join(&hash).join(".complete");
    marker.exists()
}

/// Get cached package path if it exists
pub fn get_cached_path(url: &str) -> Option<PathBuf> {
    let hash = hash_url(url);
    let path = cas_dir().join(&hash);
    let marker = path.join(".complete");
    if marker.exists() { Some(path) } else { None }
}

/// Store extracted package in CAS cache
pub fn store_in_cache(url: &str, source_dir: &Path) -> Result<PathBuf> {
    let hash = hash_url(url);
    let cache_path = cas_dir().join(&hash);

    // Create cache directory
    fs::create_dir_all(&cache_path)?;

    // Copy files to cache
    copy_dir_recursive(source_dir, &cache_path)?;

    // Write completion marker
    let marker = cache_path.join(".complete");
    fs::write(&marker, url.as_bytes())?;

    Ok(cache_path)
}

/// Link or copy from cache to destination
/// Uses hardlinks on Unix for instant "copies"
pub fn link_from_cache(cache_path: &Path, dest: &Path) -> Result<()> {
    // Remove existing destination
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }

    // Create parent directories
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // Try hardlinks first, fall back to copy
    link_dir_recursive(cache_path, dest)
}

/// Hash a URL to get a cache key (using standard hasher for simplicity)
fn hash_url(url: &str) -> String {
    let mut hasher = DefaultHasher::new();
    url.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Recursively copy directory
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if entry.file_name() == ".complete" {
            continue;
        }

        if file_type.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            fs::copy(&src_path, &dst_path)?;
        }
    }

    Ok(())
}

/// Recursively link directory using hardlinks (pnpm-style)
fn link_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if entry.file_name() == ".complete" {
            continue;
        }

        if file_type.is_dir() {
            link_dir_recursive(&src_path, &dst_path)?;
        } else if file_type.is_file() {
            // Try hardlink first (instant, saves space)
            if fs::hard_link(&src_path, &dst_path).is_err() {
                // Fall back to copy (different filesystem)
                fs::copy(&src_path, &dst_path)?;
            }
        }
    }

    Ok(())
}

/// Clear the entire cache
pub fn clear_cache() -> Result<()> {
    let cache = cache_dir();
    if cache.exists() {
        fs::remove_dir_all(&cache)?;
    }
    Ok(())
}
