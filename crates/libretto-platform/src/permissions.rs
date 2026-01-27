//! Platform-specific file permissions and attributes.
//!
//! Provides unified interface for:
//! - Unix: chmod, umask, file permissions (rwx)
//! - Windows: file attributes, ACLs

#![allow(unsafe_code)]

use crate::{PlatformError, Result};
use std::path::Path;

/// File permission mode (Unix-style).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FilePermissions {
    /// Permission bits (Unix-style, 0o755, etc.).
    mode: u32,
}

impl FilePermissions {
    /// Create from raw mode bits.
    #[must_use]
    pub const fn from_mode(mode: u32) -> Self {
        Self {
            mode: mode & 0o7777,
        }
    }

    /// Create from rwx flags.
    #[must_use]
    pub const fn from_rwx(owner: Rwx, group: Rwx, other: Rwx) -> Self {
        let mode = ((owner.0 as u32) << 6) | ((group.0 as u32) << 3) | (other.0 as u32);
        Self { mode }
    }

    /// Get raw mode bits.
    #[must_use]
    pub const fn mode(&self) -> u32 {
        self.mode
    }

    /// Check if owner can read.
    #[must_use]
    pub const fn owner_read(&self) -> bool {
        self.mode & 0o400 != 0
    }

    /// Check if owner can write.
    #[must_use]
    pub const fn owner_write(&self) -> bool {
        self.mode & 0o200 != 0
    }

    /// Check if owner can execute.
    #[must_use]
    pub const fn owner_execute(&self) -> bool {
        self.mode & 0o100 != 0
    }

    /// Check if group can read.
    #[must_use]
    pub const fn group_read(&self) -> bool {
        self.mode & 0o040 != 0
    }

    /// Check if group can write.
    #[must_use]
    pub const fn group_write(&self) -> bool {
        self.mode & 0o020 != 0
    }

    /// Check if group can execute.
    #[must_use]
    pub const fn group_execute(&self) -> bool {
        self.mode & 0o010 != 0
    }

    /// Check if others can read.
    #[must_use]
    pub const fn other_read(&self) -> bool {
        self.mode & 0o004 != 0
    }

    /// Check if others can write.
    #[must_use]
    pub const fn other_write(&self) -> bool {
        self.mode & 0o002 != 0
    }

    /// Check if others can execute.
    #[must_use]
    pub const fn other_execute(&self) -> bool {
        self.mode & 0o001 != 0
    }

    /// Check if setuid bit is set.
    #[must_use]
    pub const fn setuid(&self) -> bool {
        self.mode & 0o4000 != 0
    }

    /// Check if setgid bit is set.
    #[must_use]
    pub const fn setgid(&self) -> bool {
        self.mode & 0o2000 != 0
    }

    /// Check if sticky bit is set.
    #[must_use]
    pub const fn sticky(&self) -> bool {
        self.mode & 0o1000 != 0
    }

    /// Check if file is readable (by anyone).
    #[must_use]
    pub const fn is_readable(&self) -> bool {
        self.mode & 0o444 != 0
    }

    /// Check if file is writable (by anyone).
    #[must_use]
    pub const fn is_writable(&self) -> bool {
        self.mode & 0o222 != 0
    }

    /// Check if file is executable (by anyone).
    #[must_use]
    pub const fn is_executable(&self) -> bool {
        self.mode & 0o111 != 0
    }

    /// Common permission: readable by all, writable by owner (0o644).
    pub const FILE_DEFAULT: Self = Self::from_mode(0o644);

    /// Common permission: executable by all, writable by owner (0o755).
    pub const EXECUTABLE: Self = Self::from_mode(0o755);

    /// Common permission: directory default (0o755).
    pub const DIR_DEFAULT: Self = Self::from_mode(0o755);

    /// Common permission: private file (0o600).
    pub const PRIVATE: Self = Self::from_mode(0o600);

    /// Common permission: private directory (0o700).
    pub const PRIVATE_DIR: Self = Self::from_mode(0o700);

    /// Format as symbolic string (e.g., "rwxr-xr-x").
    #[must_use]
    pub fn to_symbolic(&self) -> String {
        let mut s = String::with_capacity(9);

        s.push(if self.owner_read() { 'r' } else { '-' });
        s.push(if self.owner_write() { 'w' } else { '-' });
        s.push(match (self.owner_execute(), self.setuid()) {
            (true, true) => 's',
            (true, false) => 'x',
            (false, true) => 'S',
            (false, false) => '-',
        });

        s.push(if self.group_read() { 'r' } else { '-' });
        s.push(if self.group_write() { 'w' } else { '-' });
        s.push(match (self.group_execute(), self.setgid()) {
            (true, true) => 's',
            (true, false) => 'x',
            (false, true) => 'S',
            (false, false) => '-',
        });

        s.push(if self.other_read() { 'r' } else { '-' });
        s.push(if self.other_write() { 'w' } else { '-' });
        s.push(match (self.other_execute(), self.sticky()) {
            (true, true) => 't',
            (true, false) => 'x',
            (false, true) => 'T',
            (false, false) => '-',
        });

        s
    }
}

