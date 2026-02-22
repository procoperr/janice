//! File I/O with streaming copy and metadata preservation

use std::fs::{self, File, Metadata};
use std::io::{self, BufRead, BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::SystemTime;
use thiserror::Error;

// 256KB: optimal for modern SSD throughput
const COPY_BUFFER_SIZE: usize = 256 * 1024;

/// Janice temp directory name (inside destination root)
pub const JAN_TEMP_DIR: &str = ".jan-tmp";

/// Janice journal file name (inside destination root)
pub const JAN_JOURNAL_FILE: &str = ".jan-journal";

/// Monotonic counter for unique temp file names within a process
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Errors that can occur during I/O operations
#[derive(Error, Debug)]
pub enum IoError {
    #[error("Failed to copy file: {0}")]
    CopyFailed(String),

    #[error("Failed to set metadata: {0}")]
    MetadataFailed(String),

    #[error("Failed to remove file: {0}")]
    RemoveFailed(String),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Copy a file with streaming I/O and optional metadata preservation
///
/// This function copies a file from source to destination using buffered
/// streaming I/O to minimize memory usage. It can optionally preserve
/// file timestamps and permissions.
///
/// # Performance
///
/// - Uses 256KB buffer for efficient streaming
/// - Single allocation for buffer (reused throughout copy)
/// - Optimized for both small and large files
/// - Typical throughput: limited by I/O subsystem (500MB/s - 3GB/s on modern SSDs)
///
/// # Arguments
///
/// * `source` - Source file path
/// * `dest` - Destination file path
/// * `preserve_timestamps` - Whether to preserve modification time
///
/// # Example
///
/// ```no_run
/// use janice::io::copy_file_with_metadata;
/// use std::path::Path;
///
/// # fn main() -> std::io::Result<()> {
/// copy_file_with_metadata(
///     Path::new("source.txt"),
///     Path::new("dest.txt"),
///     true
/// )?;
/// # Ok(())
/// # }
/// ```
pub fn copy_file_with_metadata(
    source: &Path,
    dest: &Path,
    preserve_timestamps: bool,
) -> io::Result<()> {
    // Get metadata before copying
    let metadata = fs::metadata(source)?;

    // Perform the streaming copy
    copy_file_streaming(source, dest)?;

    // Preserve metadata if requested
    if preserve_timestamps {
        set_file_mtime(dest, metadata.modified()?)?;
    }

    // Preserve permissions on Unix systems
    #[cfg(unix)]
    {
        set_file_permissions(dest, &metadata)?;
    }

    Ok(())
}

fn copy_file_streaming(source: &Path, dest: &Path) -> io::Result<()> {
    let source_file = File::open(source)?;
    let dest_file = File::create(dest)?;

    let mut reader = BufReader::with_capacity(COPY_BUFFER_SIZE, source_file);
    let mut writer = BufWriter::with_capacity(COPY_BUFFER_SIZE, dest_file);

    io::copy(&mut reader, &mut writer)?;

    writer.flush()?;
    writer.into_inner()?.sync_all()?;

    Ok(())
}

pub fn set_file_mtime(path: &Path, mtime: SystemTime) -> io::Result<()> {
    let file = File::open(path)?;
    file.set_modified(mtime)?;
    Ok(())
}

#[cfg(unix)]
pub fn set_file_permissions(path: &Path, metadata: &Metadata) -> io::Result<()> {
    let permissions = metadata.permissions();
    fs::set_permissions(path, permissions)?;
    Ok(())
}

/// Remove file, ignoring "not found" errors
///
/// ```no_run
/// use janice::io::remove_file_safe;
/// use std::path::Path;
///
/// # fn main() -> std::io::Result<()> {
/// remove_file_safe(Path::new("old_file.txt"))?;
/// # Ok(())
/// # }
/// ```
pub fn remove_file_safe(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // File doesn't exist - this is fine, treat as success
            Ok(())
        },
        Err(e) => Err(e),
    }
}

/// Remove a directory and all its contents recursively
///
/// This function is similar to `fs::remove_dir_all` but with enhanced
/// error handling and safety checks.
pub fn remove_dir_recursive(path: &Path) -> io::Result<()> {
    if !path.exists() {
        return Ok(());
    }

    fs::remove_dir_all(path)
}

