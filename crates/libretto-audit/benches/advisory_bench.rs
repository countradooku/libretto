//! Benchmarks for advisory fetching and vulnerability matching.

use criterion::{Criterion, black_box, criterion_group, criterion_main};
use libretto_audit::AdvisoryDatabase;
use libretto_core::{PackageId, Version};

fn bench_advisory_cache(c: &mut Criterion) {
    let _runtime = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("advisory_database_creation", |b| {
        b.iter(|| black_box(AdvisoryDatabase::new().unwrap()));
    });
}

fn bench_vulnerability_checking(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let db = AdvisoryDatabase::new().unwrap();

    let packages = vec![
        (
            PackageId::parse("symfony/symfony").unwrap(),
            Version::parse("2.0.0").unwrap(),
        ),
        (
            PackageId::parse("laravel/framework").unwrap(),
            Version::parse("5.0.0").unwrap(),
        ),
        (
            PackageId::parse("doctrine/orm").unwrap(),
            Version::parse("2.5.0").unwrap(),
        ),
    ];

    c.bench_function("check_single_package", |b| {
        let package = PackageId::parse("symfony/symfony").unwrap();
        let version = Version::parse("2.0.0").unwrap();

        b.to_async(&runtime)
            .iter(|| async { black_box(db.check_version(&package, &version).await.unwrap()) });
    });

    c.bench_function("check_multiple_packages", |b| {
        b.to_async(&runtime)
            .iter(|| async { black_box(db.check_packages(&packages).await.unwrap()) });
    });
}

criterion_group!(benches, bench_advisory_cache, bench_vulnerability_checking);
criterion_main!(benches);