impl std::fmt::Display for FilePermissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04o}", self.mode)
    }
}

impl Default for FilePermissions {
    fn default() -> Self {
        Self::FILE_DEFAULT
    }
}

/// Read-write-execute permission triplet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rwx(u8);

impl Rwx {
    /// No permissions.
    pub const NONE: Self = Self(0);
    /// Execute only.
    pub const X: Self = Self(1);
    /// Write only.
    pub const W: Self = Self(2);
    /// Write and execute.
    pub const WX: Self = Self(3);
    /// Read only.
    pub const R: Self = Self(4);
    /// Read and execute.
    pub const RX: Self = Self(5);
    /// Read and write.
    pub const RW: Self = Self(6);
    /// Read, write, and execute.
    pub const RWX: Self = Self(7);

    /// Create from individual flags.
    #[must_use]
    pub const fn new(read: bool, write: bool, execute: bool) -> Self {
        let mut bits = 0u8;
        if read {
            bits |= 4;
        }
        if write {
            bits |= 2;
        }
        if execute {
            bits |= 1;
        }
        Self(bits)
    }

    /// Get raw bits.
    #[must_use]
    pub const fn bits(&self) -> u8 {
        self.0
    }
}

/// Permission operations trait.
pub trait PermissionOps {
    /// Get file permissions.
    fn get_permissions(path: &Path) -> Result<FilePermissions>;

    /// Set file permissions.
    fn set_permissions(path: &Path, perm: FilePermissions) -> Result<()>;

    /// Make file executable.
    fn make_executable(path: &Path) -> Result<()>;

    /// Make file read-only.
    fn make_readonly(path: &Path) -> Result<()>;

    /// Get current umask.
    fn get_umask() -> u32;

    /// Set umask (returns old value).
    fn set_umask(mask: u32) -> u32;
}

/// Platform-specific permission operations.
#[derive(Debug, Clone, Copy)]
pub struct PlatformPermissions;

#[cfg(unix)]
impl PermissionOps for PlatformPermissions {
    fn get_permissions(path: &Path) -> Result<FilePermissions> {
        use std::os::unix::fs::PermissionsExt;
        let metadata = std::fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        Ok(FilePermissions::from_mode(metadata.permissions().mode()))
    }

    fn set_permissions(path: &Path, perm: FilePermissions) -> Result<()> {
        use std::os::unix::fs::PermissionsExt;
        let permissions = std::fs::Permissions::from_mode(perm.mode());
        std::fs::set_permissions(path, permissions).map_err(|e| PlatformError::io(path, e))
    }

    fn make_executable(path: &Path) -> Result<()> {
        let current = Self::get_permissions(path)?;
        // Add execute bit for all who can read
        let mut mode = current.mode();
        if mode & 0o400 != 0 {
            mode |= 0o100;
        }
        if mode & 0o040 != 0 {
            mode |= 0o010;
        }
        if mode & 0o004 != 0 {
            mode |= 0o001;
        }
        Self::set_permissions(path, FilePermissions::from_mode(mode))
    }

    fn make_readonly(path: &Path) -> Result<()> {
        let current = Self::get_permissions(path)?;
        // Remove all write bits
        let mode = current.mode() & !0o222;
        Self::set_permissions(path, FilePermissions::from_mode(mode))
    }

    fn get_umask() -> u32 {
        // Get current umask by setting and then restoring
        unsafe {
            let mask = libc::umask(0o022);
            libc::umask(mask);
            mask as u32
        }
    }

    fn set_umask(mask: u32) -> u32 {
        unsafe { libc::umask(mask as libc::mode_t) as u32 }
    }
}

#[cfg(windows)]
impl PermissionOps for PlatformPermissions {
    fn get_permissions(path: &Path) -> Result<FilePermissions> {
        let metadata = std::fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;

        // Windows only has read-only concept
        if metadata.permissions().readonly() {
            Ok(FilePermissions::from_mode(0o444))
        } else {
            Ok(FilePermissions::from_mode(0o644))
        }
    }

