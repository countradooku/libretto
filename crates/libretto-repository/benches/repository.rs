//! Repository benchmarks.
//!
//! These benchmarks test the performance of repository operations.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use libretto_repository::cache::RepositoryCache;
use std::time::Duration;

/// Benchmark cache operations.
fn bench_cache_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache");

    // Cache put
    group.bench_function("put_metadata", |b| {
        let cache = RepositoryCache::new();
        let data = vec![0u8; 1024]; // 1KB
        let mut counter = 0u64;

        b.iter(|| {
            counter = counter.wrapping_add(1);
            let key = format!("key_{counter}");
            cache
                .put_metadata(&key, black_box(&data), Duration::from_secs(3600), None)
                .unwrap();
        });
    });

    // Cache get (hit)
    group.bench_function("get_metadata_hit", |b| {
        let cache = RepositoryCache::new();
        let data = vec![0u8; 1024];
        cache
            .put_metadata("test-key", &data, Duration::from_secs(3600), None)
            .unwrap();

        b.iter(|| {
            let _ = black_box(cache.get_metadata("test-key"));
        });
    });

    // Cache get (miss)
    group.bench_function("get_metadata_miss", |b| {
        let cache = RepositoryCache::new();
        let mut counter = 0u64;

        b.iter(|| {
            counter = counter.wrapping_add(1);
            let key = format!("missing_{counter}");
            let _ = black_box(cache.get_metadata(&key));
        });
    });

    group.finish();
}

/// Benchmark JSON parsing with sonic-rs.
fn bench_json_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("json_parsing");

    use libretto_repository::packagist::PackageMetadataResponse;

    // Package metadata
    let metadata_json = r#"{
        "packages": {
            "symfony/console": [
                {
                    "name": "symfony/console",
                    "version": "v6.4.0",
                    "description": "Eases the creation of beautiful and testable command line interfaces",
                    "type": "library",
                    "license": ["MIT"],
                    "require": {
                        "php": ">=8.1",
                        "symfony/polyfill-mbstring": "~1.0",
                        "symfony/deprecation-contracts": "^2.5|^3",
                        "symfony/service-contracts": "^2.5|^3",
                        "symfony/string": "^5.4|^6.0|^7.0"
                    },
                    "dist": {
                        "type": "zip",
                        "url": "https://api.github.com/repos/symfony/console/zipball/abc123",
                        "shasum": "abc123"
                    }
                }
            ]
        }
    }"#;

    group.bench_function("parse_package_metadata", |b| {
        b.iter(|| {
            let _: PackageMetadataResponse = black_box(sonic_rs::from_str(metadata_json).unwrap());
        });
    });

    group.finish();
}

/// Benchmark cache statistics.
fn bench_cache_stats(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_stats");

    use libretto_repository::cache::RepositoryCacheStats;
    use std::sync::atomic::Ordering;

    group.bench_function("hit_rate_calculation", |b| {
        let stats = RepositoryCacheStats::new();
        stats.hits.store(7500, Ordering::Relaxed);
        stats.misses.store(2500, Ordering::Relaxed);

        b.iter(|| {
            let _ = black_box(stats.hit_rate());
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cache_operations,
    bench_json_parsing,
    bench_cache_stats,
);
criterion_main!(benches);
