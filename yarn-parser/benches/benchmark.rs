use criterion::{black_box, criterion_group, criterion_main, Criterion};
use yarn_parser::pool;

fn fibonacci(n: u64) -> u64 {
    match n {
        0 => 1,
        1 => 1,
        n => fibonacci(n - 1) + fibonacci(n - 2),
    }
}

fn simple_bench(c: &mut Criterion) {
    const THREADS: usize = 32;
    let mut group = c.benchmark_group("simple_threaded_benchmark");
    group.sampling_mode(criterion::SamplingMode::Flat);
    group.bench_function("single fib 1x35", |b| {
        b.iter(|| {
            black_box(fibonacci(black_box(35)));
        })
    });
    group.bench_function("single fib Nx35", |b| {
        b.iter(|| {
            for _ in 0..THREADS {
                black_box(fibonacci(black_box(35)));
            }
        })
    });
    group.bench_function("threaded fib Nx35", |b| {
        let job0 = pool::job(|| {
            black_box(fibonacci(black_box(35)));
        });
        b.iter(|| {
            pool::scope(|mut scope: pool::Scope<THREADS>| {
                for _ in 0..THREADS {
                    scope.spawn(&job0);
                }
                scope
            });
        });
    });
}

criterion_group!(benches, simple_bench);
criterion_main!(benches);
