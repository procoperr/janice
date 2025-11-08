//! Benchmarks for directory walking performance
//!
//! These benchmarks measure the throughput of directory traversal operations
//! to characterize the performance of the ignore crate-based walker.

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use std::fs;
use std::hint::black_box;
use tempfile::TempDir;

/// Helper to create a directory structure with many files
fn create_flat_directory(file_count: usize) -> TempDir {
    let temp_dir = TempDir::new().unwrap();

    for i in 0..file_count {
        let path = temp_dir.path().join(format!("file_{:04}.txt", i));
        fs::write(&path, format!("Content {}", i).as_bytes()).unwrap();
    }

    temp_dir
}

/// Helper to create a nested directory structure
fn create_nested_directory(depth: usize, files_per_level: usize) -> TempDir {
    let temp_dir = TempDir::new().unwrap();

    fn create_level(base: &std::path::Path, current_depth: usize, max_depth: usize, files: usize) {
        if current_depth >= max_depth {
            return;
        }

        // Create files at this level
        for i in 0..files {
            let path = base.join(format!("file_{}.txt", i));
            fs::write(&path, format!("Content at depth {}", current_depth).as_bytes()).unwrap();
        }

        // Create subdirectories
        for i in 0..3 {
            let subdir = base.join(format!("subdir_{}", i));
            fs::create_dir_all(&subdir).unwrap();
            create_level(&subdir, current_depth + 1, max_depth, files);
        }
    }

    create_level(temp_dir.path(), 0, depth, files_per_level);
    temp_dir
}

/// Benchmark walking flat directories with many files
fn bench_flat_directory_walk(c: &mut Criterion) {
    let mut group = c.benchmark_group("flat_directory_walk");

    let file_counts = vec![("10_files", 10), ("100_files", 100), ("1000_files", 1000)];

    for (name, count) in file_counts {
        let temp_dir = create_flat_directory(count);
        group.throughput(Throughput::Elements(count as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &temp_dir, |b, dir| {
            b.iter(|| {
                let walker =
                    ignore::WalkBuilder::new(dir.path()).hidden(false).git_ignore(true).build();

                let mut file_count = 0;
                for entry in walker.flatten() {
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        file_count += 1;
                    }
                }
                black_box(file_count);
            });
        });
    }

    group.finish();
}

/// Benchmark walking nested directory structures
fn bench_nested_directory_walk(c: &mut Criterion) {
    let mut group = c.benchmark_group("nested_directory_walk");

    let configs = vec![("depth_3", 3, 5), ("depth_5", 5, 3)];

    for (name, depth, files_per_level) in configs {
        let temp_dir = create_nested_directory(depth, files_per_level);

        // Count total files for throughput
        let walker = ignore::WalkBuilder::new(temp_dir.path()).build();
        let total_files = walker
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|ft| ft.is_file()).unwrap_or(false))
            .count();

        group.throughput(Throughput::Elements(total_files as u64));

        group.bench_with_input(BenchmarkId::from_parameter(name), &temp_dir, |b, dir| {
            b.iter(|| {
                let walker =
                    ignore::WalkBuilder::new(dir.path()).hidden(false).git_ignore(true).build();

                let mut file_count = 0;
                for entry in walker.flatten() {
                    if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                        file_count += 1;
                    }
                }
                black_box(file_count);
            });
        });
    }

    group.finish();
}

/// Benchmark parallel directory walking
fn bench_parallel_directory_walk(c: &mut Criterion) {
    let mut group = c.benchmark_group("parallel_directory_walk");

    let file_count = 500;
    let temp_dir = create_flat_directory(file_count);

    group.throughput(Throughput::Elements(file_count as u64));

    group.bench_function("parallel", |b| {
        b.iter(|| {
            let walker = ignore::WalkBuilder::new(temp_dir.path())
                .hidden(false)
                .git_ignore(true)
                .threads(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1))
                .build_parallel();

            let file_count = std::sync::atomic::AtomicUsize::new(0);

            walker.run(|| {
                let file_count = &file_count;
                Box::new(move |entry| {
                    if let Ok(entry) = entry {
                        if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                            file_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    ignore::WalkState::Continue
                })
            });

            black_box(file_count.load(std::sync::atomic::Ordering::Relaxed));
        });
    });

    group.finish();
}

/// Benchmark directory walk with metadata collection
fn bench_walk_with_metadata(c: &mut Criterion) {
    let mut group = c.benchmark_group("walk_with_metadata");

    let file_count = 100;
    let temp_dir = create_flat_directory(file_count);

    group.throughput(Throughput::Elements(file_count as u64));

    group.bench_function("collect_metadata", |b| {
        b.iter(|| {
            let walker =
                ignore::WalkBuilder::new(temp_dir.path()).hidden(false).git_ignore(true).build();

            let mut total_size = 0u64;
            for entry in walker.flatten() {
                if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    if let Ok(metadata) = entry.metadata() {
                        total_size += metadata.len();
                    }
                }
            }
            black_box(total_size);
        });
    });

    group.finish();
}

/// Benchmark walking with .gitignore filtering
fn bench_walk_with_gitignore(c: &mut Criterion) {
    let mut group = c.benchmark_group("walk_with_gitignore");

    // Create directory with .gitignore
    let temp_dir = TempDir::new().unwrap();

    // Create files
    for i in 0..50 {
        fs::write(temp_dir.path().join(format!("file_{}.txt", i)), "content").unwrap();
    }

    // Create ignored files
    let ignored_dir = temp_dir.path().join("ignored");
    fs::create_dir_all(&ignored_dir).unwrap();
    for i in 0..50 {
        fs::write(ignored_dir.join(format!("ignored_{}.txt", i)), "content").unwrap();
    }

    // Create .gitignore
    fs::write(temp_dir.path().join(".gitignore"), "ignored/\n").unwrap();

    group.bench_function("with_gitignore", |b| {
        b.iter(|| {
            let walker =
                ignore::WalkBuilder::new(temp_dir.path()).hidden(false).git_ignore(true).build();

            let mut file_count = 0;
            for entry in walker.flatten() {
                if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    file_count += 1;
                }
            }
            black_box(file_count);
        });
    });

    group.bench_function("without_gitignore", |b| {
        b.iter(|| {
            let walker =
                ignore::WalkBuilder::new(temp_dir.path()).hidden(false).git_ignore(true).build();

            let mut file_count = 0;
            for entry in walker.flatten() {
                if entry.file_type().map(|ft| ft.is_file()).unwrap_or(false) {
                    file_count += 1;
                }
            }
            black_box(file_count);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_flat_directory_walk,
    bench_nested_directory_walk,
    bench_parallel_directory_walk,
    bench_walk_with_metadata,
    bench_walk_with_gitignore
);
criterion_main!(benches);
