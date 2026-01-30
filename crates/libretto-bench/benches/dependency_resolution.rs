//! Dependency resolution benchmarks.
//!
//! Benchmarks for resolver operations and version parsing.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use libretto_bench::fixtures::DependencyGraph;
use std::time::Duration;

/// Benchmark version string parsing.
fn bench_version_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolution/version");

    let versions = [
        "1.0.0",
        "2.3.4",
        "10.20.30",
        "1.0.0-alpha.1",
        "2.0.0-beta.1+build.123",
    ];

    for version in versions {
        group.bench_with_input(BenchmarkId::new("parse", version), &version, |b, v| {
            b.iter(|| black_box(semver::Version::parse(v)));
        });
    }

    group.finish();
}

/// Benchmark constraint parsing (using semver crate).
fn bench_constraint_parsing(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolution/constraint");

    let constraints = ["^1.0", "~2.3.4", ">=1.0.0", "=1.2.3"];

    for constraint in constraints {
        group.bench_with_input(
            BenchmarkId::new("parse", constraint),
            &constraint,
            |b, c| {
                b.iter(|| black_box(semver::VersionReq::parse(c)));
            },
        );
    }

    group.finish();
}

/// Benchmark dependency graph creation.
fn bench_graph_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolution/graph");
    group.measurement_time(Duration::from_secs(10));

    for size in [10, 50, 100, 500] {
        group.bench_with_input(BenchmarkId::new("linear", size), &size, |b, &n| {
            b.iter(|| black_box(DependencyGraph::linear(n)));
        });

        group.bench_with_input(BenchmarkId::new("complex", size), &size, |b, &n| {
            b.iter(|| black_box(DependencyGraph::complex(n, 3)));
        });
    }

    group.finish();
}

/// Benchmark version matching operations.
fn bench_version_matching(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolution/matching");

    let req = semver::VersionReq::parse("^1.0").unwrap();
    let versions: Vec<semver::Version> = (0..100).map(|i| semver::Version::new(1, i, 0)).collect();

    group.bench_function("match_100_versions", |b| {
        b.iter(|| {
            let matches: Vec<_> = versions.iter().filter(|v| req.matches(v)).collect();
            black_box(matches)
        });
    });

    group.finish();
}

/// Benchmark sorting versions (used in resolution).
fn bench_version_sorting(c: &mut Criterion) {
    let mut group = c.benchmark_group("resolution/sorting");

    for count in [100, 500, 1000] {
        let versions: Vec<semver::Version> = (0..count)
            .map(|i| {
                semver::Version::new((i / 100) as u64, ((i / 10) % 10) as u64, (i % 10) as u64)
            })
            .collect();

        group.bench_with_input(BenchmarkId::new("versions", count), &versions, |b, v| {
            b.iter_with_setup(
                || v.clone(),
                |mut versions| {
                    versions.sort();
                    black_box(versions)
                },
            );
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_version_parsing,
    bench_constraint_parsing,
    bench_graph_creation,
    bench_version_matching,
    bench_version_sorting,
);

criterion_main!(benches);
