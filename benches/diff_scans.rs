//! Benchmarks for diff_scans performance
//!
//! These benchmarks measure the performance of the diff algorithm with varying
//! file counts and different operation types (adds, modifies, renames, deletes).

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use janice::{diff_scans, FileMeta, ScanResult};
use std::hint::black_box;
use std::path::PathBuf;
use std::time::SystemTime;

/// Helper to create a mock hash
fn mock_hash(seed: u64) -> janice::ContentHash {
    let bytes = seed.to_le_bytes();
    let mut hash_bytes = [0u8; 32];
    for i in 0..4 {
        hash_bytes[i * 8..(i + 1) * 8].copy_from_slice(&bytes);
    }
    janice::hash::hash_bytes(&hash_bytes)
}

/// Create a mock ScanResult with specified number of files
fn create_scan_result(file_count: usize, base_path: &str) -> ScanResult {
    let files: Vec<FileMeta> = (0..file_count)
        .map(|i| FileMeta {
            path: PathBuf::from(format!("{}/file_{:05}.txt", base_path, i)),
            size: 1024 * (i as u64 + 1),
            mtime: SystemTime::now(),
            hash: mock_hash(i as u64),
            permissions: Some(0o644),
        })
        .collect();

    ScanResult {
        root: PathBuf::from(base_path),
        files,
        scan_time: SystemTime::now(),
    }
}

/// Create scan results with no changes (identical)
fn create_identical_scans(file_count: usize) -> (ScanResult, ScanResult) {
    let source = create_scan_result(file_count, "source");
    let dest = ScanResult {
        root: PathBuf::from("dest"),
        files: source.files.clone(),
        scan_time: SystemTime::now(),
    };
    (source, dest)
}

/// Create scan results with all new files (100% adds)
fn create_all_new_scans(file_count: usize) -> (ScanResult, ScanResult) {
    let source = create_scan_result(file_count, "source");
    let dest = ScanResult {
        root: PathBuf::from("dest"),
        files: vec![],
        scan_time: SystemTime::now(),
    };
    (source, dest)
}

/// Create scan results with modifications (same paths, different hashes)
fn create_modified_scans(file_count: usize, modify_percent: usize) -> (ScanResult, ScanResult) {
    let source = create_scan_result(file_count, "source");
    let modify_count = (file_count * modify_percent) / 100;

    let dest_files: Vec<FileMeta> = source
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if i < modify_count {
                // Modified: different hash
                FileMeta {
                    path: f.path.clone(),
                    size: f.size + 100,
                    mtime: f.mtime,
                    hash: mock_hash((i + 100000) as u64),
                    permissions: f.permissions,
                }
            } else {
                // Unchanged
                f.clone()
            }
        })
        .collect();

    let dest = ScanResult {
        root: PathBuf::from("dest"),
        files: dest_files,
        scan_time: SystemTime::now(),
    };
    (source, dest)
}

/// Create scan results with renames (same hashes, different paths)
fn create_renamed_scans(file_count: usize, rename_percent: usize) -> (ScanResult, ScanResult) {
    let source = create_scan_result(file_count, "source");
    let rename_count = (file_count * rename_percent) / 100;

    let dest_files: Vec<FileMeta> = source
        .files
        .iter()
        .enumerate()
        .map(|(i, f)| {
            if i < rename_count {
                // Renamed: same hash, different path
                FileMeta {
                    path: PathBuf::from(format!("dest/renamed_{:05}.txt", i)),
                    size: f.size,
                    mtime: f.mtime,
                    hash: f.hash.clone(),
                    permissions: f.permissions,
                }
            } else {
                // Unchanged
                f.clone()
            }
        })
        .collect();

    let dest = ScanResult {
        root: PathBuf::from("dest"),
        files: dest_files,
        scan_time: SystemTime::now(),
    };
    (source, dest)
}

/// Create scan results with mixed operations
fn create_mixed_scans(file_count: usize) -> (ScanResult, ScanResult) {
    let source = create_scan_result(file_count, "source");
    let quarter = file_count / 4;

    let mut dest_files = Vec::new();

    // First quarter: unchanged
    for i in 0..quarter {
        dest_files.push(source.files[i].clone());
    }

    // Second quarter: modified
    for i in quarter..quarter * 2 {
        dest_files.push(FileMeta {
            path: source.files[i].path.clone(),
            size: source.files[i].size + 100,
            mtime: source.files[i].mtime,
            hash: mock_hash((i + 100000) as u64),
            permissions: source.files[i].permissions,
        });
    }

    // Third quarter: renamed
    for i in quarter * 2..quarter * 3 {
        dest_files.push(FileMeta {
            path: PathBuf::from(format!("dest/renamed_{:05}.txt", i)),
            size: source.files[i].size,
            mtime: source.files[i].mtime,
            hash: source.files[i].hash.clone(),
            permissions: source.files[i].permissions,
        });
    }

    // Fourth quarter: deleted (not in dest)

    let dest = ScanResult {
        root: PathBuf::from("dest"),
        files: dest_files,
        scan_time: SystemTime::now(),
    };
    (source, dest)
}

/// Benchmark diff_scans with varying file counts
fn bench_diff_scans_scale(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_scans_scale");

    let file_counts = vec![("100_files", 100), ("1000_files", 1000), ("10000_files", 10000)];

    for (name, count) in file_counts {
        let (source, dest) = create_identical_scans(count);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &count, |b, _| {
            b.iter(|| {
                let result = diff_scans(black_box(&source), black_box(&dest)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark diff_scans with all new files
fn bench_diff_scans_all_new(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_scans_all_new");

    let file_counts = vec![("100_files", 100), ("1000_files", 1000)];

    for (name, count) in file_counts {
        let (source, dest) = create_all_new_scans(count);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &count, |b, _| {
            b.iter(|| {
                let result = diff_scans(black_box(&source), black_box(&dest)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark diff_scans with modifications
fn bench_diff_scans_modified(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_scans_modified");

    let configs = vec![
        ("1000_files_10pct", 1000, 10),
        ("1000_files_50pct", 1000, 50),
        ("1000_files_90pct", 1000, 90),
    ];

    for (name, count, percent) in configs {
        let (source, dest) = create_modified_scans(count, percent);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &count, |b, _| {
            b.iter(|| {
                let result = diff_scans(black_box(&source), black_box(&dest)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark diff_scans with renames (tests hash map performance)
fn bench_diff_scans_renames(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_scans_renames");

    let configs = vec![
        ("1000_files_10pct", 1000, 10),
        ("1000_files_50pct", 1000, 50),
        ("1000_files_90pct", 1000, 90),
    ];

    for (name, count, percent) in configs {
        let (source, dest) = create_renamed_scans(count, percent);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &count, |b, _| {
            b.iter(|| {
                let result = diff_scans(black_box(&source), black_box(&dest)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

/// Benchmark diff_scans with mixed operations
fn bench_diff_scans_mixed(c: &mut Criterion) {
    let mut group = c.benchmark_group("diff_scans_mixed");

    let file_counts = vec![("1000_files", 1000), ("10000_files", 10000)];

    for (name, count) in file_counts {
        let (source, dest) = create_mixed_scans(count);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &count, |b, _| {
            b.iter(|| {
                let result = diff_scans(black_box(&source), black_box(&dest)).unwrap();
                black_box(result);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_diff_scans_scale,
    bench_diff_scans_all_new,
    bench_diff_scans_modified,
    bench_diff_scans_renames,
    bench_diff_scans_mixed
);
criterion_main!(benches);
