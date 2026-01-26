//! JSON parsing benchmarks.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use libretto_core::{from_json, to_json};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ComposerJson {
    name: String,
    description: String,
    version: String,
    require: std::collections::HashMap<String, String>,
}

fn create_test_data() -> ComposerJson {
    let mut require = std::collections::HashMap::new();
    for i in 0..50 {
        require.insert(format!("vendor/package-{i}"), format!("^{i}.0"));
    }
    ComposerJson {
        name: "test/package".into(),
        description: "A test package for benchmarking".into(),
        version: "1.0.0".into(),
        require,
    }
}

fn bench_json(c: &mut Criterion) {
    let data = create_test_data();
    let json = to_json(&data).expect("serialize");

    c.bench_function("json_serialize", |b| {
        b.iter(|| to_json(black_box(&data)));
    });

    c.bench_function("json_deserialize", |b| {
        b.iter(|| from_json::<ComposerJson>(black_box(&json)));
    });
}

criterion_group!(benches, bench_json);
criterion_main!(benches);