    fn set_permissions(path: &Path, perm: FilePermissions) -> Result<()> {
        let mut permissions = std::fs::metadata(path)
            .map_err(|e| PlatformError::io(path, e))?
            .permissions();

        // Set read-only if no write bits
        permissions.set_readonly(!perm.is_writable());
        std::fs::set_permissions(path, permissions).map_err(|e| PlatformError::io(path, e))
    }

    fn make_executable(path: &Path) -> Result<()> {
        // Windows doesn't have execute permission; executability is based on extension
        // Just verify the file exists
        if !path.exists() {
            return Err(PlatformError::not_found(path));
        }
        Ok(())
    }

    fn make_readonly(path: &Path) -> Result<()> {
        let mut permissions = std::fs::metadata(path)
            .map_err(|e| PlatformError::io(path, e))?
            .permissions();

        permissions.set_readonly(true);
        std::fs::set_permissions(path, permissions).map_err(|e| PlatformError::io(path, e))
    }

    fn get_umask() -> u32 {
        // Windows doesn't have umask
        0o022
    }

    fn set_umask(_mask: u32) -> u32 {
        // Windows doesn't have umask
        0o022
    }
}

/// Unix-specific user/group operations.
#[cfg(unix)]
pub mod unix {
    use super::*;

    /// Get the owner UID of a file.
    ///
    /// # Errors
    /// Returns error if metadata cannot be read.
    pub fn get_owner(path: &Path) -> Result<u32> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        Ok(metadata.uid())
    }

    /// Get the group GID of a file.
    ///
    /// # Errors
    /// Returns error if metadata cannot be read.
    pub fn get_group(path: &Path) -> Result<u32> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        Ok(metadata.gid())
    }

    /// Change owner of a file.
    ///
    /// # Errors
    /// Returns error if chown fails.
    pub fn chown(path: &Path, uid: Option<u32>, gid: Option<u32>) -> Result<()> {
        use nix::unistd::{chown as nix_chown, Gid, Uid};

        let uid = uid.map(Uid::from_raw);
        let gid = gid.map(Gid::from_raw);

        nix_chown(path, uid, gid)
            .map_err(|e| PlatformError::io(path, std::io::Error::from_raw_os_error(e as i32)))
    }

    /// Get current user's UID.
    #[must_use]
    pub fn getuid() -> u32 {
        nix::unistd::getuid().as_raw()
    }

    /// Get current user's effective UID.
    #[must_use]
    pub fn geteuid() -> u32 {
        nix::unistd::geteuid().as_raw()
    }

    /// Get current user's GID.
    #[must_use]
    pub fn getgid() -> u32 {
        nix::unistd::getgid().as_raw()
    }

    /// Get current user's effective GID.
    #[must_use]
    pub fn getegid() -> u32 {
        nix::unistd::getegid().as_raw()
    }

    /// Check if running as root.
    #[must_use]
    pub fn is_root() -> bool {
        geteuid() == 0
    }
}

/// Windows-specific file attributes.
#[cfg(windows)]
pub mod windows {
    use super::*;

    /// Windows file attributes.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FileAttributes(pub u32);

    impl FileAttributes {
        pub const READONLY: u32 = 0x1;
        pub const HIDDEN: u32 = 0x2;
        pub const SYSTEM: u32 = 0x4;
        pub const DIRECTORY: u32 = 0x10;
        pub const ARCHIVE: u32 = 0x20;
        pub const DEVICE: u32 = 0x40;
        pub const NORMAL: u32 = 0x80;
        pub const TEMPORARY: u32 = 0x100;
        pub const SPARSE_FILE: u32 = 0x200;
        pub const REPARSE_POINT: u32 = 0x400;
        pub const COMPRESSED: u32 = 0x800;
        pub const OFFLINE: u32 = 0x1000;
        pub const NOT_CONTENT_INDEXED: u32 = 0x2000;
        pub const ENCRYPTED: u32 = 0x4000;

        /// Create from raw attributes.
        #[must_use]
        pub const fn from_raw(attrs: u32) -> Self {
            Self(attrs)
        }

        /// Get raw attributes.
        #[must_use]
        pub const fn raw(&self) -> u32 {
            self.0
        }

        /// Check if read-only.
        #[must_use]
        pub const fn is_readonly(&self) -> bool {
            self.0 & Self::READONLY != 0
        }

        /// Check if hidden.
        #[must_use]
        pub const fn is_hidden(&self) -> bool {
            self.0 & Self::HIDDEN != 0
        }

        /// Check if system file.
        #[must_use]
        pub const fn is_system(&self) -> bool {
            self.0 & Self::SYSTEM != 0
        }

        /// Check if directory.
        #[must_use]
        pub const fn is_directory(&self) -> bool {
            self.0 & Self::DIRECTORY != 0
        }

