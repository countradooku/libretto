//! Cross-platform file system abstractions.
//!
//! Provides:
//! - Cross-platform path handling
//! - Atomic file operations
//! - Symlink/hardlink/junction support
//! - Windows long path support
//! - Case sensitivity handling

#![allow(unsafe_code)]

use crate::{Platform, PlatformError, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::{debug, trace, warn};

/// Lock timeout for file operations.
const LOCK_TIMEOUT: Duration = Duration::from_secs(30);

/// Cross-platform path abstraction.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CrossPath {
    /// Internal path representation.
    inner: PathBuf,
    /// Whether this is a Windows long path.
    is_long_path: bool,
}

impl CrossPath {
    /// Create a new cross-platform path.
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        let path = path.as_ref();
        let (normalized, is_long) = Self::normalize_path(path);
        Self {
            inner: normalized,
            is_long_path: is_long,
        }
    }

    /// Create from a string, handling platform differences.
    #[must_use]
    pub fn from_string(path: &str) -> Self {
        // Normalize path separators
        let normalized = if cfg!(windows) {
            path.replace('/', "\\")
        } else {
            path.replace('\\', "/")
        };
        Self::new(&normalized)
    }

    /// Get the inner path.
    #[must_use]
    pub fn as_path(&self) -> &Path {
        &self.inner
    }

    /// Get as PathBuf.
    #[must_use]
    pub fn to_path_buf(&self) -> PathBuf {
        self.inner.clone()
    }

    /// Convert to Windows long path format if needed.
    #[must_use]
    pub fn to_windows_long_path(&self) -> PathBuf {
        #[cfg(windows)]
        {
            if self.is_long_path || self.inner.to_string_lossy().starts_with(r"\\?\") {
                self.inner.clone()
            } else if let Some(path_str) = self.inner.to_str() {
                if path_str.len() > 260 {
                    // Convert to long path format
                    let abs = if self.inner.is_absolute() {
                        self.inner.clone()
                    } else {
                        std::env::current_dir()
                            .map(|cwd| cwd.join(&self.inner))
                            .unwrap_or_else(|_| self.inner.clone())
                    };
                    PathBuf::from(format!(r"\\?\{}", abs.display()))
                } else {
                    self.inner.clone()
                }
            } else {
                self.inner.clone()
            }
        }
        #[cfg(not(windows))]
        {
            self.inner.clone()
        }
    }

    /// Check if this is an absolute path.
    #[must_use]
    pub fn is_absolute(&self) -> bool {
        self.inner.is_absolute()
    }

    /// Get the parent directory.
    #[must_use]
    pub fn parent(&self) -> Option<Self> {
        self.inner.parent().map(|p| Self::new(p))
    }

    /// Join with another path component.
    #[must_use]
    pub fn join(&self, path: impl AsRef<Path>) -> Self {
        Self::new(self.inner.join(path))
    }

    /// Get the file name.
    #[must_use]
    pub fn file_name(&self) -> Option<&std::ffi::OsStr> {
        self.inner.file_name()
    }

    /// Get the extension.
    #[must_use]
    pub fn extension(&self) -> Option<&std::ffi::OsStr> {
        self.inner.extension()
    }

    /// Check if path exists.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.inner.exists()
    }

    /// Check if path is a file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.inner.is_file()
    }

    /// Check if path is a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.inner.is_dir()
    }

    /// Check if path is a symlink.
    #[must_use]
    pub fn is_symlink(&self) -> bool {
        self.inner
            .symlink_metadata()
            .map(|m| m.is_symlink())
            .unwrap_or(false)
    }

    /// Canonicalize the path.
    ///
    /// # Errors
    /// Returns error if path cannot be canonicalized.
    pub fn canonicalize(&self) -> Result<Self> {
        self.inner
            .canonicalize()
            .map(Self::new)
            .map_err(|e| PlatformError::io(&self.inner, e))
    }

    fn normalize_path(path: &Path) -> (PathBuf, bool) {
        let path_str = path.to_string_lossy();

        #[cfg(windows)]
        {
            // Check for Windows long path prefix
            let is_long = path_str.starts_with(r"\\?\") || path_str.starts_with(r"\\.\");

            // Normalize separators
            let normalized: String = path_str
                .chars()
                .map(|c| if c == '/' { '\\' } else { c })
                .collect();

            (PathBuf::from(normalized), is_long)
        }

        #[cfg(not(windows))]
        {
            // On Unix, just convert Windows separators
            let normalized: String = path_str
                .chars()
                .map(|c| if c == '\\' { '/' } else { c })
                .collect();

            (PathBuf::from(normalized), false)
        }
    }
}

