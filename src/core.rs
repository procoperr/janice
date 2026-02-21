//! Core synchronization logic for scanning, diffing, and syncing directories.

use crate::hash::{ContentHash, Hasher};
use crate::io::{
    atomic_copy_file_with_metadata, fsync_directory, generate_temp_path, remove_file_safe,
    SyncJournal, JAN_JOURNAL_FILE, JAN_TEMP_DIR,
};
use ahash::{HashMap, HashMapExt};
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;
use thiserror::Error;

/// Errors that can occur during synchronization operations
#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Failed to read directory: {0}")]
    DirectoryRead(String),

    #[error("Failed to hash file: {0}")]
    HashError(String),

    #[error("Failed to copy file: {0}")]
    CopyError(String),

    #[error("Invalid path: {0}")]
    InvalidPath(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Metadata for a single file including content hash
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMeta {
    /// Relative path from scan root
    pub path: PathBuf,
    /// File size in bytes
    pub size: u64,
    /// Last modified time
    pub mtime: SystemTime,
    /// Content hash (BLAKE3 or SHA-256)
    pub hash: ContentHash,
    /// Unix permissions (if available)
    pub permissions: Option<u32>,
}

/// Result of scanning a directory
#[derive(Debug, Clone)]
pub struct ScanResult {
    /// Root directory that was scanned
    pub root: PathBuf,
    /// List of all files found
    pub files: Vec<FileMeta>,
    /// Timestamp when scan was performed
    pub scan_time: SystemTime,
}

impl ScanResult {
    /// Calculate total size of all files
    pub fn total_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// Result of comparing two scans
#[derive(Debug, Clone)]
pub struct DiffResult {
    /// Files present in source but not in destination
    pub added: Vec<FileMeta>,
    /// Files present in destination but not in source
    pub removed: Vec<FileMeta>,
    /// Files present in both but with different content
    pub modified: Vec<FileMeta>,
    /// Files that were renamed (old, new)
    pub renamed: Vec<(FileMeta, FileMeta)>,
}

/// Options for sync operations
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Delete files in destination not present in source
    pub delete_removed: bool,
    /// Preserve file timestamps
    pub preserve_timestamps: bool,
    /// Verify file hash after copying
    pub verify_after_copy: bool,
}

impl Default for SyncOptions {
    fn default() -> Self {
        Self {
            delete_removed: false,
            preserve_timestamps: true,
            verify_after_copy: false,
        }
    }
}

/// Scan a directory and compute content hashes for all files
///
/// This function walks the directory tree in parallel, computing content hashes
/// for each file using streaming I/O to minimize memory usage.
///
/// # Arguments
///
/// * `root` - Root directory to scan
/// * `exclude_patterns` - Optional glob patterns to exclude from scan
///
/// # Performance
///
/// - Uses `ignore` crate for parallel directory traversal
/// - Hashes files in parallel using `rayon`
/// - Streaming hash computation for constant memory usage
/// - Respects .gitignore patterns for efficiency
pub fn scan_directory(root: &Path) -> Result<ScanResult> {
    scan_directory_with_excludes(root, &[])
}

/// Scan a directory with custom exclude patterns
pub fn scan_directory_with_excludes(
    root: &Path,
    exclude_patterns: &[String],
) -> Result<ScanResult> {
    if !root.exists() {
        return Err(SyncError::InvalidPath(format!(
            "Directory does not exist: {}",
            root.display()
        ))
        .into());
    }

    let mut builder = ignore::WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_exclude(true)
        .threads(std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1));

    // Add custom exclude patterns
    let mut override_builder = ignore::overrides::OverrideBuilder::new(root);
    for pattern in exclude_patterns {
        override_builder.add(&format!("!{}", pattern)).map_err(|e| {
            SyncError::InvalidPath(format!("Invalid exclude pattern '{}': {}", pattern, e))
        })?;
    }

    // Auto-exclude Janice internal files
    override_builder
        .add(&format!("!{JAN_TEMP_DIR}"))
        .map_err(|e| SyncError::InvalidPath(format!("Internal exclude failed: {e}")))?;
    override_builder
        .add(&format!("!{JAN_JOURNAL_FILE}"))
        .map_err(|e| SyncError::InvalidPath(format!("Internal exclude failed: {e}")))?;

    if let Ok(overrides) = override_builder.build() {
        builder.overrides(overrides);
    }

    let walker = builder.build_parallel();

    let files = std::sync::Mutex::new(Vec::with_capacity(1024));

    walker.run(|| {
        Box::new(|entry_result| {
            if let Ok(entry) = entry_result {
                if let Some(file_type) = entry.file_type() {
                    if file_type.is_file() {
                        files.lock().unwrap().push(entry.path().to_path_buf());
                    }
                }
            }
            ignore::WalkState::Continue
        })
    });

    let file_paths = files.into_inner().unwrap();

    // Hash files in parallel
    let file_metas: Vec<Result<FileMeta>> = file_paths
        .par_iter()
        .map(|path| {
            let metadata = fs::metadata(path)?;
            let size = metadata.len();
            let mtime = metadata.modified()?;

            // Get permissions on Unix systems
            #[cfg(unix)]
            let permissions = {
                use std::os::unix::fs::PermissionsExt;
                Some(metadata.permissions().mode())
            };
            #[cfg(not(unix))]
            let permissions = None;

            // Compute content hash using streaming
            let mut hasher = Hasher::new();
            hasher.hash_file(path)?;
            let hash = hasher.finalize();

            // Make path relative to root
            let rel_path = path
                .strip_prefix(root)
                .map_err(|_| {
                    SyncError::InvalidPath(format!("Path not under root: {}", path.display()))
                })?
                .to_path_buf();

            Ok(FileMeta {
                path: rel_path,
                size,
                mtime,
                hash,
                permissions,
            })
        })
        .collect();

    // Collect results, logging errors but not failing the entire scan
    let mut successful_files = Vec::new();
    let mut error_count = 0;

    for result in file_metas {
        match result {
            Ok(meta) => successful_files.push(meta),
            Err(e) => {
                error_count += 1;
                eprintln!("Warning: Failed to process file: {e}");
            },
        }
    }

    if error_count > 0 {
        eprintln!("Warning: {error_count} files could not be processed");
    }

    Ok(ScanResult {
        root: root.to_path_buf(),
        files: successful_files,
        scan_time: SystemTime::now(),
    })
}

