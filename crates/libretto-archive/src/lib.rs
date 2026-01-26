//! Archive extraction for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use flate2::read::GzDecoder;
use libretto_core::{Error, Result};
use std::fs::File;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use tracing::{debug, info};
use walkdir::WalkDir;

/// Supported archive types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveType {
    /// ZIP archive.
    Zip,
    /// Gzipped tarball.
    TarGz,
    /// Plain tarball.
    Tar,
}

impl ArchiveType {
    /// Detect archive type from path extension.
    #[must_use]
    pub fn from_path(path: &Path) -> Option<Self> {
        let name = path.file_name()?.to_str()?;
        Self::from_filename(name)
    }

    /// Detect archive type from filename.
    #[must_use]
    pub fn from_filename(name: &str) -> Option<Self> {
        let lower = name.to_lowercase();
        if lower.ends_with(".zip") {
            Some(Self::Zip)
        } else if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
            Some(Self::TarGz)
        } else if lower.ends_with(".tar") {
            Some(Self::Tar)
        } else {
            None
        }
    }

    /// Get file extension.
    #[must_use]
    pub const fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::TarGz => "tar.gz",
            Self::Tar => "tar",
        }
    }
}

/// Archive extractor.
#[derive(Debug, Default)]
pub struct Extractor {
    strip_prefix: Option<usize>,
}

impl Extractor {
    /// Create new extractor.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Strip N path components from extracted files.
    #[must_use]
    pub const fn strip_prefix(mut self, components: usize) -> Self {
        self.strip_prefix = Some(components);
        self
    }

    /// Extract archive to directory.
    ///
    /// # Errors
    /// Returns error if extraction fails.
    pub fn extract(&self, archive: &Path, dest: &Path) -> Result<ExtractionResult> {
        let archive_type = ArchiveType::from_path(archive).ok_or_else(|| {
            Error::Archive(format!("unknown archive type: {}", archive.display()))
        })?;

        debug!(archive = ?archive, dest = ?dest, "extracting");

        std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;

        let result = match archive_type {
            ArchiveType::Zip => self.extract_zip(archive, dest)?,
            ArchiveType::TarGz => self.extract_tar_gz(archive, dest)?,
            ArchiveType::Tar => self.extract_tar(archive, dest)?,
        };

        info!(
            files = result.files_extracted,
            size = result.total_size,
            "extraction complete"
        );

        Ok(result)
    }

    fn extract_zip(&self, archive: &Path, dest: &Path) -> Result<ExtractionResult> {
        let file = File::open(archive).map_err(|e| Error::io(archive, e))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| Error::Archive(e.to_string()))?;

        let mut files_extracted = 0;
        let mut total_size = 0u64;

        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| Error::Archive(e.to_string()))?;

            let path = match entry.enclosed_name() {
                Some(p) => p.to_path_buf(),
                None => continue,
            };

            let out_path = self.apply_strip_prefix(&path, dest);
            if out_path == dest {
                continue;
            }

            if entry.is_dir() {
                std::fs::create_dir_all(&out_path).map_err(|e| Error::io(&out_path, e))?;
            } else {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                }

                let mut out_file = File::create(&out_path).map_err(|e| Error::io(&out_path, e))?;
                let size = std::io::copy(&mut entry, &mut out_file)
                    .map_err(|e| Error::io(&out_path, e))?;

                files_extracted += 1;
                total_size += size;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Some(mode) = entry.unix_mode() {
                        std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))
                            .ok();
                    }
                }
            }
        }

        Ok(ExtractionResult {
            files_extracted,
            total_size,
            root_dir: find_root_dir(dest),
        })
    }

    fn extract_tar_gz(&self, archive: &Path, dest: &Path) -> Result<ExtractionResult> {
        let file = File::open(archive).map_err(|e| Error::io(archive, e))?;
        let decoder = GzDecoder::new(file);
        self.extract_tar_reader(decoder, dest)
    }

    fn extract_tar(&self, archive: &Path, dest: &Path) -> Result<ExtractionResult> {
        let file = File::open(archive).map_err(|e| Error::io(archive, e))?;
        self.extract_tar_reader(file, dest)
    }

    fn extract_tar_reader<R: Read>(&self, reader: R, dest: &Path) -> Result<ExtractionResult> {
        let mut archive = tar::Archive::new(reader);

        let mut files_extracted = 0;
        let mut total_size = 0u64;

        for entry in archive
            .entries()
            .map_err(|e| Error::Archive(e.to_string()))?
        {
            let mut entry = entry.map_err(|e| Error::Archive(e.to_string()))?;
            let path = entry
                .path()
                .map_err(|e| Error::Archive(e.to_string()))?
                .into_owned();

            let out_path = self.apply_strip_prefix(&path, dest);
            if out_path == dest {
                continue;
            }

            let entry_type = entry.header().entry_type();

            if entry_type.is_dir() {
                std::fs::create_dir_all(&out_path).map_err(|e| Error::io(&out_path, e))?;
            } else if entry_type.is_file() {
                if let Some(parent) = out_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                }

                let mut out_file = File::create(&out_path).map_err(|e| Error::io(&out_path, e))?;
                let size = std::io::copy(&mut entry, &mut out_file)
                    .map_err(|e| Error::io(&out_path, e))?;

                files_extracted += 1;
                total_size += size;

                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if let Ok(mode) = entry.header().mode() {
                        std::fs::set_permissions(&out_path, std::fs::Permissions::from_mode(mode))
                            .ok();
                    }
                }
            }
        }

        Ok(ExtractionResult {
            files_extracted,
            total_size,
            root_dir: find_root_dir(dest),
        })
    }

    fn apply_strip_prefix(&self, path: &Path, dest: &Path) -> PathBuf {
        if let Some(n) = self.strip_prefix {
            let components: Vec<_> = path.components().skip(n).collect();
            if components.is_empty() {
                return dest.to_path_buf();
            }
            dest.join(components.iter().collect::<PathBuf>())
        } else {
            dest.join(path)
        }
    }
}

