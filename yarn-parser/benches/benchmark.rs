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
    let mut group = c.benchmark_group("simple_threaded_benchmark");
    group.sample_size(10);
    group.bench_function("single fib 1x35", |b| {
        b.iter(|| {
            black_box(fibonacci(black_box(35)));
    })});
    group.bench_function("single fib 8x35", |b| {
        b.iter(|| {
            black_box(fibonacci(black_box(35)));
            black_box(fibonacci(black_box(35)));
            black_box(fibonacci(black_box(35)));
            black_box(fibonacci(black_box(35)));

            black_box(fibonacci(black_box(35)));
            black_box(fibonacci(black_box(35)));
            black_box(fibonacci(black_box(35)));
            black_box(fibonacci(black_box(35)));
    })});
    group.bench_function("finite threaded fib 8x35", |b| {
        b.iter(|| {
            pool::Finite::new()
                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .with_job(|| {black_box(fibonacci(black_box(35)));})

                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .with_job(|| {black_box(fibonacci(black_box(35)));})
                .execute();
        })
    });

    group.bench_function("finite threaded fib 8x5", |b| {
        b.iter(|| {
            pool::Finite::new()
                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .with_job(|| {black_box(fibonacci(black_box(5)));})

                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .with_job(|| {black_box(fibonacci(black_box(5)));})
                .execute();
        })
    });

    // criterion thrashes the threadpool with lots of parallel called iterations, which saturates
    // the jobs queue. 
    group.bench_function("pool threaded fib 8x5", |b| {
        pool::scope(|scope| {
            b.iter(|| {
                    pool::spawn(scope, || {black_box(fibonacci(black_box(20)));});
                    pool::spawn(scope, || {black_box(fibonacci(black_box(20)));});
                    pool::spawn(scope, || {black_box(fibonacci(black_box(20)));});
                    pool::spawn(scope, || {black_box(fibonacci(black_box(20)));});

                    pool::spawn(scope,|| {black_box(fibonacci(black_box(20)));});
                    pool::spawn(scope,|| {black_box(fibonacci(black_box(20)));});
                    pool::spawn(scope,|| {black_box(fibonacci(black_box(20)));});
                    pool::spawn(scope,|| {black_box(fibonacci(black_box(20)));});
            });
        }).unwrap();
    });
}

criterion_group!(benches, simple_bench);
criterion_main!(benches);
