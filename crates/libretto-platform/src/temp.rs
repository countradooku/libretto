//! Temporary file and directory management with automatic cleanup.
//!
//! Provides:
//! - RAII-based temp file/directory management
//! - Cross-platform temp directory detection
//! - Automatic cleanup on drop
//! - Secure temp file creation

use crate::{PlatformError, Result};
use parking_lot::Mutex;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::{debug, warn};

/// Global temp file counter for unique names.
static TEMP_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Temporary file with automatic cleanup.
#[derive(Debug)]
pub struct TempFile {
    /// Path to the temp file.
    path: PathBuf,
    /// File handle (if open).
    file: Option<File>,
    /// Whether to delete on drop.
    delete_on_drop: bool,
    /// Whether the file was persisted.
    persisted: bool,
}

impl TempFile {
    /// Create a new temp file in the system temp directory.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn new() -> Result<Self> {
        Self::in_dir(std::env::temp_dir())
    }

    /// Create a new temp file with a specific prefix.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn with_prefix(prefix: &str) -> Result<Self> {
        Self::in_dir_with_prefix(std::env::temp_dir(), prefix)
    }

    /// Create a new temp file with a specific suffix (extension).
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn with_suffix(suffix: &str) -> Result<Self> {
        Self::in_dir_with_suffix(std::env::temp_dir(), suffix)
    }

    /// Create a new temp file in a specific directory.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn in_dir(dir: impl AsRef<Path>) -> Result<Self> {
        Self::in_dir_with_prefix(dir, "tmp")
    }

    /// Create a new temp file with prefix in a specific directory.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn in_dir_with_prefix(dir: impl AsRef<Path>, prefix: &str) -> Result<Self> {
        Self::create_temp_file(dir.as_ref(), prefix, "")
    }

    /// Create a new temp file with suffix in a specific directory.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn in_dir_with_suffix(dir: impl AsRef<Path>, suffix: &str) -> Result<Self> {
        Self::create_temp_file(dir.as_ref(), "tmp", suffix)
    }

    fn create_temp_file(dir: &Path, prefix: &str, suffix: &str) -> Result<Self> {
        // Ensure directory exists
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|e| PlatformError::io(dir, e))?;
        }

        // Generate unique name
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let filename = format!("{prefix}_{pid}_{timestamp}_{id}{suffix}");
        let path = dir.join(filename);

        // Create file with exclusive access
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| PlatformError::io(&path, e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // Set restrictive permissions (owner only)
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
                .map_err(|e| PlatformError::io(&path, e))?;
        }

        debug!(path = %path.display(), "Created temp file");

        Ok(Self {
            path,
            file: Some(file),
            delete_on_drop: true,
            persisted: false,
        })
    }

    /// Get the path to the temp file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Get the file handle for writing.
    #[must_use]
    pub fn file(&mut self) -> Option<&mut File> {
        self.file.as_mut()
    }

    /// Take ownership of the file handle.
    #[must_use]
    pub fn take_file(&mut self) -> Option<File> {
        self.file.take()
    }

    /// Write data to the temp file.
    ///
    /// # Errors
    /// Returns error if write fails.
    pub fn write_all(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref mut file) = self.file {
            file.write_all(data)
                .map_err(|e| PlatformError::io(&self.path, e))?;
            file.flush().map_err(|e| PlatformError::io(&self.path, e))?;
        } else {
            // Reopen file
            let mut file = OpenOptions::new()
                .write(true)
                .open(&self.path)
                .map_err(|e| PlatformError::io(&self.path, e))?;
            file.write_all(data)
                .map_err(|e| PlatformError::io(&self.path, e))?;
            file.flush().map_err(|e| PlatformError::io(&self.path, e))?;
        }
        Ok(())
    }

    /// Read the temp file contents.
    ///
    /// # Errors
    /// Returns error if read fails.
    pub fn read(&self) -> Result<Vec<u8>> {
        fs::read(&self.path).map_err(|e| PlatformError::io(&self.path, e))
    }

    /// Read the temp file as string.
    ///
    /// # Errors
    /// Returns error if read fails or content is not UTF-8.
    pub fn read_string(&self) -> Result<String> {
        fs::read_to_string(&self.path).map_err(|e| PlatformError::io(&self.path, e))
    }

    /// Persist the temp file (don't delete on drop).
    pub fn persist(&mut self) {
        self.persisted = true;
        self.delete_on_drop = false;
    }

    /// Persist and rename to a new location.
    ///
    /// # Errors
    /// Returns error if rename fails.
    pub fn persist_to(mut self, dest: impl AsRef<Path>) -> Result<PathBuf> {
        self.persisted = true;
        self.delete_on_drop = false;

        // Close file handle before rename
        drop(self.file.take());

        let dest = dest.as_ref();
        fs::rename(&self.path, dest).map_err(|e| PlatformError::io(&self.path, e))?;

        debug!(from = %self.path.display(), to = %dest.display(), "Persisted temp file");

        Ok(dest.to_path_buf())
    }

    /// Delete the temp file immediately.
    ///
    /// # Errors
    /// Returns error if deletion fails.
    pub fn delete(mut self) -> Result<()> {
        self.delete_on_drop = false;
        drop(self.file.take());

        if self.path.exists() {
            fs::remove_file(&self.path).map_err(|e| PlatformError::io(&self.path, e))?;
        }
        Ok(())
    }

    /// Keep the temp file (don't delete on drop), but mark as not persisted.
    pub fn keep(&mut self) {
        self.delete_on_drop = false;
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if self.delete_on_drop && !self.persisted {
            drop(self.file.take());
            if let Err(e) = fs::remove_file(&self.path) {
                if e.kind() != io::ErrorKind::NotFound {
                    warn!(path = %self.path.display(), error = %e, "Failed to delete temp file");
                }
            } else {
                debug!(path = %self.path.display(), "Deleted temp file");
            }
        }
    }
}