/// Extraction result.
#[derive(Debug)]
pub struct ExtractionResult {
    /// Number of files extracted.
    pub files_extracted: usize,
    /// Total size in bytes.
    pub total_size: u64,
    /// Detected root directory (if single root).
    pub root_dir: Option<PathBuf>,
}

fn find_root_dir(dest: &Path) -> Option<PathBuf> {
    let entries: Vec<_> = WalkDir::new(dest)
        .min_depth(1)
        .max_depth(1)
        .into_iter()
        .filter_map(std::result::Result::ok)
        .collect();

    if entries.len() == 1 && entries[0].file_type().is_dir() {
        Some(entries[0].path().to_path_buf())
    } else {
        None
    }
}

/// Create a ZIP archive.
///
/// # Errors
/// Returns error if archive creation fails.
pub fn create_zip<W: std::io::Write + Seek>(
    writer: W,
    source: &Path,
    prefix: Option<&str>,
) -> Result<()> {
    let mut zip = zip::ZipWriter::new(writer);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in WalkDir::new(source).min_depth(1) {
        let entry = entry.map_err(|e| Error::Archive(e.to_string()))?;
        let path = entry.path();
        let relative = path
            .strip_prefix(source)
            .map_err(|e| Error::Archive(e.to_string()))?;

        let name = if let Some(p) = prefix {
            PathBuf::from(p).join(relative)
        } else {
            relative.to_path_buf()
        };

        let name_str = name.to_string_lossy();

        if path.is_dir() {
            zip.add_directory(&*name_str, options)
                .map_err(|e| Error::Archive(e.to_string()))?;
        } else {
            zip.start_file(&*name_str, options)
                .map_err(|e| Error::Archive(e.to_string()))?;

            let mut file = File::open(path).map_err(|e| Error::io(path, e))?;
            std::io::copy(&mut file, &mut zip).map_err(|e| Error::Archive(e.to_string()))?;
        }
    }

    zip.finish().map_err(|e| Error::Archive(e.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_type_detection() {
        assert_eq!(
            ArchiveType::from_filename("package.zip"),
            Some(ArchiveType::Zip)
        );
        assert_eq!(
            ArchiveType::from_filename("package.tar.gz"),
            Some(ArchiveType::TarGz)
        );
        assert_eq!(
            ArchiveType::from_filename("package.tgz"),
            Some(ArchiveType::TarGz)
        );
        assert_eq!(
            ArchiveType::from_filename("package.tar"),
            Some(ArchiveType::Tar)
        );
        assert_eq!(ArchiveType::from_filename("package.rar"), None);
    }

    #[test]
    fn archive_extension() {
        assert_eq!(ArchiveType::Zip.extension(), "zip");
        assert_eq!(ArchiveType::TarGz.extension(), "tar.gz");
    }
}