/// Compare two scan results and identify differences
///
/// This function performs intelligent diff computation with rename detection:
/// 1. Build hash maps for fast lookup
/// 2. Identify added/removed/modified files
/// 3. Detect renames by matching content hashes
/// 4. Use path similarity as fallback for ambiguous renames
///
/// # Performance
///
/// - O(n) hash map construction
/// - O(1) lookups for most operations
/// - Rename detection is O(n*m) worst case but typically O(n) with hash matching
pub fn diff_scans(source: &ScanResult, dest: &ScanResult) -> Result<DiffResult> {
    // O(n) lookups via hash maps
    let source_by_path: HashMap<&PathBuf, &FileMeta> =
        HashMap::from_iter(source.files.iter().map(|f| (&f.path, f)));
    let dest_by_path: HashMap<&PathBuf, &FileMeta> =
        HashMap::from_iter(dest.files.iter().map(|f| (&f.path, f)));

    let mut source_by_hash: HashMap<&ContentHash, Vec<&FileMeta>> =
        HashMap::with_capacity(source.files.len());
    for file in &source.files {
        source_by_hash
            .entry(&file.hash)
            .or_insert_with(|| Vec::with_capacity(2))
            .push(file);
    }

    let mut dest_by_hash: HashMap<&ContentHash, Vec<&FileMeta>> =
        HashMap::with_capacity(dest.files.len());
    for file in &dest.files {
        dest_by_hash
            .entry(&file.hash)
            .or_insert_with(|| Vec::with_capacity(2))
            .push(file);
    }

    let mut added = Vec::with_capacity(source.files.len() / 10);
    let mut removed = Vec::with_capacity(dest.files.len() / 10);
    let mut modified = Vec::with_capacity(source.files.len() / 20);
    let mut renamed = Vec::with_capacity(source.files.len() / 50);
    let mut processed_dest_paths = HashSet::with_capacity(dest.files.len());

    for source_file in &source.files {
        if let Some(dest_file) = dest_by_path.get(&source_file.path) {
            if source_file.hash != dest_file.hash {
                modified.push(source_file.clone());
            }
            processed_dest_paths.insert(&dest_file.path);
        } else {
            // Not at same path - check if renamed or new
            if let Some(dest_files_with_hash) = dest_by_hash.get(&source_file.hash) {
                // Same content exists - find best path match
                let mut best_match: Option<&FileMeta> = None;
                let mut best_score = 0.0;

                for candidate in dest_files_with_hash {
                    if processed_dest_paths.contains(&candidate.path) {
                        continue;
                    }

                    let score = path_similarity(&source_file.path, &candidate.path);
                    if score > best_score {
                        best_score = score;
                        best_match = Some(candidate);
                    }
                }

                if let Some(matched_dest) = best_match {
                    renamed.push(((*matched_dest).clone(), source_file.clone()));
                    processed_dest_paths.insert(&matched_dest.path);
                } else {
                    added.push(source_file.clone());
                }
            } else {
                added.push(source_file.clone());
            }
        }
    }

    // Find removed files (in dest but not in source, and not part of a rename)
    for dest_file in &dest.files {
        if !source_by_path.contains_key(&dest_file.path)
            && !processed_dest_paths.contains(&dest_file.path)
        {
            removed.push(dest_file.clone());
        }
    }

    Ok(DiffResult { added, removed, modified, renamed })
}