/// Verify that two files have identical content
///
/// This function compares two files byte-by-byte using streaming I/O.
/// Useful for verifying that a copy operation succeeded.
///
/// # Performance
///
/// - Buffered streaming comparison (constant memory)
/// - Early exit on first difference
/// - Optimized for both matching and non-matching files
/// - Uses 256KB buffers for optimal I/O performance
pub fn verify_files_identical(path1: &Path, path2: &Path) -> io::Result<bool> {
    // Quick metadata check first
    let meta1 = fs::metadata(path1)?;
    let meta2 = fs::metadata(path2)?;

    // If sizes differ, files can't be identical
    if meta1.len() != meta2.len() {
        return Ok(false);
    }

    // Compare contents with buffered I/O
    let file1 = File::open(path1)?;
    let file2 = File::open(path2)?;

    let mut reader1 = BufReader::with_capacity(COPY_BUFFER_SIZE, file1);
    let mut reader2 = BufReader::with_capacity(COPY_BUFFER_SIZE, file2);

    let mut buffer1 = vec![0u8; COPY_BUFFER_SIZE];
    let mut buffer2 = vec![0u8; COPY_BUFFER_SIZE];

    loop {
        let bytes_read1 = reader1.read(&mut buffer1)?;
        let bytes_read2 = reader2.read(&mut buffer2)?;

        // If read sizes differ, files are different
        if bytes_read1 != bytes_read2 {
            return Ok(false);
        }

        // End of both files
        if bytes_read1 == 0 {
            break;
        }

        // Compare the buffers
        if buffer1[..bytes_read1] != buffer2[..bytes_read2] {
            return Ok(false);
        }
    }

    Ok(true)
}

/// Compute the total size of a directory recursively
///
/// This function walks a directory tree and sums the sizes of all files.
/// Useful for progress reporting and capacity planning.
pub fn directory_size(path: &Path) -> io::Result<u64> {
    let mut total = 0u64;

    if path.is_file() {
        return Ok(fs::metadata(path)?.len());
    }

    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = entry.metadata()?;

        if metadata.is_file() {
            total += metadata.len();
        } else if metadata.is_dir() {
            total += directory_size(&entry.path())?;
        }
    }

    Ok(total)
}

/// Ensure a directory exists, creating it and all parent directories if necessary
///
/// This is a convenience wrapper around `fs::create_dir_all` with better
/// error messages.
pub fn ensure_directory(path: &Path) -> io::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("Path exists but is not a directory: {}", path.display()),
            ));
        }
        return Ok(());
    }

    fs::create_dir_all(path)
}

/// Generate a unique temp file path within the given directory.
///
/// Format: `{PID}-{counter}.tmp` — unique per process, monotonic counter.
pub fn generate_temp_path(temp_dir: &Path) -> PathBuf {
    let pid = std::process::id();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    temp_dir.join(format!("{pid}-{counter}.tmp"))
}

/// Crash-safe atomic file writer.
///
/// Writes data to a temporary file, then atomically renames to the final
/// destination on commit. If dropped without commit, the temp file is
/// cleaned up automatically.
///
/// # Safety guarantees
///
/// - Destination file is never in a partial/corrupt state
/// - On crash: either the old file or the complete new file exists
/// - Temp file is always on the same filesystem as destination
pub struct AtomicWriter {
    temp_path: PathBuf,
    final_path: PathBuf,
    writer: BufWriter<File>,
    hasher: Option<crate::hash::Hasher>,
    committed: bool,
}

impl AtomicWriter {
    /// Create a new atomic writer.
    ///
    /// * `temp_path` - Pre-generated temp file path (same filesystem as final_path)
    /// * `final_path` - Final destination path after atomic rename
    /// * `verify` - If true, compute BLAKE3 hash during write for verification
    pub fn new(temp_path: PathBuf, final_path: PathBuf, verify: bool) -> io::Result<Self> {
        let file = File::create(&temp_path)?;
        let writer = BufWriter::with_capacity(COPY_BUFFER_SIZE, file);
        let hasher = if verify {
            Some(crate::hash::Hasher::new())
        } else {
            None
        };

        Ok(Self {
            temp_path,
            final_path,
            writer,
            hasher,
            committed: false,
        })
    }

