//! Fast Packagist fetcher with metadata caching.
//!
//! Uses reqwest with HTTP/2, connection pooling, and aggressive timeouts.
//! Caches package metadata locally for fast resolution on subsequent runs.

use libretto_resolver::turbo::{FetchedPackage, FetchedVersion, TurboFetcher};
use reqwest::Client;
use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tracing::{debug, trace};

/// Cache TTL for package metadata (1 hour)
const METADATA_CACHE_TTL: Duration = Duration::from_secs(3600);

/// Fast Packagist fetcher with HTTP/2, connection pooling, and metadata caching.
pub struct Fetcher {
    client: Client,
    base_url: String,
    cache_dir: PathBuf,
    requests: AtomicU64,
    bytes: AtomicU64,
    cache_hits: AtomicU64,
}

impl Fetcher {
    pub fn new() -> Result<Self, reqwest::Error> {
        let client = Client::builder()
            // Connection pooling
            .pool_max_idle_per_host(100)
            .pool_idle_timeout(Duration::from_secs(90))
            // HTTP/2 settings
            .http2_adaptive_window(true)
            .http2_initial_stream_window_size(2 * 1024 * 1024)
            .http2_initial_connection_window_size(4 * 1024 * 1024)
            // Timeouts
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(10))
            // Compression
            .gzip(true)
            .brotli(true)
            .deflate(true)
            .zstd(true)
            // TCP optimizations
            .tcp_keepalive(Duration::from_secs(30))
            .tcp_nodelay(true)
            .user_agent(format!("Libretto/{}", env!("CARGO_PKG_VERSION")))
            .build()?;

        // Set up metadata cache directory
        let cache_dir = directories::BaseDirs::new()
            .map(|d| d.home_dir().join(".libretto").join("metadata"))
            .unwrap_or_else(|| PathBuf::from(".libretto/metadata"));
        let _ = std::fs::create_dir_all(&cache_dir);

        Ok(Self {
            client,
            base_url: "https://repo.packagist.org/p2".to_string(),
            cache_dir,
            requests: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
        })
    }

    pub fn request_count(&self) -> u64 {
        self.requests.load(Ordering::Relaxed)
    }

    pub fn bytes_downloaded(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    pub fn cache_hits(&self) -> u64 {
        self.cache_hits.load(Ordering::Relaxed)
    }

    /// Get cache file path for a package
    fn cache_path(&self, name: &str) -> PathBuf {
        // Replace / with ~ for filesystem safety
        let safe_name = name.replace('/', "~");
        self.cache_dir.join(format!("{}.json", safe_name))
    }

    /// Try to read from cache
    fn read_cache(&self, name: &str) -> Option<Vec<u8>> {
        let path = self.cache_path(name);

        // Check if file exists and is fresh
        let metadata = std::fs::metadata(&path).ok()?;
        let modified = metadata.modified().ok()?;
        let age = modified.elapsed().ok()?;

        if age > METADATA_CACHE_TTL {
            trace!(package = %name, "cache expired");
            return None;
        }

        std::fs::read(&path).ok()
    }

    /// Write to cache
    fn write_cache(&self, name: &str, data: &[u8]) {
        let path = self.cache_path(name);
        let _ = std::fs::write(&path, data);
    }

    async fn fetch_impl(&self, name: &str) -> Option<FetchedPackage> {
        // Try cache first
        let bytes = if let Some(cached) = self.read_cache(name) {
            self.cache_hits.fetch_add(1, Ordering::Relaxed);
            trace!(package = %name, "cache hit");
            cached
        } else {
            // Fetch from network
            let url = format!("{}/{}.json", self.base_url, name);
            self.requests.fetch_add(1, Ordering::Relaxed);

            trace!(package = %name, "fetching from network");

            let response = match self.client.get(&url).send().await {
                Ok(r) => r,
                Err(e) => {
                    debug!(package = %name, error = %e, "fetch failed");
                    return None;
                }
            };

            if !response.status().is_success() {
                debug!(package = %name, status = %response.status(), "non-success status");
                return None;
            }

            let bytes = match response.bytes().await {
                Ok(b) => b,
                Err(e) => {
                    debug!(package = %name, error = %e, "failed to read body");
                    return None;
                }
            };

            self.bytes.fetch_add(bytes.len() as u64, Ordering::Relaxed);

            // Cache the response
            self.write_cache(name, &bytes);

            bytes.to_vec()
        };

        let json: PackagistResponse = match sonic_rs::from_slice(&bytes) {
            Ok(j) => j,
            Err(e) => {
                debug!(package = %name, error = %e, "JSON parse failed");
                return None;
            }
        };

        let versions = json.packages.get(name)?;

        let fetched_versions: Vec<FetchedVersion> = versions
            .iter()
            .filter_map(|v| {
                Some(FetchedVersion {
                    version: v.version.clone(),
                    require: v
                        .require
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    require_dev: v
                        .require_dev
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    replace: v
                        .replace
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    provide: v
                        .provide
                        .iter()
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                    dist_url: v.dist.as_ref().map(|d| d.url.clone()),
                    dist_type: v.dist.as_ref().map(|d| d.dist_type.clone()),
                    dist_shasum: v.dist.as_ref().and_then(|d| d.shasum.clone()),
                    source_url: v.source.as_ref().map(|s| s.url.clone()),
                    source_type: v.source.as_ref().map(|s| s.source_type.clone()),
                    source_reference: v.source.as_ref().map(|s| s.reference.clone()),
                })
            })
            .collect();

        if fetched_versions.is_empty() {
            None
        } else {
            Some(FetchedPackage {
                name: name.to_string(),
                versions: fetched_versions,
            })
        }
    }
}