impl AsRef<Path> for CrossPath {
    fn as_ref(&self) -> &Path {
        &self.inner
    }
}

impl From<PathBuf> for CrossPath {
    fn from(path: PathBuf) -> Self {
        Self::new(path)
    }
}

impl From<&Path> for CrossPath {
    fn from(path: &Path) -> Self {
        Self::new(path)
    }
}

impl From<String> for CrossPath {
    fn from(path: String) -> Self {
        Self::from_string(&path)
    }
}

impl From<&str> for CrossPath {
    fn from(path: &str) -> Self {
        Self::from_string(path)
    }
}

impl std::fmt::Display for CrossPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.inner.display())
    }
}

/// Link types for file system links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LinkType {
    /// Symbolic link (Unix/Windows).
    Symbolic,
    /// Hard link (Unix/Windows).
    Hard,
    /// Junction (Windows only).
    Junction,
}

impl LinkType {
    /// Check if this link type is supported on the current platform.
    #[must_use]
    pub fn is_supported(&self) -> bool {
        match self {
            Self::Symbolic | Self::Hard => true,
            Self::Junction => cfg!(windows),
        }
    }
}

/// Atomic file writer with crash-safe guarantees.
///
/// Uses temp file + rename pattern for atomic writes.
#[derive(Debug)]
pub struct AtomicFile {
    /// Target path.
    target: PathBuf,
    /// Temp file path.
    temp_path: PathBuf,
    /// Lock file path (kept for cleanup in Drop).
    #[allow(dead_code)]
    lock_path: PathBuf,
    /// Lock file handle (RAII: lock released when dropped).
    #[allow(dead_code)]
    lock_handle: Option<File>,
    /// Whether to create backup.
    create_backup: bool,
    /// Backup path.
    backup_path: PathBuf,
}

impl AtomicFile {
    /// Create a new atomic file writer.
    ///
    /// # Errors
    /// Returns error if lock cannot be acquired.
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let target = path.as_ref().to_path_buf();
        let temp_path = Self::temp_path_for(&target);
        let lock_path = Self::lock_path_for(&target);
        let backup_path = Self::backup_path_for(&target);

        debug!(target = %target.display(), "Creating atomic file writer");

