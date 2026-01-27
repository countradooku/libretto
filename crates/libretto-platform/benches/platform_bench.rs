//! Benchmarks for platform compatibility layer.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use libretto_platform::simd::{SimdOps, SimdRuntime};

fn bench_simd_compare_32(c: &mut Criterion) {
    let runtime = SimdRuntime::new();
    let a = [42u8; 32];
    let b = [42u8; 32];

    let mut group = c.benchmark_group("simd_compare_32");
    group.throughput(Throughput::Bytes(32));

    group.bench_function("equal", |bencher| {
        bencher.iter(|| runtime.compare_bytes_32(black_box(&a), black_box(&b)))
    });

    let mut c_different = [42u8; 32];
    c_different[31] = 0;
    group.bench_function("different", |bencher| {
        bencher.iter(|| runtime.compare_bytes_32(black_box(&a), black_box(&c_different)))
    });

    group.finish();
}

fn bench_simd_compare_64(c: &mut Criterion) {
    let runtime = SimdRuntime::new();
    let a = [42u8; 64];
    let b = [42u8; 64];

    let mut group = c.benchmark_group("simd_compare_64");
    group.throughput(Throughput::Bytes(64));

    group.bench_function("equal", |bencher| {
        bencher.iter(|| runtime.compare_bytes_64(black_box(&a), black_box(&b)))
    });

    group.finish();
}

fn bench_simd_find_byte(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("simd_find_byte");

    // Small haystack
    let small = vec![0u8; 64];
    group.throughput(Throughput::Bytes(64));
    group.bench_function("small_64b_not_found", |bencher| {
        bencher.iter(|| runtime.find_byte(black_box(&small), black_box(0xFF)))
    });

    // Medium haystack
    let medium = vec![0u8; 1024];
    group.throughput(Throughput::Bytes(1024));
    group.bench_function("medium_1kb_not_found", |bencher| {
        bencher.iter(|| runtime.find_byte(black_box(&medium), black_box(0xFF)))
    });

    // Large haystack
    let large = vec![0u8; 64 * 1024];
    group.throughput(Throughput::Bytes(64 * 1024));
    group.bench_function("large_64kb_not_found", |bencher| {
        bencher.iter(|| runtime.find_byte(black_box(&large), black_box(0xFF)))
    });

    // Find at beginning
    let mut early_find = vec![0u8; 1024];
    early_find[10] = 0xFF;
    group.bench_function("medium_1kb_early_find", |bencher| {
        bencher.iter(|| runtime.find_byte(black_box(&early_find), black_box(0xFF)))
    });

    // Find at end
    let mut late_find = vec![0u8; 1024];
    late_find[1020] = 0xFF;
    group.bench_function("medium_1kb_late_find", |bencher| {
        bencher.iter(|| runtime.find_byte(black_box(&late_find), black_box(0xFF)))
    });

    group.finish();
}

fn bench_simd_starts_with(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("simd_starts_with");

    // Short prefix (uses scalar)
    let haystack = vec![42u8; 1024];
    let short_prefix = vec![42u8; 16];
    group.bench_function("short_prefix_16b", |bencher| {
        bencher.iter(|| runtime.starts_with(black_box(&haystack), black_box(&short_prefix)))
    });

    // Long prefix (uses SIMD)
    let long_prefix = vec![42u8; 256];
    group.throughput(Throughput::Bytes(256));
    group.bench_function("long_prefix_256b", |bencher| {
        bencher.iter(|| runtime.starts_with(black_box(&haystack), black_box(&long_prefix)))
    });

    // Mismatch early
    let mut mismatch_early = vec![42u8; 256];
    mismatch_early[10] = 0;
    group.bench_function("mismatch_early", |bencher| {
        bencher.iter(|| runtime.starts_with(black_box(&haystack), black_box(&mismatch_early)))
    });

    group.finish();
}

fn bench_simd_xor(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("simd_xor");

    // 256 bytes
    let src_256 = vec![0xAAu8; 256];
    let mut dst_256 = vec![0x55u8; 256];
    group.throughput(Throughput::Bytes(256));
    group.bench_function("256b", |bencher| {
        bencher.iter(|| {
            dst_256.fill(0x55);
            runtime.xor_slices(black_box(&mut dst_256), black_box(&src_256))
        })
    });

    // 4KB
    let src_4k = vec![0xAAu8; 4096];
    let mut dst_4k = vec![0x55u8; 4096];
    group.throughput(Throughput::Bytes(4096));
    group.bench_function("4kb", |bencher| {
        bencher.iter(|| {
            dst_4k.fill(0x55);
            runtime.xor_slices(black_box(&mut dst_4k), black_box(&src_4k))
        })
    });

    // 64KB
    let src_64k = vec![0xAAu8; 65536];
    let mut dst_64k = vec![0x55u8; 65536];
    group.throughput(Throughput::Bytes(65536));
    group.bench_function("64kb", |bencher| {
        bencher.iter(|| {
            dst_64k.fill(0x55);
            runtime.xor_slices(black_box(&mut dst_64k), black_box(&src_64k))
        })
    });

    group.finish();
}

fn bench_simd_or(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("simd_or");

    // 4KB
    let src = vec![0xF0u8; 4096];
    let mut dst = vec![0x0Fu8; 4096];
    group.throughput(Throughput::Bytes(4096));
    group.bench_function("4kb", |bencher| {
        bencher.iter(|| {
            dst.fill(0x0F);
            runtime.or_slices(black_box(&mut dst), black_box(&src))
        })
    });

    group.finish();
}

