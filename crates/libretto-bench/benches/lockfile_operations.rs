//! Lock file operations benchmarks.
//!
//! Benchmarks for parsing, generating, and diffing composer.lock files.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use libretto_bench::fixtures::{generate_composer_json, generate_composer_lock};

/// Benchmark composer.lock parsing with sonic-rs.
fn bench_lockfile_parse_sonic(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfile/parse/sonic_rs");

    for num_packages in [10, 100, 500, 1000] {
        let lock_content = generate_composer_lock(num_packages);
        let bytes = lock_content.as_bytes();

        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("packages", num_packages),
            &lock_content,
            |b, content| {
                b.iter(|| {
                    let parsed: serde_json::Value = sonic_rs::from_str(black_box(content)).unwrap();
                    black_box(parsed)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark composer.lock parsing with serde_json (for comparison).
fn bench_lockfile_parse_serde(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfile/parse/serde_json");

    for num_packages in [10, 100, 500, 1000] {
        let lock_content = generate_composer_lock(num_packages);
        let bytes = lock_content.as_bytes();

        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("packages", num_packages),
            &lock_content,
            |b, content| {
                b.iter(|| {
                    let parsed: serde_json::Value =
                        serde_json::from_str(black_box(content)).unwrap();
                    black_box(parsed)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark content-hash computation.
fn bench_content_hash(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfile/content_hash");

    for num_deps in [10, 50, 100, 500] {
        let json_content = generate_composer_json(num_deps);
        let bytes = json_content.as_bytes();

        group.throughput(Throughput::Bytes(bytes.len() as u64));
        group.bench_with_input(
            BenchmarkId::new("deps", num_deps),
            &json_content,
            |b, content| {
                b.iter(|| {
                    let hash = blake3::hash(black_box(content.as_bytes()));
                    black_box(hash)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark lock file serialization.
fn bench_lockfile_serialize(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfile/serialize");

    for num_packages in [10, 100, 500, 1000] {
        let lock_content = generate_composer_lock(num_packages);
        let parsed: serde_json::Value = serde_json::from_str(&lock_content).unwrap();

        group.throughput(Throughput::Elements(num_packages as u64));
        group.bench_with_input(
            BenchmarkId::new("packages", num_packages),
            &parsed,
            |b, value| {
                b.iter(|| {
                    let output = sonic_rs::to_string(black_box(value)).unwrap();
                    black_box(output)
                });
            },
        );
    }

    group.finish();
}

/// Benchmark lock file diff computation.
fn bench_lockfile_diff(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfile/diff");
    group.sample_size(50);

    for num_packages in [100, 500] {
        let lock1 = generate_composer_lock(num_packages);
        let lock2 = generate_composer_lock(num_packages + 10);

        let parsed1: serde_json::Value = serde_json::from_str(&lock1).unwrap();
        let parsed2: serde_json::Value = serde_json::from_str(&lock2).unwrap();

        group.bench_with_input(
            BenchmarkId::new("packages", num_packages),
            &(parsed1.clone(), parsed2.clone()),
            |b, (v1, v2)| {
                b.iter(|| {
                    let packages1 = v1["packages"].as_array().unwrap();
                    let packages2 = v2["packages"].as_array().unwrap();

                    let names1: std::collections::HashSet<String> = packages1
                        .iter()
                        .filter_map(|p| p["name"].as_str().map(String::from))
                        .collect();
                    let names2: std::collections::HashSet<String> = packages2
                        .iter()
                        .filter_map(|p| p["name"].as_str().map(String::from))
                        .collect();

                    let added: Vec<_> = names2.difference(&names1).cloned().collect();
                    let removed: Vec<_> = names1.difference(&names2).cloned().collect();

                    black_box((added, removed))
                });
            },
        );
    }

    group.finish();
}

/// Benchmark deterministic JSON output.
fn bench_deterministic_output(c: &mut Criterion) {
    let mut group = c.benchmark_group("lockfile/deterministic");

    for num_packages in [100, 500] {
        let lock_content = generate_composer_lock(num_packages);
        let parsed: serde_json::Value = serde_json::from_str(&lock_content).unwrap();

        group.bench_with_input(
            BenchmarkId::new("packages", num_packages),
            &parsed,
            |b, value| {
                b.iter(|| {
                    let output = serde_json::to_string_pretty(black_box(value)).unwrap();
                    black_box(output)
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_lockfile_parse_sonic,
    bench_lockfile_parse_serde,
    bench_content_hash,
    bench_lockfile_serialize,
    bench_lockfile_diff,
    bench_deterministic_output,
);

criterion_main!(benches);