        // Ensure parent directory exists
        if let Some(parent) = target.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| PlatformError::io(parent, e))?;
            }
        }

        // Acquire exclusive lock
        let lock_handle = Self::acquire_lock(&lock_path)?;

        Ok(Self {
            target,
            temp_path,
            lock_path,
            lock_handle: Some(lock_handle),
            create_backup: true,
            backup_path,
        })
    }

    /// Disable backup creation.
    #[must_use]
    pub fn no_backup(mut self) -> Self {
        self.create_backup = false;
        self
    }

    /// Write content atomically.
    ///
    /// # Errors
    /// Returns error if write fails.
    pub fn write(&self, content: &[u8]) -> Result<AtomicWriteResult> {
        debug!(
            target = %self.target.display(),
            size = content.len(),
            "Writing atomic file"
        );

        // Write to temp file
        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&self.temp_path)
                .map_err(|e| PlatformError::io(&self.temp_path, e))?;

            file.write_all(content)
                .map_err(|e| PlatformError::io(&self.temp_path, e))?;

            // Sync to disk
            file.sync_all()
                .map_err(|e| PlatformError::io(&self.temp_path, e))?;
        }

        // Create backup if requested and target exists
        let had_existing = self.target.exists();
        if had_existing && self.create_backup {
            fs::copy(&self.target, &self.backup_path)
                .map_err(|e| PlatformError::io(&self.backup_path, e))?;
            trace!(backup = %self.backup_path.display(), "Created backup");
        }

        // Atomic rename
        fs::rename(&self.temp_path, &self.target)
            .map_err(|e| PlatformError::io(&self.target, e))?;

        // Sync parent directory on Unix
        #[cfg(unix)]
        if let Some(parent) = self.target.parent() {
            if let Ok(dir) = File::open(parent) {
                let _ = dir.sync_all();
            }
        }

        debug!(target = %self.target.display(), "Atomic write completed");

        Ok(AtomicWriteResult {
            path: self.target.clone(),
            bytes_written: content.len(),
            had_existing,
        })
    }

    /// Write from a reader atomically.
    ///
    /// # Errors
    /// Returns error if write fails.
    pub fn write_from<R: Read>(&self, mut reader: R) -> Result<AtomicWriteResult> {
        let mut content = Vec::new();
        reader
            .read_to_end(&mut content)
            .map_err(|e| PlatformError::Io {
                path: PathBuf::new(),
                message: e.to_string(),
            })?;
        self.write(&content)
    }

    fn temp_path_for(target: &Path) -> PathBuf {
        let mut temp = target.to_path_buf();
        let ext = temp
            .extension()
            .map(|e| format!("{}.tmp", e.to_string_lossy()))
            .unwrap_or_else(|| "tmp".to_string());
        temp.set_extension(ext);
        temp
    }

    fn lock_path_for(target: &Path) -> PathBuf {
        let mut lock = target.to_path_buf();
        let ext = lock
            .extension()
            .map(|e| format!("{}.lock", e.to_string_lossy()))
            .unwrap_or_else(|| "lock".to_string());
        lock.set_extension(ext);
        lock
    }

    fn backup_path_for(target: &Path) -> PathBuf {
        let mut backup = target.to_path_buf();
        let ext = backup
            .extension()
            .map(|e| format!("{}.bak", e.to_string_lossy()))
            .unwrap_or_else(|| "bak".to_string());
        backup.set_extension(ext);
        backup
    }

    fn acquire_lock(path: &Path) -> Result<File> {
        use fs2::FileExt;

        // Ensure parent exists
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| PlatformError::io(parent, e))?;
            }
        }

        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| PlatformError::io(path, e))?;

        let start = std::time::Instant::now();
        loop {
            match file.try_lock_exclusive() {
                Ok(()) => {
                    debug!(path = %path.display(), "Acquired exclusive lock");
                    return Ok(file);
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if start.elapsed() > LOCK_TIMEOUT {
                        return Err(PlatformError::LockTimeout {
                            path: path.to_path_buf(),
                            timeout_secs: LOCK_TIMEOUT.as_secs(),
                        });
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(e) => return Err(PlatformError::io(path, e)),
            }
        }
    }
}

impl Drop for AtomicFile {
    fn drop(&mut self) {
        // Clean up temp file if exists
        if self.temp_path.exists() {
            warn!(temp = %self.temp_path.display(), "Cleaning up orphaned temp file");
            let _ = fs::remove_file(&self.temp_path);
        }
        // Lock released when handle is dropped
    }
}

/// Result of an atomic write operation.
#[derive(Debug)]
pub struct AtomicWriteResult {
    /// Path that was written.
    pub path: PathBuf,
    /// Bytes written.
    pub bytes_written: usize,
    /// Whether there was an existing file.
    pub had_existing: bool,
}

/// File system operations trait.
pub trait FileSystemOps {
    /// Create a symbolic link.
    fn create_symlink(target: &Path, link: &Path) -> Result<()>;

    /// Create a hard link.
    fn create_hardlink(target: &Path, link: &Path) -> Result<()>;

    /// Read symlink target.
    fn read_link(path: &Path) -> Result<PathBuf>;

    /// Set file permissions.
    fn set_permissions(path: &Path, mode: u32) -> Result<()>;

    /// Get file permissions.
    fn get_permissions(path: &Path) -> Result<u32>;

    /// Copy file preserving metadata.
    fn copy_file(src: &Path, dst: &Path) -> Result<u64>;

    /// Move file (rename across filesystems).
    fn move_file(src: &Path, dst: &Path) -> Result<()>;

    /// Remove file or directory recursively.
    fn remove_all(path: &Path) -> Result<()>;
}

/// Platform-specific file system operations.
#[derive(Debug, Clone, Copy)]
pub struct PlatformFs;