fn bench_simd_popcount(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("simd_popcount");

    // Single u64
    group.bench_function("single_u64", |bencher| {
        bencher.iter(|| runtime.popcount_u64(black_box(0xAAAA_AAAA_AAAA_AAAAu64)))
    });

    // Slice of u64 (typical bloom filter size)
    let slice_128 = vec![0xAAAA_AAAA_AAAA_AAAAu64; 128];
    group.throughput(Throughput::Bytes(128 * 8));
    group.bench_function("slice_128_u64", |bencher| {
        bencher.iter(|| runtime.popcount_slice(black_box(&slice_128)))
    });

    // Larger slice
    let slice_1k = vec![0xAAAA_AAAA_AAAA_AAAAu64; 1024];
    group.throughput(Throughput::Bytes(1024 * 8));
    group.bench_function("slice_1k_u64", |bencher| {
        bencher.iter(|| runtime.popcount_slice(black_box(&slice_1k)))
    });

    group.finish();
}

fn bench_simd_sum_bytes(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("simd_sum_bytes");

    // 1KB
    let data_1k: Vec<u8> = (0..1024).map(|i| (i % 256) as u8).collect();
    group.throughput(Throughput::Bytes(1024));
    group.bench_function("1kb", |bencher| {
        bencher.iter(|| runtime.sum_bytes(black_box(&data_1k)))
    });

    // 64KB
    let data_64k: Vec<u8> = (0..65536).map(|i| (i % 256) as u8).collect();
    group.throughput(Throughput::Bytes(65536));
    group.bench_function("64kb", |bencher| {
        bencher.iter(|| runtime.sum_bytes(black_box(&data_64k)))
    });

    group.finish();
}

fn bench_hash_matching(c: &mut Criterion) {
    let runtime = SimdRuntime::new();

    let mut group = c.benchmark_group("hash_matching");

    // Create a set of hashes
    let mut hashes: Vec<[u8; 32]> = (0..1000)
        .map(|i| {
            let mut hash = [0u8; 32];
            hash[0] = (i % 256) as u8;
            hash[1] = ((i / 256) % 256) as u8;
            hash
        })
        .collect();

    // Add a few matching hashes
    let target = [42u8; 32];
    hashes[100] = target;
    hashes[500] = target;
    hashes[900] = target;

    group.bench_function("1000_hashes_3_matches", |bencher| {
        bencher.iter(|| runtime.find_matching_hashes(black_box(&hashes), black_box(&target)))
    });

    // No matches
    let no_match_target = [0xFFu8; 32];
    group.bench_function("1000_hashes_no_match", |bencher| {
        bencher
            .iter(|| runtime.find_matching_hashes(black_box(&hashes), black_box(&no_match_target)))
    });

    group.finish();
}

fn bench_cpu_detection(c: &mut Criterion) {
    use libretto_platform::cpu::CpuFeatures;

    let mut group = c.benchmark_group("cpu_detection");

    group.bench_function("detect_features", |bencher| {
        bencher.iter(|| CpuFeatures::detect())
    });

    group.bench_function("get_cached", |bencher| bencher.iter(|| CpuFeatures::get()));

    group.finish();
}

fn bench_platform_detection(c: &mut Criterion) {
    use libretto_platform::Platform;

    let mut group = c.benchmark_group("platform_detection");

    group.bench_function("detect", |bencher| bencher.iter(|| Platform::detect()));

    group.bench_function("current_cached", |bencher| {
        bencher.iter(|| Platform::current())
    });

    group.finish();
}

fn bench_cross_path(c: &mut Criterion) {
    use libretto_platform::fs::CrossPath;

    let mut group = c.benchmark_group("cross_path");

    group.bench_function("new_simple", |bencher| {
        bencher.iter(|| CrossPath::new(black_box("/home/user/project/file.txt")))
    });

    group.bench_function("from_string_mixed", |bencher| {
        bencher.iter(|| CrossPath::from_string(black_box("home\\user/project\\file.txt")))
    });

    let base = CrossPath::new("/home/user/project");
    group.bench_function("join", |bencher| {
        bencher.iter(|| base.join(black_box("subdir/file.txt")))
    });

    group.finish();
}

fn bench_shell_escape(c: &mut Criterion) {
    use libretto_platform::shell::{escape_shell_arg, ShellType};

    let mut group = c.benchmark_group("shell_escape");

    group.bench_function("simple_bash", |bencher| {
        bencher.iter(|| escape_shell_arg(black_box("simple"), ShellType::Bash))
    });

    group.bench_function("complex_bash", |bencher| {
        bencher.iter(|| escape_shell_arg(black_box("it's a \"test\" with $vars"), ShellType::Bash))
    });

    group.bench_function("simple_cmd", |bencher| {
        bencher.iter(|| escape_shell_arg(black_box("simple"), ShellType::Cmd))
    });

    group.bench_function("complex_cmd", |bencher| {
        bencher.iter(|| escape_shell_arg(black_box("test & more | pipes"), ShellType::Cmd))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_simd_compare_32,
    bench_simd_compare_64,
    bench_simd_find_byte,
    bench_simd_starts_with,
    bench_simd_xor,
    bench_simd_or,
    bench_simd_popcount,
    bench_simd_sum_bytes,
    bench_hash_matching,
    bench_cpu_detection,
    bench_platform_detection,
    bench_cross_path,
    bench_shell_escape,
);

criterion_main!(benches);