/// Temporary directory with automatic cleanup.
#[derive(Debug)]
pub struct TempDir {
    /// Path to the temp directory.
    path: PathBuf,
    /// Whether to delete on drop.
    delete_on_drop: bool,
    /// Whether the directory was persisted.
    persisted: bool,
}

impl TempDir {
    /// Create a new temp directory in the system temp directory.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn new() -> Result<Self> {
        Self::in_dir(std::env::temp_dir())
    }

    /// Create a new temp directory with a specific prefix.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn with_prefix(prefix: &str) -> Result<Self> {
        Self::in_dir_with_prefix(std::env::temp_dir(), prefix)
    }

    /// Create a new temp directory in a specific parent directory.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn in_dir(parent: impl AsRef<Path>) -> Result<Self> {
        Self::in_dir_with_prefix(parent, "tmp")
    }

    /// Create a new temp directory with prefix in a specific parent.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn in_dir_with_prefix(parent: impl AsRef<Path>, prefix: &str) -> Result<Self> {
        let parent = parent.as_ref();

        // Ensure parent exists
        if !parent.exists() {
            fs::create_dir_all(parent).map_err(|e| PlatformError::io(parent, e))?;
        }

        // Generate unique name
        let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
        let pid = std::process::id();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);

        let dirname = format!("{prefix}_{pid}_{timestamp}_{id}");
        let path = parent.join(dirname);

        // Create directory
        fs::create_dir(&path).map_err(|e| PlatformError::io(&path, e))?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))
                .map_err(|e| PlatformError::io(&path, e))?;
        }

        debug!(path = %path.display(), "Created temp directory");

        Ok(Self {
            path,
            delete_on_drop: true,
            persisted: false,
        })
    }

    /// Get the path to the temp directory.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Create a temp file within this directory.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn create_file(&self, name: &str) -> Result<TempFile> {
        let path = self.path.join(name);
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| PlatformError::io(&path, e))?;

        Ok(TempFile {
            path,
            file: Some(file),
            delete_on_drop: false, // Parent dir handles cleanup
            persisted: false,
        })
    }

    /// Create a subdirectory within this temp directory.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn create_dir(&self, name: &str) -> Result<PathBuf> {
        let path = self.path.join(name);
        fs::create_dir(&path).map_err(|e| PlatformError::io(&path, e))?;
        Ok(path)
    }

    /// Join a path to this temp directory.
    #[must_use]
    pub fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.path.join(path)
    }

    /// Persist the temp directory (don't delete on drop).
    pub fn persist(&mut self) {
        self.persisted = true;
        self.delete_on_drop = false;
    }

    /// Persist and rename to a new location.
    ///
    /// # Errors
    /// Returns error if rename fails.
    pub fn persist_to(mut self, dest: impl AsRef<Path>) -> Result<PathBuf> {
        self.persisted = true;
        self.delete_on_drop = false;

        let dest = dest.as_ref();
        fs::rename(&self.path, dest).map_err(|e| PlatformError::io(&self.path, e))?;

        debug!(from = %self.path.display(), to = %dest.display(), "Persisted temp directory");

        Ok(dest.to_path_buf())
    }

    /// Delete the temp directory immediately.
    ///
    /// # Errors
    /// Returns error if deletion fails.
    pub fn delete(mut self) -> Result<()> {
        self.delete_on_drop = false;

        if self.path.exists() {
            fs::remove_dir_all(&self.path).map_err(|e| PlatformError::io(&self.path, e))?;
        }
        Ok(())
    }

    /// Keep the temp directory (don't delete on drop).
    pub fn keep(&mut self) {
        self.delete_on_drop = false;
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        if self.delete_on_drop && !self.persisted {
            if let Err(e) = fs::remove_dir_all(&self.path) {
                if e.kind() != io::ErrorKind::NotFound {
                    warn!(path = %self.path.display(), error = %e, "Failed to delete temp directory");
                }
            } else {
                debug!(path = %self.path.display(), "Deleted temp directory");
            }
        }
    }
}

