//! A file sync tool that refuses to waste your time.

pub mod core;
pub mod hash;
pub mod io;

pub use core::{
    diff_scans, scan_directory, scan_directory_with_excludes, sync_changes, DiffResult, FileMeta,
    ScanResult, SyncOptions,
};
pub use hash::{hash_bytes, hash_file, ContentHash, Hasher};
pub use io::{
    atomic_copy_file_with_metadata, fsync_directory, generate_temp_path, AtomicWriter, SyncJournal,
    JAN_JOURNAL_FILE, JAN_TEMP_DIR,
};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
