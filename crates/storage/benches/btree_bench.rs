use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use rand::Rng;
use storage::BTree;

fn bench_set(c: &mut Criterion) {
    let mut group = c.benchmark_group("set");
    for size in [1_000usize, 10_000, 100_000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, &size| {
            let mut t = BTree::new();
            for i in 0..size {
                t.set(format!("key{i:08}").as_bytes(), b"warmup-value");
            }
            let mut i = size;
            b.iter(|| {
                t.set(
                    format!("key{i:08}").as_bytes(),
                    b"benchmark-value-0123456789",
                );
                i += 1;
            });
        });
    }
    group.finish();
}

fn bench_get(c: &mut Criterion) {
    let mut group = c.benchmark_group("get");
    for size in [1_000usize, 10_000, 100_000] {
        let mut t = BTree::new();
        for i in 0..size {
            t.set(
                format!("key{i:08}").as_bytes(),
                b"benchmark-value-0123456789",
            );
        }
        let mut rng = rand::thread_rng();
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(BenchmarkId::from_parameter(size), &size, |b, _| {
            b.iter(|| {
                let i = rng.gen_range(0..size);
                black_box(t.get(format!("key{i:08}").as_bytes()));
            });
        });
    }
    group.finish();
}

fn bench_mixed(c: &mut Criterion) {
    c.bench_function("mixed_80_get_20_set", |b| {
        let mut t = BTree::new();
        for i in 0..50_000 {
            t.set(format!("key{i:08}").as_bytes(), b"value");
        }
        let mut rng = rand::thread_rng();
        let mut counter = 50_000usize;
        b.iter(|| {
            if rng.gen_bool(0.8) {
                let i = rng.gen_range(0..50_000);
                black_box(t.get(format!("key{i:08}").as_bytes()));
            } else {
                t.set(format!("key{counter:08}").as_bytes(), b"value");
                counter += 1;
            }
        });
    });
}

criterion_group!(benches, bench_set, bench_get, bench_mixed);
criterion_main!(benches);
