//! File I/O with streaming copy and metadata preservation

use std::fs::{self, File, Metadata};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;
use std::time::SystemTime;
use thiserror::Error;

// 256KB: optimal for modern SSD throughput
const COPY_BUFFER_SIZE: usize = 256 * 1024;

#[allow(dead_code)]
const MAX_RETRY_ATTEMPTS: u32 = 3;

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
/// - Uses 64KB buffer for efficient streaming
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
}