/// Compute path similarity score between two paths (0.0 to 1.0)
///
/// Uses Damerau-Levenshtein distance for accurate rename detection.
/// Handles typos, case changes, and partial renames correctly.
fn path_similarity(path1: &Path, path2: &Path) -> f64 {
    let name1 = path1.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let name2 = path2.file_name().and_then(|n| n.to_str()).unwrap_or("");

    // Exact filename match (case-insensitive)
    if name1.eq_ignore_ascii_case(name2) {
        return 0.95;
    }

    // Use Damerau-Levenshtein for filename similarity
    let filename_sim = strsim::normalized_damerau_levenshtein(name1, name2);

    // Directory similarity (keep Jaccard for speed)
    let dir1 = path1.parent().map(|p| p.to_string_lossy());
    let dir2 = path2.parent().map(|p| p.to_string_lossy());

    let dir_sim = match (dir1, dir2) {
        (Some(d1), Some(d2)) => simple_string_similarity(&d1, &d2),
        _ => 0.0,
    };

    // Weight filename more heavily than directory
    filename_sim * 0.7 + dir_sim * 0.3
}

/// Jaccard similarity on character sets
fn simple_string_similarity(s1: &str, s2: &str) -> f64 {
    if s1 == s2 {
        return 1.0;
    }
    if s1.is_empty() || s2.is_empty() {
        return 0.0;
    }

    let mut chars1: HashSet<char> = HashSet::with_capacity(s1.len());
    chars1.extend(s1.chars());

    let mut chars2: HashSet<char> = HashSet::with_capacity(s2.len());
    chars2.extend(s2.chars());

    let intersection = chars1.intersection(&chars2).count();
    let union = chars1.union(&chars2).count();

    if union == 0 {
        0.0
    } else {
        intersection as f64 / union as f64
    }
}