impl FileSystemOps for PlatformFs {
    fn create_symlink(target: &Path, link: &Path) -> Result<()> {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).map_err(|e| PlatformError::Symlink {
                path: link.to_path_buf(),
                reason: e.to_string(),
            })
        }
        #[cfg(windows)]
        {
            if target.is_dir() {
                std::os::windows::fs::symlink_dir(target, link)
            } else {
                std::os::windows::fs::symlink_file(target, link)
            }
            .map_err(|e| PlatformError::Symlink {
                path: link.to_path_buf(),
                reason: e.to_string(),
            })
        }
    }

    fn create_hardlink(target: &Path, link: &Path) -> Result<()> {
        fs::hard_link(target, link).map_err(|e| PlatformError::Symlink {
            path: link.to_path_buf(),
            reason: e.to_string(),
        })
    }

    fn read_link(path: &Path) -> Result<PathBuf> {
        fs::read_link(path).map_err(|e| PlatformError::io(path, e))
    }

    #[cfg(unix)]
    fn set_permissions(path: &Path, mode: u32) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let permissions = fs::Permissions::from_mode(mode);
        fs::set_permissions(path, permissions).map_err(|e| PlatformError::io(path, e))
    }

    #[cfg(windows)]
    fn set_permissions(path: &Path, mode: u32) -> Result<()> {
        // Windows doesn't have Unix-style permissions
        // We can only set read-only flag
        let mut permissions = fs::metadata(path)
            .map_err(|e| PlatformError::io(path, e))?
            .permissions();

        // If no write bits are set (mode & 0o222 == 0), make read-only
        permissions.set_readonly((mode & 0o222) == 0);
        fs::set_permissions(path, permissions).map_err(|e| PlatformError::io(path, e))
    }

    #[cfg(unix)]
    fn get_permissions(path: &Path) -> Result<u32> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        Ok(metadata.permissions().mode())
    }

    #[cfg(windows)]
    fn get_permissions(path: &Path) -> Result<u32> {
        let metadata = fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        // Return Unix-like permissions based on read-only status
        if metadata.permissions().readonly() {
            Ok(0o444) // Read-only
        } else {
            Ok(0o644) // Read-write
        }
    }

    fn copy_file(src: &Path, dst: &Path) -> Result<u64> {
        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| PlatformError::io(parent, e))?;
            }
        }

        #[cfg(unix)]
        {
            // Use copy_file_range on Linux for zero-copy if available
            #[cfg(target_os = "linux")]
            {
                if let Ok(result) = Self::copy_file_range(src, dst) {
                    return Ok(result);
                }
            }
        }

        // Fallback to standard copy
        fs::copy(src, dst).map_err(|e| PlatformError::io(src, e))
    }

    fn move_file(src: &Path, dst: &Path) -> Result<()> {
        // Ensure parent directory exists
        if let Some(parent) = dst.parent() {
            if !parent.exists() {
                fs::create_dir_all(parent).map_err(|e| PlatformError::io(parent, e))?;
            }
        }

        // Try rename first (same filesystem)
        if fs::rename(src, dst).is_ok() {
            return Ok(());
        }

        // Cross-filesystem move: copy then delete
        Self::copy_file(src, dst)?;
        fs::remove_file(src).map_err(|e| PlatformError::io(src, e))?;
        Ok(())
    }

    fn remove_all(path: &Path) -> Result<()> {
        if !path.exists() {
            return Ok(());
        }

        if path.is_dir() {
            fs::remove_dir_all(path).map_err(|e| PlatformError::io(path, e))
        } else {
            fs::remove_file(path).map_err(|e| PlatformError::io(path, e))
        }
    }
}

#[cfg(target_os = "linux")]
impl PlatformFs {
    fn copy_file_range(src: &Path, dst: &Path) -> std::io::Result<u64> {
        use std::os::unix::io::AsRawFd;

        let src_file = File::open(src)?;
        let dst_file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(dst)?;

        let src_len = src_file.metadata()?.len();
        let mut copied = 0u64;

        while copied < src_len {
            let remaining = src_len - copied;
            let to_copy = remaining.min(1 << 30); // 1GB at a time

            let result = unsafe {
                libc::copy_file_range(
                    src_file.as_raw_fd(),
                    std::ptr::null_mut(),
                    dst_file.as_raw_fd(),
                    std::ptr::null_mut(),
                    to_copy as usize,
                    0,
                )
            };

            if result < 0 {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EXDEV) {
                    // Cross-device, fall back to regular copy
                    return Err(err);
                }
                return Err(err);
            }

            if result == 0 {
                break;
            }

            copied += result as u64;
        }

        Ok(copied)
    }
}

/// Windows-specific file operations.
#[cfg(windows)]
pub mod windows {
    use super::*;