impl TurboFetcher for Fetcher {
    fn fetch(
        &self,
        name: String,
    ) -> Pin<Box<dyn std::future::Future<Output = Option<FetchedPackage>> + Send + '_>> {
        Box::pin(async move { self.fetch_impl(&name).await })
    }
}

// --- Packagist JSON types ---

#[derive(Debug, serde::Deserialize)]
struct PackagistResponse {
    packages: HashMap<String, Vec<PackagistVersion>>,
}

#[derive(Debug, serde::Deserialize)]
struct PackagistVersion {
    version: String,
    #[serde(default, deserialize_with = "deserialize_deps")]
    require: HashMap<String, String>,
    #[serde(default, rename = "require-dev", deserialize_with = "deserialize_deps")]
    require_dev: HashMap<String, String>,
    #[serde(default, deserialize_with = "deserialize_deps")]
    replace: HashMap<String, String>,
    #[serde(default, deserialize_with = "deserialize_deps")]
    provide: HashMap<String, String>,
    #[serde(default)]
    dist: Option<PackagistDist>,
    #[serde(default)]
    source: Option<PackagistSource>,
}

fn deserialize_deps<'de, D>(deserializer: D) -> Result<HashMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};

    struct DepsVisitor;

    impl<'de> Visitor<'de> for DepsVisitor {
        type Value = HashMap<String, String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a map or \"__unset\" string")
        }

        fn visit_str<E>(self, _v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(HashMap::new())
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(HashMap::new())
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(HashMap::new())
        }

        fn visit_map<M>(self, mut map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let mut deps = HashMap::new();
            while let Some((key, value)) = map.next_entry::<String, String>()? {
                deps.insert(key, value);
            }
            Ok(deps)
        }
    }

    deserializer.deserialize_any(DepsVisitor)
}

#[derive(Debug, serde::Deserialize)]
struct PackagistDist {
    #[serde(rename = "type")]
    dist_type: String,
    url: String,
    shasum: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct PackagistSource {
    #[serde(rename = "type")]
    source_type: String,
    url: String,
    reference: String,
}