/// Global temp file manager for cleanup on exit.
#[derive(Debug)]
pub struct TempManager {
    /// Tracked temp files and directories.
    tracked: Mutex<Vec<PathBuf>>,
    /// Whether cleanup is enabled.
    cleanup_enabled: bool,
}

impl TempManager {
    /// Create a new temp manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tracked: Mutex::new(Vec::new()),
            cleanup_enabled: true,
        }
    }

    /// Get the global temp manager.
    #[must_use]
    pub fn global() -> &'static Self {
        static MANAGER: once_cell::sync::Lazy<TempManager> =
            once_cell::sync::Lazy::new(TempManager::new);
        &MANAGER
    }

    /// Create a tracked temp file.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn create_file(&self) -> Result<TempFile> {
        let file = TempFile::new()?;
        self.tracked.lock().push(file.path().to_path_buf());
        Ok(file)
    }

    /// Create a tracked temp file with prefix.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn create_file_with_prefix(&self, prefix: &str) -> Result<TempFile> {
        let file = TempFile::with_prefix(prefix)?;
        self.tracked.lock().push(file.path().to_path_buf());
        Ok(file)
    }

    /// Create a tracked temp directory.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn create_dir(&self) -> Result<TempDir> {
        let dir = TempDir::new()?;
        self.tracked.lock().push(dir.path().to_path_buf());
        Ok(dir)
    }

    /// Create a tracked temp directory with prefix.
    ///
    /// # Errors
    /// Returns error if directory cannot be created.
    pub fn create_dir_with_prefix(&self, prefix: &str) -> Result<TempDir> {
        let dir = TempDir::with_prefix(prefix)?;
        self.tracked.lock().push(dir.path().to_path_buf());
        Ok(dir)
    }

    /// Disable cleanup (useful for debugging).
    pub fn disable_cleanup(&mut self) {
        self.cleanup_enabled = false;
    }

    /// Enable cleanup.
    pub fn enable_cleanup(&mut self) {
        self.cleanup_enabled = true;
    }

    /// Clean up all tracked temp files and directories.
    pub fn cleanup(&self) {
        if !self.cleanup_enabled {
            return;
        }

        let paths = std::mem::take(&mut *self.tracked.lock());
        for path in paths {
            if path.exists() {
                let result = if path.is_dir() {
                    fs::remove_dir_all(&path)
                } else {
                    fs::remove_file(&path)
                };

                if let Err(e) = result {
                    if e.kind() != io::ErrorKind::NotFound {
                        warn!(path = %path.display(), error = %e, "Failed to clean up");
                    }
                }
            }
        }
    }

    /// Get count of tracked items.
    #[must_use]
    pub fn tracked_count(&self) -> usize {
        self.tracked.lock().len()
    }
}

impl Default for TempManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TempManager {
    fn drop(&mut self) {
        self.cleanup();
    }
}

