//! Benchmarks for hashing performance
//!
//! These benchmarks measure the throughput of content hashing operations
//! across different file sizes to characterize performance.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use janice::hash::{hash_bytes, Hasher};
use std::io::Write;
use tempfile::NamedTempFile;

/// Benchmark hashing of in-memory data of various sizes
fn bench_hash_bytes(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_bytes");

    let sizes = vec![
        ("4KB", 4 * 1024),
        ("64KB", 64 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    for (name, size) in sizes {
        let data = vec![0x42u8; size];
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &data, |b, data| {
            b.iter(|| {
                let hash = hash_bytes(black_box(data));
                black_box(hash);
            });
        });
    }

    group.finish();
}

/// Benchmark streaming file hashing
fn bench_hash_file(c: &mut Criterion) {
    let mut group = c.benchmark_group("hash_file");

    let sizes = vec![
        ("4KB", 4 * 1024),
        ("64KB", 64 * 1024),
        ("1MB", 1024 * 1024),
        ("10MB", 10 * 1024 * 1024),
    ];

    for (name, size) in sizes {
        // Create temporary file
        let mut temp_file = NamedTempFile::new().unwrap();
        let data = vec![0x42u8; size];
        temp_file.write_all(&data).unwrap();
        temp_file.flush().unwrap();

        let path = temp_file.path().to_path_buf();
        group.throughput(Throughput::Bytes(size as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &path, |b, path| {
            b.iter(|| {
                let mut hasher = Hasher::new();
                hasher.hash_file(black_box(path)).unwrap();
                let hash = hasher.finalize();
                black_box(hash);
            });
        });
    }

    group.finish();
}

/// Benchmark incremental hashing
fn bench_incremental_hashing(c: &mut Criterion) {
    let mut group = c.benchmark_group("incremental_hashing");

    let total_size = 1024 * 1024; // 1MB
    let chunk_sizes =
        vec![("1KB chunks", 1024), ("4KB chunks", 4 * 1024), ("64KB chunks", 64 * 1024)];

    let data = vec![0x42u8; total_size];

    for (name, chunk_size) in chunk_sizes {
        group.throughput(Throughput::Bytes(total_size as u64));

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &(data.clone(), chunk_size),
            |b, (data, chunk_size)| {
                b.iter(|| {
                    let mut hasher = Hasher::new();
                    for chunk in data.chunks(*chunk_size) {
                        hasher.update(black_box(chunk));
                    }
                    let hash = hasher.finalize();
                    black_box(hash);
                });
            },
        );
    }

    group.finish();
}

/// Benchmark hashing many small files (simulating directory scan)
fn bench_many_small_files(c: &mut Criterion) {
    let mut group = c.benchmark_group("many_small_files");

    let file_size = 4 * 1024; // 4KB each
    let file_count = 100;

    // Create temporary files
    let temp_files: Vec<_> = (0..file_count)
        .map(|i| {
            let mut temp = NamedTempFile::new().unwrap();
            let data = vec![i as u8; file_size];
            temp.write_all(&data).unwrap();
            temp.flush().unwrap();
            temp
        })
        .collect();

    let paths: Vec<_> = temp_files.iter().map(|t| t.path().to_path_buf()).collect();
    let total_bytes = (file_size * file_count) as u64;

    group.throughput(Throughput::Bytes(total_bytes));

    group.bench_function("sequential", |b| {
        b.iter(|| {
            for path in &paths {
                let mut hasher = Hasher::new();
                hasher.hash_file(black_box(path)).unwrap();
                let hash = hasher.finalize();
                black_box(hash);
            }
        });
    });

    group.finish();
}

/// Benchmark hash computation for different data patterns
fn bench_data_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("data_patterns");

    let size = 1024 * 1024; // 1MB
    group.throughput(Throughput::Bytes(size as u64));

    // All zeros
    let zeros = vec![0u8; size];
    group.bench_function("zeros", |b| {
        b.iter(|| {
            let hash = hash_bytes(black_box(&zeros));
            black_box(hash);
        });
    });

    // All ones
    let ones = vec![0xFFu8; size];
    group.bench_function("ones", |b| {
        b.iter(|| {
            let hash = hash_bytes(black_box(&ones));
            black_box(hash);
        });
    });

    // Random pattern (pseudo-random but consistent)
    let random: Vec<u8> = (0..size).map(|i| (i * 31 + 17) as u8).collect();
    group.bench_function("pseudorandom", |b| {
        b.iter(|| {
            let hash = hash_bytes(black_box(&random));
            black_box(hash);
        });
    });

    // Repeating pattern
    let pattern = vec![0x42u8; size];
    group.bench_function("repeating", |b| {
        b.iter(|| {
            let hash = hash_bytes(black_box(&pattern));
            black_box(hash);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_hash_bytes,
    bench_hash_file,
    bench_incremental_hashing,
    bench_many_small_files,
    bench_data_patterns
);
criterion_main!(benches);