        /// Check if archive flag is set.
        #[must_use]
        pub const fn is_archive(&self) -> bool {
            self.0 & Self::ARCHIVE != 0
        }

        /// Check if temporary.
        #[must_use]
        pub const fn is_temporary(&self) -> bool {
            self.0 & Self::TEMPORARY != 0
        }

        /// Check if compressed.
        #[must_use]
        pub const fn is_compressed(&self) -> bool {
            self.0 & Self::COMPRESSED != 0
        }

        /// Check if encrypted.
        #[must_use]
        pub const fn is_encrypted(&self) -> bool {
            self.0 & Self::ENCRYPTED != 0
        }
    }

    /// Get Windows file attributes.
    ///
    /// # Errors
    /// Returns error if attributes cannot be read.
    pub fn get_attributes(path: &Path) -> Result<FileAttributes> {
        use std::os::windows::fs::MetadataExt;
        let metadata = std::fs::metadata(path).map_err(|e| PlatformError::io(path, e))?;
        Ok(FileAttributes::from_raw(metadata.file_attributes()))
    }

    /// Set Windows file attributes.
    ///
    /// # Errors
    /// Returns error if attributes cannot be set.
    pub fn set_attributes(path: &Path, attrs: FileAttributes) -> Result<()> {
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;

        let path_wide: Vec<u16> = OsStr::new(path)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();

        let result = unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileAttributesW(path_wide.as_ptr(), attrs.0)
        };

        if result != 0 {
            Ok(())
        } else {
            Err(PlatformError::io(path, std::io::Error::last_os_error()))
        }
    }

    /// Set a file as hidden.
    ///
    /// # Errors
    /// Returns error if attribute cannot be set.
    pub fn set_hidden(path: &Path, hidden: bool) -> Result<()> {
        let mut attrs = get_attributes(path)?;
        if hidden {
            attrs.0 |= FileAttributes::HIDDEN;
        } else {
            attrs.0 &= !FileAttributes::HIDDEN;
        }
        set_attributes(path, attrs)
    }

    /// Set a file as system file.
    ///
    /// # Errors
    /// Returns error if attribute cannot be set.
    pub fn set_system(path: &Path, system: bool) -> Result<()> {
        let mut attrs = get_attributes(path)?;
        if system {
            attrs.0 |= FileAttributes::SYSTEM;
        } else {
            attrs.0 &= !FileAttributes::SYSTEM;
        }
        set_attributes(path, attrs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_from_mode() {
        let perm = FilePermissions::from_mode(0o755);
        assert!(perm.owner_read());
        assert!(perm.owner_write());
        assert!(perm.owner_execute());
        assert!(perm.group_read());
        assert!(!perm.group_write());
        assert!(perm.group_execute());
        assert!(perm.other_read());
        assert!(!perm.other_write());
        assert!(perm.other_execute());
    }

    #[test]
    fn permission_from_rwx() {
        let perm = FilePermissions::from_rwx(Rwx::RWX, Rwx::RX, Rwx::RX);
        assert_eq!(perm.mode(), 0o755);
    }

    #[test]
    fn permission_symbolic() {
        let perm = FilePermissions::from_mode(0o755);
        assert_eq!(perm.to_symbolic(), "rwxr-xr-x");

        let perm = FilePermissions::from_mode(0o644);
        assert_eq!(perm.to_symbolic(), "rw-r--r--");

        let perm = FilePermissions::from_mode(0o4755);
        assert_eq!(perm.to_symbolic(), "rwsr-xr-x");
    }

    #[test]
    fn permission_display() {
        let perm = FilePermissions::from_mode(0o755);
        assert_eq!(format!("{perm}"), "0755");
    }

    #[test]
    fn rwx_bits() {
        assert_eq!(Rwx::NONE.bits(), 0);
        assert_eq!(Rwx::R.bits(), 4);
        assert_eq!(Rwx::W.bits(), 2);
        assert_eq!(Rwx::X.bits(), 1);
        assert_eq!(Rwx::RWX.bits(), 7);
    }

    #[test]
    fn permission_constants() {
        assert_eq!(FilePermissions::FILE_DEFAULT.mode(), 0o644);
        assert_eq!(FilePermissions::EXECUTABLE.mode(), 0o755);
        assert_eq!(FilePermissions::PRIVATE.mode(), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn unix_umask() {
        let old = PlatformPermissions::get_umask();
        let returned = PlatformPermissions::set_umask(0o077);
        let current = PlatformPermissions::get_umask();
        PlatformPermissions::set_umask(old); // Restore

        assert_eq!(returned, old);
        assert_eq!(current, 0o077);
    }
}