/// Get the system temp directory, ensuring it exists.
///
/// # Errors
/// Returns error if temp directory cannot be determined or created.
pub fn temp_dir() -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| PlatformError::io(&dir, e))?;
    }
    Ok(dir)
}

/// Get a unique temp path (doesn't create the file).
#[must_use]
pub fn temp_path() -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("tmp_{pid}_{id}"))
}

/// Get a unique temp path with extension.
#[must_use]
pub fn temp_path_with_ext(ext: &str) -> PathBuf {
    let id = TEMP_COUNTER.fetch_add(1, Ordering::SeqCst);
    let pid = std::process::id();
    let ext = if ext.starts_with('.') {
        ext
    } else {
        &format!(".{ext}")
    };
    std::env::temp_dir().join(format!("tmp_{pid}_{id}{ext}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_file_creation() {
        let temp = TempFile::new().unwrap();
        assert!(temp.path().exists());
        let path = temp.path().to_path_buf();
        drop(temp);
        assert!(!path.exists());
    }

    #[test]
    fn temp_file_write_read() {
        let mut temp = TempFile::new().unwrap();
        temp.write_all(b"hello world").unwrap();

        let content = temp.read_string().unwrap();
        assert_eq!(content, "hello world");
    }

    #[test]
    fn temp_file_persist() {
        let mut temp = TempFile::new().unwrap();
        temp.persist();
        let path = temp.path().to_path_buf();
        drop(temp);
        assert!(path.exists());
        fs::remove_file(&path).unwrap();
    }

    #[test]
    fn temp_file_persist_to() {
        let mut temp = TempFile::new().unwrap();
        temp.write_all(b"test content").unwrap();

        let dest = temp_path_with_ext("txt");
        let result = temp.persist_to(&dest).unwrap();

        assert_eq!(result, dest);
        assert!(dest.exists());
        assert_eq!(fs::read_to_string(&dest).unwrap(), "test content");

        fs::remove_file(&dest).unwrap();
    }

    #[test]
    fn temp_file_with_prefix() {
        let temp = TempFile::with_prefix("myprefix").unwrap();
        let filename = temp.path().file_name().unwrap().to_string_lossy();
        assert!(filename.starts_with("myprefix"));
    }

    #[test]
    fn temp_file_with_suffix() {
        let temp = TempFile::with_suffix(".txt").unwrap();
        let filename = temp.path().file_name().unwrap().to_string_lossy();
        assert!(filename.ends_with(".txt"));
    }

    #[test]
    fn temp_dir_creation() {
        let temp = TempDir::new().unwrap();
        assert!(temp.path().exists());
        assert!(temp.path().is_dir());
        let path = temp.path().to_path_buf();
        drop(temp);
        assert!(!path.exists());
    }

    #[test]
    fn temp_dir_create_file() {
        let temp = TempDir::new().unwrap();
        let file = temp.create_file("test.txt").unwrap();
        assert!(file.path().exists());
        assert!(file.path().starts_with(temp.path()));
    }

    #[test]
    fn temp_dir_create_subdir() {
        let temp = TempDir::new().unwrap();
        let subdir = temp.create_dir("subdir").unwrap();
        assert!(subdir.exists());
        assert!(subdir.is_dir());
    }

    #[test]
    fn temp_dir_persist() {
        let mut temp = TempDir::new().unwrap();
        temp.persist();
        let path = temp.path().to_path_buf();
        drop(temp);
        assert!(path.exists());
        fs::remove_dir_all(&path).unwrap();
    }

    #[test]
    fn temp_manager_tracking() {
        let manager = TempManager::new();
        let _file1 = manager.create_file().unwrap();
        let _file2 = manager.create_file().unwrap();
        assert_eq!(manager.tracked_count(), 2);
    }

    #[test]
    fn temp_path_unique() {
        let path1 = temp_path();
        let path2 = temp_path();
        assert_ne!(path1, path2);
    }

    #[test]
    fn temp_path_with_extension() {
        let path = temp_path_with_ext("json");
        assert!(path.to_string_lossy().ends_with(".json"));
    }

    #[test]
    fn temp_dir_ensure_exists() {
        let dir = temp_dir().unwrap();
        assert!(dir.exists());
    }
}