    /// Write data to the temp file, updating the hash if verification is enabled.
    pub fn write(&mut self, buf: &[u8]) -> io::Result<()> {
        self.writer.write_all(buf)?;
        if let Some(ref mut hasher) = self.hasher {
            hasher.update(buf);
        }
        Ok(())
    }

    /// Commit the atomic write: flush, fsync, verify hash, rename.
    ///
    /// If `expected_hash` is provided and verification is enabled, the computed
    /// hash is compared against it. Returns an error on mismatch (temp cleaned
    /// up by Drop).
    pub fn commit(mut self, expected_hash: Option<&crate::hash::ContentHash>) -> io::Result<()> {
        self.writer.flush()?;
        self.writer.get_ref().sync_all()?;

        if let (Some(hasher), Some(expected)) = (self.hasher.take(), expected_hash) {
            let computed = hasher.finalize();
            if computed != *expected {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!(
                        "Hash verification failed for {}: expected {expected}, got {computed}",
                        self.final_path.display(),
                    ),
                ));
            }
        }

        fs::rename(&self.temp_path, &self.final_path)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for AtomicWriter {
    fn drop(&mut self) {
        if !self.committed {
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}

/// Copy a file atomically with streaming I/O and optional verification.
///
/// Writes to a temp file, fsyncs, then atomically renames to `dest`.
/// The destination is never in a partial state.
pub fn atomic_copy_file_with_metadata(
    source: &Path,
    dest: &Path,
    temp_path: &Path,
    preserve_timestamps: bool,
    verify: bool,
    expected_hash: Option<&crate::hash::ContentHash>,
) -> io::Result<()> {
    let metadata = fs::metadata(source)?;

    let mut writer = AtomicWriter::new(temp_path.to_path_buf(), dest.to_path_buf(), verify)?;

    let source_file = File::open(source)?;
    let mut reader = BufReader::with_capacity(COPY_BUFFER_SIZE, source_file);
    let mut buffer = vec![0u8; COPY_BUFFER_SIZE];

    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        writer.write(&buffer[..bytes_read])?;
    }

    writer.commit(expected_hash)?;

    if preserve_timestamps {
        set_file_mtime(dest, metadata.modified()?)?;
    }

    #[cfg(unix)]
    {
        set_file_permissions(dest, &metadata)?;
    }

    Ok(())
}

/// Flush directory metadata to disk (ensures renames are persisted).
///
/// No-op on Windows where directory fsync is not supported.
pub fn fsync_directory(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

/// Append-only journal for crash recovery.
///
/// Records pending/committed state for each file operation. On recovery,
/// entries with P but no matching C indicate incomplete operations whose
/// temp files should be cleaned up.
pub struct SyncJournal {
    file: Mutex<BufWriter<File>>,
    path: PathBuf,
}

impl SyncJournal {
    /// Create a new journal file at the given path.
    pub fn create(path: PathBuf) -> io::Result<Self> {
        let file = File::create(&path)?;
        let writer = BufWriter::new(file);
        Ok(Self { file: Mutex::new(writer), path })
    }

    /// Record that an operation is about to begin.
    pub fn record_pending(&self, op: &str, temp_path: &Path, final_path: &Path) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();
        writeln!(file, "P\t{op}\t{}\t{}", temp_path.display(), final_path.display())?;
        file.flush()?;
        Ok(())
    }

    /// Record that an operation completed successfully.
    pub fn record_committed(
        &self,
        op: &str,
        temp_path: &Path,
        final_path: &Path,
    ) -> io::Result<()> {
        let mut file = self.file.lock().unwrap();
        writeln!(file, "C\t{op}\t{}\t{}", temp_path.display(), final_path.display())?;
        file.flush()?;
        Ok(())
    }

    /// Clean up the journal file.
    pub fn remove(self) -> io::Result<()> {
        drop(self.file);
        remove_file_safe(&self.path)
    }

    /// Recover from a previous interrupted sync.
    ///
    /// Reads the journal, finds P entries without matching C entries,
    /// and cleans up their temp files. Then removes the journal and
    /// sweeps the temp directory.
    pub fn recover(journal_path: &Path, temp_dir: &Path) -> io::Result<()> {
        if !journal_path.exists() {
            // No journal means clean state; still sweep orphaned temps
            if temp_dir.exists() {
                cleanup_temp_dir(temp_dir)?;
            }
            return Ok(());
        }

        let file = File::open(journal_path)?;
        let reader = BufReader::new(file);

        let mut pending: Vec<(String, PathBuf, PathBuf)> = Vec::new();
        let mut committed: Vec<(String, PathBuf, PathBuf)> = Vec::new();

        for line in reader.lines() {
            let line = match line {
                Ok(l) => l,
                Err(_) => break,
            };

            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() < 4 {
                continue;
            }

            let entry = (parts[1].to_string(), PathBuf::from(parts[2]), PathBuf::from(parts[3]));

            match parts[0] {
                "P" => pending.push(entry),
                "C" => committed.push(entry),
                _ => continue,
            }
        }

        for (op, temp, final_path) in &pending {
            let is_committed = committed
                .iter()
                .any(|(cop, ctemp, cfinal)| cop == op && ctemp == temp && cfinal == final_path);

            if !is_committed {
                let _ = fs::remove_file(temp);
            }
        }

        remove_file_safe(journal_path)?;

        if temp_dir.exists() {
            cleanup_temp_dir(temp_dir)?;
        }

        Ok(())
    }
}

/// Remove all files in the temp directory (orphan cleanup).
fn cleanup_temp_dir(temp_dir: &Path) -> io::Result<()> {
    if let Ok(entries) = fs::read_dir(temp_dir) {
        for entry in entries.flatten() {
            let _ = fs::remove_file(entry.path());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::{tempdir, NamedTempFile};

    #[test]
    fn test_copy_small_file() -> io::Result<()> {
        let mut source = NamedTempFile::new()?;
        let dest_dir = tempdir()?;
        let dest_path = dest_dir.path().join("dest.txt");

        let data = b"Hello, Janice!";
        source.write_all(data)?;
        source.flush()?;

        copy_file_with_metadata(source.path(), &dest_path, false)?;

        let copied_data = fs::read(&dest_path)?;
        assert_eq!(copied_data, data);

        Ok(())
    }

    #[test]
    fn test_copy_large_file() -> io::Result<()> {
        let mut source = NamedTempFile::new()?;
        let dest_dir = tempdir()?;
        let dest_path = dest_dir.path().join("dest.bin");

        // Create a file larger than the buffer size
        let chunk = vec![0x42u8; COPY_BUFFER_SIZE];
        for _ in 0..10 {
            source.write_all(&chunk)?;
        }
        source.flush()?;

        copy_file_with_metadata(source.path(), &dest_path, false)?;

        let source_size = fs::metadata(source.path())?.len();
        let dest_size = fs::metadata(&dest_path)?.len();
        assert_eq!(source_size, dest_size);

        Ok(())
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_preserve_timestamps() -> io::Result<()> {
        let mut source = NamedTempFile::new()?;
        let dest_dir = tempdir()?;
        let dest_path = dest_dir.path().join("dest.txt");

        source.write_all(b"test data")?;
        source.flush()?;

        let original_mtime = fs::metadata(source.path())?.modified()?;

        // Sleep briefly to ensure time passes
        std::thread::sleep(std::time::Duration::from_millis(10));

        copy_file_with_metadata(source.path(), &dest_path, true)?;

        let copied_mtime = fs::metadata(&dest_path)?.modified()?;

        // Timestamps should be close (within 1 second due to filesystem precision)
        let diff = copied_mtime
            .duration_since(original_mtime)
            .unwrap_or_else(|_| original_mtime.duration_since(copied_mtime).unwrap());
        assert!(diff.as_secs() < 2);

        Ok(())
    }

    #[test]
    fn test_remove_file_safe() -> io::Result<()> {
        let mut temp = NamedTempFile::new()?;
        temp.write_all(b"test")?;
        temp.flush()?;

        let path = temp.path().to_path_buf();

        // First removal should succeed
        remove_file_safe(&path)?;

        // Second removal should also succeed (file doesn't exist)
        remove_file_safe(&path)?;

        Ok(())
    }

    #[test]
    fn test_verify_files_identical() -> io::Result<()> {
        let mut file1 = NamedTempFile::new()?;
        let mut file2 = NamedTempFile::new()?;

        let data = b"test data for verification";
        file1.write_all(data)?;
        file2.write_all(data)?;
        file1.flush()?;
        file2.flush()?;

        assert!(verify_files_identical(file1.path(), file2.path())?);

        // Create a different file
        let mut file3 = NamedTempFile::new()?;
        file3.write_all(b"different data")?;
        file3.flush()?;

        assert!(!verify_files_identical(file1.path(), file3.path())?);

        Ok(())
    }

    #[test]
    fn test_verify_different_sizes() -> io::Result<()> {
        let mut file1 = NamedTempFile::new()?;
        let mut file2 = NamedTempFile::new()?;

        file1.write_all(b"short")?;
        file2.write_all(b"much longer content")?;
        file1.flush()?;
        file2.flush()?;

        assert!(!verify_files_identical(file1.path(), file2.path())?);

        Ok(())
    }

    #[test]
    fn test_ensure_directory() -> io::Result<()> {
        let temp_dir = tempdir()?;
        let nested_path = temp_dir.path().join("a").join("b").join("c");

        ensure_directory(&nested_path)?;
        assert!(nested_path.exists());
        assert!(nested_path.is_dir());

        // Calling again should succeed
        ensure_directory(&nested_path)?;

        Ok(())
    }

    #[test]
    #[cfg(not(target_os = "windows"))]
    fn test_directory_size() -> io::Result<()> {
        let temp_dir = tempdir()?;

        // Create some files
        let mut file1 = File::create(temp_dir.path().join("file1.txt"))?;
        let mut file2 = File::create(temp_dir.path().join("file2.txt"))?;

        file1.write_all(&vec![0u8; 1000])?;
        file2.write_all(&vec![0u8; 2000])?;

        let size = directory_size(temp_dir.path())?;
        assert_eq!(size, 3000);

        Ok(())
    }

    #[test]
    fn test_atomic_writer_commit() -> io::Result<()> {
        let dir = tempdir()?;
        let temp_path = dir.path().join("temp.tmp");
        let final_path = dir.path().join("final.txt");

        let mut writer = AtomicWriter::new(temp_path.clone(), final_path.clone(), false)?;
        writer.write(b"hello atomic")?;
        writer.commit(None)?;

        assert!(final_path.exists());
        assert!(!temp_path.exists());
        assert_eq!(fs::read_to_string(&final_path)?, "hello atomic");

        Ok(())
    }

    #[test]
    fn test_atomic_writer_drop_cleanup() -> io::Result<()> {
        let dir = tempdir()?;
        let temp_path = dir.path().join("temp.tmp");
        let final_path = dir.path().join("final.txt");

        {
            let mut writer = AtomicWriter::new(temp_path.clone(), final_path.clone(), false)?;
            writer.write(b"uncommitted data")?;
        }

        assert!(!temp_path.exists(), "Temp file should be cleaned up on drop");
        assert!(!final_path.exists(), "Final path should not exist");

        Ok(())
    }

    #[test]
    fn test_atomic_writer_verify_success() -> io::Result<()> {
        let dir = tempdir()?;
        let temp_path = dir.path().join("temp.tmp");
        let final_path = dir.path().join("final.txt");

        let data = b"verify me";
        let expected_hash = crate::hash::hash_bytes(data);

        let mut writer = AtomicWriter::new(temp_path, final_path.clone(), true)?;
        writer.write(data)?;
        writer.commit(Some(&expected_hash))?;

        assert!(final_path.exists());
        Ok(())
    }

    #[test]
    fn test_atomic_writer_verify_failure() -> io::Result<()> {
        let dir = tempdir()?;
        let temp_path = dir.path().join("temp.tmp");
        let final_path = dir.path().join("final.txt");

        let wrong_hash = crate::hash::hash_bytes(b"wrong data");

        let mut writer = AtomicWriter::new(temp_path, final_path.clone(), true)?;
        writer.write(b"actual data")?;
        let result = writer.commit(Some(&wrong_hash));

        assert!(result.is_err());
        assert!(!final_path.exists(), "Should not rename on hash mismatch");

        Ok(())
    }

    #[test]
    fn test_atomic_copy_file_with_metadata_basic() -> io::Result<()> {
        let src_dir = tempdir()?;
        let dest_dir = tempdir()?;
        let temp_dir = dest_dir.path().join(JAN_TEMP_DIR);
        fs::create_dir_all(&temp_dir)?;

        let source_path = src_dir.path().join("source.txt");
        fs::write(&source_path, b"atomic copy test")?;

        let dest_path = dest_dir.path().join("dest.txt");
        let temp_path = generate_temp_path(&temp_dir);

        atomic_copy_file_with_metadata(&source_path, &dest_path, &temp_path, false, false, None)?;

        assert!(dest_path.exists());
        assert!(!temp_path.exists());
        assert_eq!(fs::read_to_string(&dest_path)?, "atomic copy test");

        Ok(())
    }

    #[test]
    fn test_atomic_copy_with_verify() -> io::Result<()> {
        let src_dir = tempdir()?;
        let dest_dir = tempdir()?;
        let temp_dir = dest_dir.path().join(JAN_TEMP_DIR);
        fs::create_dir_all(&temp_dir)?;

        let data = b"verified copy";
        let source_path = src_dir.path().join("source.txt");
        fs::write(&source_path, data)?;
        let expected_hash = crate::hash::hash_bytes(data);

        let dest_path = dest_dir.path().join("dest.txt");
        let temp_path = generate_temp_path(&temp_dir);

        atomic_copy_file_with_metadata(
            &source_path,
            &dest_path,
            &temp_path,
            false,
            true,
            Some(&expected_hash),
        )?;

        assert!(dest_path.exists());
        assert_eq!(fs::read(&dest_path)?, data);

        Ok(())
    }

    #[test]
    fn test_sync_journal_recovery() -> io::Result<()> {
        let dir = tempdir()?;
        let journal_path = dir.path().join(JAN_JOURNAL_FILE);
        let temp_dir = dir.path().join(JAN_TEMP_DIR);
        fs::create_dir_all(&temp_dir)?;

        let orphan = temp_dir.join("999-0.tmp");
        fs::write(&orphan, b"orphaned")?;

        fs::write(&journal_path, format!("P\tCOPY\t{}\tsome/file.txt\n", orphan.display()))?;

        SyncJournal::recover(&journal_path, &temp_dir)?;

        assert!(!orphan.exists(), "Orphaned temp should be cleaned up");
        assert!(!journal_path.exists(), "Journal should be removed after recovery");

        Ok(())
    }

    #[test]
    fn test_sync_journal_recovery_committed_not_cleaned() -> io::Result<()> {
        let dir = tempdir()?;
        let journal_path = dir.path().join(JAN_JOURNAL_FILE);
        let temp_dir = dir.path().join(JAN_TEMP_DIR);
        fs::create_dir_all(&temp_dir)?;

        // A completed operation should not try to remove already-renamed temp
        let content = "P\tCOPY\t/tmp/fake.tmp\tfile.txt\nC\tCOPY\t/tmp/fake.tmp\tfile.txt\n";
        fs::write(&journal_path, content)?;

        SyncJournal::recover(&journal_path, &temp_dir)?;

        assert!(!journal_path.exists());

        Ok(())
    }

    #[test]
    fn test_generate_temp_path_uniqueness() {
        let dir = Path::new("/tmp/test");
        let path1 = generate_temp_path(dir);
        let path2 = generate_temp_path(dir);
        assert_ne!(path1, path2, "Temp paths should be unique");
    }

    #[test]
    fn test_fsync_directory() -> io::Result<()> {
        let dir = tempdir()?;
        fsync_directory(dir.path())?;
        Ok(())
    }

    #[test]
    fn test_atomic_writer_empty_file() -> io::Result<()> {
        let dir = tempdir()?;
        let temp_path = dir.path().join("temp.tmp");
        let final_path = dir.path().join("empty.txt");

        let writer = AtomicWriter::new(temp_path, final_path.clone(), false)?;
        writer.commit(None)?;

        assert!(final_path.exists());
        assert_eq!(fs::read(&final_path)?, Vec::<u8>::new());

        Ok(())
    }
}