/// Synchronize changes from source to destination based on diff results
///
/// Uses atomic file operations (write-to-temp, fsync, rename) to ensure
/// the destination is never left in a corrupted state. A journal tracks
/// in-progress operations for crash recovery.
///
/// # Arguments
///
/// * `source_root` - Source directory root
/// * `dest_root` - Destination directory root
/// * `diff` - Diff results to apply
/// * `options` - Sync options
pub fn sync_changes(
    source_root: &Path,
    dest_root: &Path,
    diff: &DiffResult,
    options: &SyncOptions,
) -> Result<()> {
    let temp_dir = dest_root.join(JAN_TEMP_DIR);
    let journal_path = dest_root.join(JAN_JOURNAL_FILE);

    // Recover from any prior interrupted sync
    SyncJournal::recover(&journal_path, &temp_dir)
        .map_err(|e| anyhow::anyhow!("Journal recovery failed: {e}"))?;

    fs::create_dir_all(&temp_dir)
        .map_err(|e| anyhow::anyhow!("Can't create {}: {}", temp_dir.display(), e))?;

    let journal = SyncJournal::create(journal_path)
        .map_err(|e| anyhow::anyhow!("Can't create journal: {e}"))?;

    // Track directories that were written to for batch dir fsync
    let written_dirs: std::sync::Mutex<HashSet<PathBuf>> = std::sync::Mutex::new(HashSet::new());

    // Copy added + modified files
    let files_to_copy: Vec<&FileMeta> = diff.added.iter().chain(diff.modified.iter()).collect();

    let copy_result = files_to_copy.par_iter().try_for_each(|file| {
        let source_path = source_root.join(&file.path);
        let dest_path = dest_root.join(&file.path);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Can't create {}: {}", parent.display(), e))?;
            written_dirs.lock().unwrap().insert(parent.to_path_buf());
        }

        let temp_path = generate_temp_path(&temp_dir);
        journal
            .record_pending("COPY", &temp_path, &dest_path)
            .map_err(|e| anyhow::anyhow!("Journal write failed: {e}"))?;

        let expected_hash = if options.verify_after_copy {
            Some(&file.hash)
        } else {
            None
        };

        atomic_copy_file_with_metadata(
            &source_path,
            &dest_path,
            &temp_path,
            options.preserve_timestamps,
            options.verify_after_copy,
            expected_hash,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "Copy failed ({} -> {}): {e}",
                source_path.display(),
                dest_path.display(),
            )
        })?;

        journal
            .record_committed("COPY", &temp_path, &dest_path)
            .map_err(|e| anyhow::anyhow!("Journal write failed: {e}"))?;

        Ok::<_, anyhow::Error>(())
    });

    if let Err(e) = copy_result {
        let _ = journal.remove();
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    // Renames: atomic copy to new location, remove old
    let rename_result = diff.renamed.par_iter().try_for_each(|(old, new)| {
        let source_path = source_root.join(&new.path);
        let dest_path = dest_root.join(&new.path);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("Can't create {}: {}", parent.display(), e))?;
            written_dirs.lock().unwrap().insert(parent.to_path_buf());
        }

        let temp_path = generate_temp_path(&temp_dir);
        journal
            .record_pending("RENAME", &temp_path, &dest_path)
            .map_err(|e| anyhow::anyhow!("Journal write failed: {e}"))?;

        let expected_hash = if options.verify_after_copy {
            Some(&new.hash)
        } else {
            None
        };

        atomic_copy_file_with_metadata(
            &source_path,
            &dest_path,
            &temp_path,
            options.preserve_timestamps,
            options.verify_after_copy,
            expected_hash,
        )
        .map_err(|e| {
            anyhow::anyhow!(
                "Rename failed ({} -> {}): {e}",
                source_path.display(),
                dest_path.display(),
            )
        })?;

        let old_dest_path = dest_root.join(&old.path);
        remove_file_safe(&old_dest_path)
            .map_err(|e| anyhow::anyhow!("Can't remove {}: {}", old_dest_path.display(), e))?;

        journal
            .record_committed("RENAME", &temp_path, &dest_path)
            .map_err(|e| anyhow::anyhow!("Journal write failed: {e}"))?;

        Ok::<_, anyhow::Error>(())
    });

    if let Err(e) = rename_result {
        let _ = journal.remove();
        let _ = fs::remove_dir_all(&temp_dir);
        return Err(e);
    }

    // Deletes
    if options.delete_removed {
        for file in &diff.removed {
            let dest_path = dest_root.join(&file.path);
            remove_file_safe(&dest_path)
                .map_err(|e| anyhow::anyhow!("Can't delete {}: {}", dest_path.display(), e))?;
            if let Some(parent) = dest_path.parent() {
                written_dirs.lock().unwrap().insert(parent.to_path_buf());
            }
        }
    }

    // Batch directory fsync — persist all renames
    let dirs = written_dirs.into_inner().unwrap();
    for dir in &dirs {
        if let Err(e) = fsync_directory(dir) {
            eprintln!("Warning: directory fsync failed for {}: {e}", dir.display());
        }
    }

    // Clean exit
    let _ = journal.remove();
    let _ = fs::remove_dir_all(&temp_dir);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_similarity() {
        // Exact filename match (case-insensitive with Levenshtein)
        let p1 = Path::new("dir1/file.txt");
        let p2 = Path::new("dir2/file.txt");
        assert!(path_similarity(p1, p2) > 0.9); // Case-insensitive exact match returns 0.95

        // Different files in same directory (directory similarity pulls score up)
        let p1 = Path::new("dir/foo.txt");
        let p2 = Path::new("dir/bar.txt");
        let score = path_similarity(p1, p2);
        assert!(score > 0.3); // Same dir boosts similarity
        assert!(score < 0.75); // But still not very similar (Levenshtein gives ~0.7)
    }

    #[test]
    fn test_string_similarity() {
        assert_eq!(simple_string_similarity("hello", "hello"), 1.0);
        assert_eq!(simple_string_similarity("", ""), 1.0); // Equal empty strings
        assert!(simple_string_similarity("hello", "hallo") > 0.5);
    }
}
