//! Unit tests for rename detection heuristics

use janice::core::{diff_scans, FileMeta, ScanResult};
use janice::hash::hash_bytes;
use std::path::PathBuf;
use std::time::SystemTime;

/// Helper function to create a FileMeta for testing
fn make_file_meta(path: &str, content: &[u8]) -> FileMeta {
    FileMeta {
        path: PathBuf::from(path),
        size: content.len() as u64,
        mtime: SystemTime::now(),
        hash: hash_bytes(content),
        permissions: None,
    }
}

/// Helper to create a ScanResult from a list of files
fn make_scan(files: Vec<FileMeta>) -> ScanResult {
    ScanResult {
        root: PathBuf::from("/test"),
        files,
        scan_time: SystemTime::now(),
    }
}

#[test]
fn test_simple_rename_detection() {
    // Source has file at one location, dest has same content at different location
    let source_files = vec![make_file_meta("new_name.txt", b"file content")];
    let dest_files = vec![make_file_meta("old_name.txt", b"file content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect one rename");
    assert_eq!(diff.added.len(), 0, "Should not report as added");
    assert_eq!(diff.removed.len(), 0, "Should not report as removed");
    assert_eq!(diff.modified.len(), 0, "Should not report as modified");

    let (old, new) = &diff.renamed[0];
    assert_eq!(old.path, PathBuf::from("old_name.txt"));
    assert_eq!(new.path, PathBuf::from("new_name.txt"));
}

#[test]
fn test_no_rename_when_content_differs() {
    // Different content should not be detected as rename
    let source_files = vec![make_file_meta("file.txt", b"new content")];
    let dest_files = vec![make_file_meta("file_old.txt", b"old content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 0, "Should not detect rename");
    assert_eq!(diff.added.len(), 1, "Should report as added");
    assert_eq!(diff.removed.len(), 1, "Should report as removed");
}

#[test]
fn test_rename_with_directory_change() {
    // File moved to different directory
    let source_files = vec![make_file_meta("subdir/document.pdf", b"PDF content")];
    let dest_files = vec![make_file_meta("old_location/document.pdf", b"PDF content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect rename across directories");
}

#[test]
fn test_no_rename_when_paths_match() {
    // Same path, same content - no change at all
    let files = vec![make_file_meta("file.txt", b"content")];

    let source_scan = make_scan(files.clone());
    let dest_scan = make_scan(files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 0);
    assert_eq!(diff.added.len(), 0);
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(diff.modified.len(), 0);
}

#[test]
fn test_modified_file_not_rename() {
    // Same path, different content - should be modification
    let source_files = vec![make_file_meta("file.txt", b"new content")];
    let dest_files = vec![make_file_meta("file.txt", b"old content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.modified.len(), 1, "Should detect modification");
    assert_eq!(diff.renamed.len(), 0, "Should not detect rename");
}

#[test]
fn test_multiple_renames() {
    // Multiple files renamed
    let source_files = vec![
        make_file_meta("doc1_new.txt", b"content1"),
        make_file_meta("doc2_new.txt", b"content2"),
        make_file_meta("doc3_new.txt", b"content3"),
    ];

    let dest_files = vec![
        make_file_meta("doc1_old.txt", b"content1"),
        make_file_meta("doc2_old.txt", b"content2"),
        make_file_meta("doc3_old.txt", b"content3"),
    ];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 3, "Should detect all three renames");
}

#[test]
fn test_rename_with_same_filename() {
    // File with same name moved to different directory
    let source_files = vec![make_file_meta("new_dir/file.txt", b"content")];
    let dest_files = vec![make_file_meta("old_dir/file.txt", b"content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect rename even with same filename");

    let (old, new) = &diff.renamed[0];
    assert_eq!(old.path, PathBuf::from("old_dir/file.txt"));
    assert_eq!(new.path, PathBuf::from("new_dir/file.txt"));
}

#[test]
fn test_duplicate_content_ambiguous_rename() {
    // Multiple files with same content - should use path similarity
    let source_files = vec![
        make_file_meta("document_v2.txt", b"same content"),
        make_file_meta("other_file.txt", b"different"),
    ];

    let dest_files = vec![
        make_file_meta("document_v1.txt", b"same content"),
        make_file_meta("unrelated.txt", b"same content"),
        make_file_meta("other_file.txt", b"different"),
    ];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    // Should pick the most similar path
    assert!(
        !diff.renamed.is_empty(),
        "Should detect at least one rename based on path similarity"
    );
}

#[test]
fn test_empty_files_rename() {
    // Empty files should still be detected as renamed
    let source_files = vec![make_file_meta("empty_new.txt", b"")];
    let dest_files = vec![make_file_meta("empty_old.txt", b"")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect rename of empty file");
}

#[test]
fn test_large_file_rename() {
    // Large file content - hash should handle efficiently
    let large_content = vec![0x42u8; 1024 * 1024]; // 1MB
    let source_files = vec![make_file_meta("large_new.bin", &large_content)];
    let dest_files = vec![make_file_meta("large_old.bin", &large_content)];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect rename of large file");
}

#[test]
fn test_mixed_operations() {
    // Combination of added, removed, modified, and renamed
    let source_files = vec![
        make_file_meta("renamed_new.txt", b"rename content"),
        make_file_meta("modified.txt", b"new version"),
        make_file_meta("added.txt", b"brand new"),
        make_file_meta("unchanged.txt", b"same"),
    ];

    let dest_files = vec![
        make_file_meta("renamed_old.txt", b"rename content"),
        make_file_meta("modified.txt", b"old version"),
        make_file_meta("removed.txt", b"will be deleted"),
        make_file_meta("unchanged.txt", b"same"),
    ];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect rename");
    assert_eq!(diff.modified.len(), 1, "Should detect modification");
    assert_eq!(diff.added.len(), 1, "Should detect addition");
    assert_eq!(diff.removed.len(), 1, "Should detect removal");
}

#[test]
fn test_no_false_positives() {
    // Completely different files should not be confused
    let source_files = vec![
        make_file_meta("fileA.txt", b"contentA"),
        make_file_meta("fileB.txt", b"contentB"),
    ];

    let dest_files = vec![
        make_file_meta("fileX.txt", b"contentX"),
        make_file_meta("fileY.txt", b"contentY"),
    ];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 0, "Should not detect false renames");
    assert_eq!(diff.added.len(), 2);
    assert_eq!(diff.removed.len(), 2);
}

#[test]
fn test_case_only_rename() {
    // Filename differs only in case
    let source_files = vec![make_file_meta("Document.txt", b"content")];
    let dest_files = vec![make_file_meta("document.txt", b"content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    // On case-sensitive filesystems, this is a rename
    assert_eq!(diff.renamed.len(), 1, "Should detect case-only rename");
}

#[test]
fn test_extension_change() {
    // File extension changed but content same
    let source_files = vec![make_file_meta("document.md", b"markdown content")];
    let dest_files = vec![make_file_meta("document.txt", b"markdown content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect extension change as rename");
}

#[test]
fn test_deep_directory_rename() {
    // File moved deep in directory tree
    let source_files = vec![make_file_meta("a/b/c/d/e/file.txt", b"nested content")];
    let dest_files = vec![make_file_meta("x/y/z/file.txt", b"nested content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should detect rename across deep directories");
}

#[test]
fn test_unicode_filename_rename() {
    // Files with unicode names
    let source_files = vec![make_file_meta("文档_new.txt", b"content")];
    let dest_files = vec![make_file_meta("文档_old.txt", b"content")];

    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 1, "Should handle unicode filenames");
}

#[test]
fn test_empty_scans() {
    // Both scans empty
    let source_scan = make_scan(vec![]);
    let dest_scan = make_scan(vec![]);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.renamed.len(), 0);
    assert_eq!(diff.added.len(), 0);
    assert_eq!(diff.removed.len(), 0);
    assert_eq!(diff.modified.len(), 0);
}

#[test]
fn test_source_empty() {
    // Source empty, dest has files - all should be removed
    let source_scan = make_scan(vec![]);
    let dest_files = vec![
        make_file_meta("file1.txt", b"content1"),
        make_file_meta("file2.txt", b"content2"),
    ];
    let dest_scan = make_scan(dest_files);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.removed.len(), 2);
    assert_eq!(diff.renamed.len(), 0);
}

#[test]
fn test_dest_empty() {
    // Dest empty, source has files - all should be added
    let source_files = vec![
        make_file_meta("file1.txt", b"content1"),
        make_file_meta("file2.txt", b"content2"),
    ];
    let source_scan = make_scan(source_files);
    let dest_scan = make_scan(vec![]);

    let diff = diff_scans(&source_scan, &dest_scan).unwrap();

    assert_eq!(diff.added.len(), 2);
    assert_eq!(diff.renamed.len(), 0);
}
