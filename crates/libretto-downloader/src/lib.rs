//! Package downloading for Libretto.

#![deny(clippy::all)]
#![allow(clippy::module_name_repetitions)]

use bytes::Bytes;
use futures::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use libretto_core::{ContentHash, Error, Result};
use reqwest::Client;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};
use url::Url;

/// Download options.
#[derive(Debug, Clone)]
pub struct DownloadOptions {
    /// Connection timeout.
    pub connect_timeout: Duration,
    /// Read timeout.
    pub read_timeout: Duration,
    /// Number of retries.
    pub retries: u32,
    /// Show progress bar.
    pub show_progress: bool,
    /// Verify checksum.
    pub verify_checksum: bool,
}

impl Default for DownloadOptions {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(10),
            read_timeout: Duration::from_secs(60),
            retries: 3,
            show_progress: true,
            verify_checksum: true,
        }
    }
}

/// Download result.
#[derive(Debug)]
pub struct DownloadResult {
    /// Path to downloaded file.
    pub path: PathBuf,
    /// Content hash.
    pub hash: ContentHash,
    /// Size in bytes.
    pub size: u64,
}

/// HTTP downloader with progress and retry support.
#[derive(Debug)]
pub struct Downloader {
    client: Client,
    options: DownloadOptions,
}

impl Downloader {
    /// Create new downloader.
    ///
    /// # Errors
    /// Returns error if HTTP client cannot be created.
    pub fn new(options: DownloadOptions) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(options.connect_timeout)
            .timeout(options.read_timeout)
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .build()
            .map_err(|e| Error::Network(e.to_string()))?;

        Ok(Self { client, options })
    }

    /// Create with default options.
    ///
    /// # Errors
    /// Returns error if HTTP client cannot be created.
    pub fn with_defaults() -> Result<Self> {
        Self::new(DownloadOptions::default())
    }

    /// Download URL to file.
    ///
    /// # Errors
    /// Returns error if download fails.
    pub async fn download(&self, url: &Url, dest: &Path) -> Result<DownloadResult> {
        let mut last_error = None;

        for attempt in 0..=self.options.retries {
            if attempt > 0 {
                debug!(attempt, url = %url, "retrying download");
                tokio::time::sleep(Duration::from_secs(u64::from(attempt))).await;
            }

            match self.try_download(url, dest).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    debug!(error = %e, attempt, "download failed");
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| Error::Network("download failed".into())))
    }

    /// Download to bytes in memory.
    ///
    /// # Errors
    /// Returns error if download fails.
    pub async fn download_bytes(&self, url: &Url) -> Result<Bytes> {
        let response = self
            .client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(Error::Network(format!("HTTP {}", response.status())));
        }

        response
            .bytes()
            .await
            .map_err(|e| Error::Network(e.to_string()))
    }

    async fn try_download(&self, url: &Url, dest: &Path) -> Result<DownloadResult> {
        debug!(url = %url, dest = ?dest, "downloading");

        let response = self
            .client
            .get(url.as_str())
            .send()
            .await
            .map_err(|e| Error::Network(e.to_string()))?;

        if !response.status().is_success() {
            return Err(Error::Network(format!("HTTP {}", response.status())));
        }

        let total_size = response.content_length();

        let progress = if self.options.show_progress {
            let pb = ProgressBar::new(total_size.unwrap_or(0));
            pb.set_style(
                ProgressStyle::default_bar()
                    .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
                    .unwrap_or_else(|_| ProgressStyle::default_bar())
                    .progress_chars("#>-"),
            );
            Some(pb)
        } else {
            None
        };

        // Download to temp file first
        let temp_file = NamedTempFile::new().map_err(|e| Error::io(dest, e))?;
        let mut file =
            tokio::fs::File::from_std(temp_file.reopen().map_err(|e| Error::io(dest, e))?);
        let mut hasher = libretto_core::ContentHasher::new();
        let mut downloaded: u64 = 0;

        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| Error::Network(e.to_string()))?;
            file.write_all(&chunk)
                .await
                .map_err(|e| Error::io(dest, e))?;
            hasher.update(&chunk);
            downloaded += chunk.len() as u64;

            if let Some(ref pb) = progress {
                pb.set_position(downloaded);
            }
        }

        file.flush().await.map_err(|e| Error::io(dest, e))?;
        drop(file);

        if let Some(pb) = progress {
            pb.finish_and_clear();
        }

        // Move temp file to destination
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }

        temp_file
            .persist(dest)
            .map_err(|e| Error::io(dest, e.error))?;

        let hash = hasher.finalize();
        info!(url = %url, size = downloaded, hash = %hash.short(), "download complete");

        Ok(DownloadResult {
            path: dest.to_path_buf(),
            hash,
            size: downloaded,
        })
    }

    /// Verify downloaded file checksum.
    ///
    /// # Errors
    /// Returns error if checksums don't match.
    pub fn verify(&self, path: &Path, expected: &str, name: &str) -> Result<()> {
        let actual = ContentHash::from_file(path).map_err(|e| Error::io(path, e))?;

        if let Some(expected_hash) = ContentHash::from_hex(expected) {
            if actual != expected_hash {
                return Err(Error::ChecksumMismatch {
                    name: name.to_string(),
                    expected: expected.to_string(),
                    actual: actual.to_hex(),
                });
            }
        }

        Ok(())
    }
}

/// Parallel downloader for multiple files.
#[derive(Debug)]
pub struct ParallelDownloader {
    downloader: Downloader,
    concurrency: usize,
}

impl ParallelDownloader {
    /// Create parallel downloader.
    ///
    /// # Errors
    /// Returns error if downloader cannot be created.
    pub fn new(concurrency: usize) -> Result<Self> {
        Ok(Self {
            downloader: Downloader::with_defaults()?,
            concurrency,
        })
    }

    /// Download multiple URLs.
    ///
    /// # Errors
    /// Returns first error encountered.
    pub async fn download_all(
        &self,
        downloads: Vec<(Url, PathBuf)>,
    ) -> Result<Vec<DownloadResult>> {
        let results = futures::stream::iter(downloads)
            .map(|(url, dest)| {
                let downloader = &self.downloader;
                async move { downloader.download(&url, &dest).await }
            })
            .buffer_unordered(self.concurrency)
            .collect::<Vec<_>>()
            .await;

        results.into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_options_default() {
        let opts = DownloadOptions::default();
        assert_eq!(opts.retries, 3);
        assert!(opts.show_progress);
    }

    #[tokio::test]
    async fn downloader_creation() {
        let downloader = Downloader::with_defaults();
        assert!(downloader.is_ok());
    }
}
