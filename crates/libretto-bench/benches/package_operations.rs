//! Package operations benchmarks.
//!
//! Benchmarks for downloads, archive extraction, and checksum verification.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use libretto_bench::generators::ArchiveGenerator;
use std::time::Duration;

/// Benchmark checksum computation with different hash algorithms.
fn bench_checksum_verification(c: &mut Criterion) {
    let mut group = c.benchmark_group("package/checksum");

    let sizes = [
        (1024, "1KB"),
        (64 * 1024, "64KB"),
        (1024 * 1024, "1MB"),
        (16 * 1024 * 1024, "16MB"),
    ];

    for (size, label) in sizes {
        let data = vec![0xABu8; size];

        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::new("blake3", label), &data, |b, d| {
            b.iter(|| {
                let hash = blake3::hash(black_box(d));
                black_box(hash)
            });
        });

        group.bench_with_input(BenchmarkId::new("sha256", label), &data, |b, d| {
            b.iter(|| {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(black_box(d));
                black_box(hasher.finalize())
            });
        });
    }

    group.finish();
}

/// Benchmark ZIP archive extraction.
fn bench_zip_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("package/extract/zip");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(15));

    let scenarios = [
        (10, 1024, "10_files_1KB"),
        (100, 1024, "100_files_1KB"),
        (10, 100 * 1024, "10_files_100KB"),
    ];

    for (num_files, file_size, label) in scenarios {
        let archive_gen = ArchiveGenerator::new().unwrap();
        let archive_path = archive_gen
            .generate_zip(label, num_files, file_size)
            .unwrap();

        group.throughput(Throughput::Bytes((num_files * file_size) as u64));
        group.bench_with_input(
            BenchmarkId::new("files", label),
            &archive_path,
            |b, path| {
                b.iter_with_setup(
                    || tempfile::tempdir().unwrap(),
                    |dest_dir| {
                        let file = std::fs::File::open(path).unwrap();
                        let mut archive = zip::ZipArchive::new(file).unwrap();
                        archive.extract(dest_dir.path()).unwrap();
                        black_box(dest_dir)
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark tar.gz archive extraction.
fn bench_tar_gz_extraction(c: &mut Criterion) {
    let mut group = c.benchmark_group("package/extract/targz");
    group.sample_size(20);
    group.measurement_time(Duration::from_secs(15));

    let scenarios = [(10, 1024, "10_files_1KB"), (100, 1024, "100_files_1KB")];

    for (num_files, file_size, label) in scenarios {
        let archive_gen = ArchiveGenerator::new().unwrap();
        let archive_path = archive_gen
            .generate_tar_gz(label, num_files, file_size)
            .unwrap();

        group.throughput(Throughput::Bytes((num_files * file_size) as u64));
        group.bench_with_input(
            BenchmarkId::new("files", label),
            &archive_path,
            |b, path| {
                b.iter_with_setup(
                    || tempfile::tempdir().unwrap(),
                    |dest_dir| {
                        use flate2::read::GzDecoder;

                        let file = std::fs::File::open(path).unwrap();
                        let decoder = GzDecoder::new(file);
                        let mut archive = tar::Archive::new(decoder);
                        archive.unpack(dest_dir.path()).unwrap();
                        black_box(dest_dir)
                    },
                );
            },
        );
    }

    group.finish();
}

/// Benchmark parallel task simulation.
fn bench_parallel_task_simulation(c: &mut Criterion) {
    let mut group = c.benchmark_group("package/parallel");
    group.sample_size(20);

    let runtime = libretto_bench::generators::create_runtime();

    for concurrency in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::new("concurrent", concurrency),
            &concurrency,
            |b, &n| {
                b.iter(|| {
                    runtime.block_on(async {
                        let handles: Vec<_> = (0..n)
                            .map(|i| {
                                tokio::spawn(async move {
                                    tokio::time::sleep(Duration::from_micros(100)).await;
                                    black_box(i)
                                })
                            })
                            .collect();

                        for handle in handles {
                            black_box(handle.await.unwrap());
                        }
                    });
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_checksum_verification,
    bench_zip_extraction,
    bench_tar_gz_extraction,
    bench_parallel_task_simulation,
);

criterion_main!(benches);
