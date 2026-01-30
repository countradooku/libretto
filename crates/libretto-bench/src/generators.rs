//! Data generators for benchmark test data.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Generator for temporary PHP project structures.
#[derive(Debug)]
pub struct PhpProjectGenerator {
    temp_dir: TempDir,
}

impl PhpProjectGenerator {
    /// Create a new project generator with a temporary directory.
    ///
    /// # Errors
    /// Returns error if temp directory cannot be created.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            temp_dir: TempDir::new()?,
        })
    }

    /// Get the project root path.
    #[must_use]
    pub fn root(&self) -> &Path {
        self.temp_dir.path()
    }

    /// Generate a project with the specified number of PHP files.
    ///
    /// # Errors
    /// Returns error if files cannot be created.
    pub fn generate_files(&self, num_files: usize) -> std::io::Result<Vec<PathBuf>> {
        let src_dir = self.temp_dir.path().join("src");
        fs::create_dir_all(&src_dir)?;

        let mut files = Vec::with_capacity(num_files);
        let namespaces = [
            "App",
            "App\\Models",
            "App\\Services",
            "App\\Controllers",
            "App\\Utils",
        ];

        for i in 0..num_files {
            let namespace = namespaces[i % namespaces.len()];
            let class_name = format!("Class{i}");
            let subdir = namespace.replace('\\', "/");
            let dir = src_dir.join(&subdir);
            fs::create_dir_all(&dir)?;

            let file_path = dir.join(format!("{class_name}.php"));
            let content = crate::fixtures::generate_php_class(namespace, &class_name);
            let mut file = File::create(&file_path)?;
            file.write_all(content.as_bytes())?;
            files.push(file_path);
        }

        Ok(files)
    }

    /// Generate composer.json for the project.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn generate_composer_json(&self, num_deps: usize) -> std::io::Result<PathBuf> {
        let path = self.temp_dir.path().join("composer.json");
        let content = crate::fixtures::generate_composer_json(num_deps);
        let mut file = File::create(&path)?;
        file.write_all(content.as_bytes())?;
        Ok(path)
    }

    /// Generate composer.lock for the project.
    ///
    /// # Errors
    /// Returns error if file cannot be created.
    pub fn generate_composer_lock(&self, num_packages: usize) -> std::io::Result<PathBuf> {
        let path = self.temp_dir.path().join("composer.lock");
        let content = crate::fixtures::generate_composer_lock(num_packages);
        let mut file = File::create(&path)?;
        file.write_all(content.as_bytes())?;
        Ok(path)
    }
}

/// Generator for test archives.
#[derive(Debug)]
pub struct ArchiveGenerator {
    temp_dir: TempDir,
}

impl ArchiveGenerator {
    /// Create a new archive generator.
    ///
    /// # Errors
    /// Returns error if temp directory cannot be created.
    pub fn new() -> std::io::Result<Self> {
        Ok(Self {
            temp_dir: TempDir::new()?,
        })
    }

    /// Generate a ZIP archive with random content.
    ///
    /// # Errors
    /// Returns error if archive cannot be created.
    pub fn generate_zip(
        &self,
        name: &str,
        num_files: usize,
        avg_file_size: usize,
    ) -> std::io::Result<PathBuf> {
        let archive_path = self.temp_dir.path().join(format!("{name}.zip"));
        let file = File::create(&archive_path)?;
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);

        for i in 0..num_files {
            let filename = format!("file{i}.txt");
            zip.start_file(&filename, options)?;
            let content = generate_random_content(avg_file_size);
            zip.write_all(&content)?;
        }

        zip.finish()?;
        Ok(archive_path)
    }

    /// Generate a tar.gz archive with random content.
    ///
    /// # Errors
    /// Returns error if archive cannot be created.
    pub fn generate_tar_gz(
        &self,
        name: &str,
        num_files: usize,
        avg_file_size: usize,
    ) -> std::io::Result<PathBuf> {
        use flate2::Compression;
        use flate2::write::GzEncoder;

        let archive_path = self.temp_dir.path().join(format!("{name}.tar.gz"));
        let file = File::create(&archive_path)?;
        let encoder = GzEncoder::new(file, Compression::default());
        let mut tar = tar::Builder::new(encoder);

        for i in 0..num_files {
            let filename = format!("file{i}.txt");
            let content = generate_random_content(avg_file_size);
            let mut header = tar::Header::new_gnu();
            header.set_size(content.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            tar.append_data(&mut header, &filename, content.as_slice())?;
        }

        tar.finish()?;
        Ok(archive_path)
    }

    /// Get the temporary directory path.
    #[must_use]
    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }
}

/// Generate random content for test files.
fn generate_random_content(size: usize) -> Vec<u8> {
    use rand::prelude::*;
    let mut rng = rand::rng();
    let mut content = vec![0u8; size];
    rng.fill_bytes(&mut content);
    content
}

/// Helper to create a tokio runtime for async benchmarks.
#[must_use]
pub fn create_runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("Failed to create tokio runtime")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_php_project_generator() {
        let generator = PhpProjectGenerator::new().unwrap();
        let files = generator.generate_files(10).unwrap();
        assert_eq!(files.len(), 10);
        for file in files {
            assert!(file.exists());
        }
    }

    #[test]
    fn test_archive_generator_zip() {
        let generator = ArchiveGenerator::new().unwrap();
        let path = generator.generate_zip("test", 5, 1024).unwrap();
        assert!(path.exists());
    }

    #[test]
    fn test_archive_generator_tar_gz() {
        let generator = ArchiveGenerator::new().unwrap();
        let path = generator.generate_tar_gz("test", 5, 1024).unwrap();
        assert!(path.exists());
    }
}