    /// Create a junction (Windows directory symlink alternative).
    ///
    /// # Errors
    /// Returns error if junction cannot be created.
    pub fn create_junction(target: &Path, link: &Path) -> Result<()> {
        use std::process::Command;

        // Use mklink /J for junctions
        let output = Command::new("cmd")
            .args(["/c", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .output()
            .map_err(|e| PlatformError::spawn_failed("mklink", e.to_string()))?;

        if output.status.success() {
            Ok(())
        } else {
            Err(PlatformError::Symlink {
                path: link.to_path_buf(),
                reason: String::from_utf8_lossy(&output.stderr).to_string(),
            })
        }
    }

    /// Get Windows file attributes.
    pub fn get_file_attributes(path: &Path) -> Result<u32> {
        use std::os::windows::fs::MetadataExt;
        let metadata = fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        Ok(metadata.file_attributes())
    }

    /// Set Windows file attributes.
    pub fn set_file_attributes(path: &Path, attributes: u32) -> Result<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let path_wide: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let result = unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(
                path_wide.as_ptr(),
                attributes,
            )
        };

        if result != 0 {
            Ok(())
        } else {
            Err(PlatformError::io(path, std::io::Error::last_os_error()))
        }
    }

    /// File attribute constants.
    pub mod attributes {
        pub const READONLY: u32 = 0x1;
        pub const HIDDEN: u32 = 0x2;
        pub const SYSTEM: u32 = 0x4;
        pub const DIRECTORY: u32 = 0x10;
        pub const ARCHIVE: u32 = 0x20;
        pub const NORMAL: u32 = 0x80;
        pub const TEMPORARY: u32 = 0x100;
        pub const SPARSE_FILE: u32 = 0x200;
        pub const REPARSE_POINT: u32 = 0x400;
        pub const COMPRESSED: u32 = 0x800;
    }
}

/// Case-sensitive path comparison helper.
pub fn paths_equal(a: &Path, b: &Path, case_sensitive: bool) -> bool {
    if case_sensitive {
        a == b
    } else {
        let a_str = a.to_string_lossy().to_lowercase();
        let b_str = b.to_string_lossy().to_lowercase();
        a_str == b_str
    }
}

/// Normalize path for comparison.
#[must_use]
pub fn normalize_for_comparison(path: &Path) -> String {
    let platform = Platform::current();
    let path_str = path.to_string_lossy();

    // Normalize separators
    let normalized = if platform.is_windows() {
        path_str.replace('/', "\\")
    } else {
        path_str.replace('\\', "/")
    };

    // Handle case sensitivity
    if platform.case_sensitive_fs {
        normalized
    } else {
        normalized.to_lowercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn cross_path_creation() {
        let path = CrossPath::new("/test/path");
        assert!(path.is_absolute() || cfg!(windows));
    }

    #[test]
    fn cross_path_from_string() {
        let path = CrossPath::from_string("test/path/file.txt");
        assert!(path.file_name().is_some());
    }

    #[test]
    fn cross_path_join() {
        let base = CrossPath::new("/base");
        let joined = base.join("child");
        assert!(joined.to_string().contains("child"));
    }

    #[test]
    fn atomic_file_write() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        let writer = AtomicFile::new(&path).unwrap();
        let result = writer.write(b"hello world").unwrap();

        assert_eq!(result.bytes_written, 11);
        assert!(!result.had_existing);
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn atomic_file_overwrite() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("test.txt");

        fs::write(&path, "old content").unwrap();

        let writer = AtomicFile::new(&path).unwrap();
        let result = writer.write(b"new content").unwrap();

        assert!(result.had_existing);
        assert_eq!(fs::read_to_string(&path).unwrap(), "new content");
    }

    #[test]
    fn platform_fs_copy() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");

        fs::write(&src, "test content").unwrap();

        let copied = PlatformFs::copy_file(&src, &dst).unwrap();
        assert_eq!(copied, 12);
        assert_eq!(fs::read_to_string(&dst).unwrap(), "test content");
    }

    #[test]
    fn platform_fs_move() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");

        fs::write(&src, "test content").unwrap();

        PlatformFs::move_file(&src, &dst).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&dst).unwrap(), "test content");
    }

    #[test]
    fn paths_equal_case_sensitive() {
        let a = Path::new("/Test/Path");
        let b = Path::new("/test/path");

        assert!(!paths_equal(a, b, true));
        assert!(paths_equal(a, b, false));
    }

    #[test]
    fn link_type_support() {
        assert!(LinkType::Symbolic.is_supported());
        assert!(LinkType::Hard.is_supported());
        assert_eq!(LinkType::Junction.is_supported(), cfg!(windows));
    }
}
