//! Unit tests for hashing functionality

use janice::hash::{hash_bytes, hash_file, Hasher};
use std::io::Write;
use tempfile::NamedTempFile;

#[test]
fn test_hash_empty_data() {
    let hash1 = hash_bytes(b"");
    let hash2 = hash_bytes(b"");

    assert_eq!(hash1, hash2, "Empty data should produce consistent hashes");
    assert!(!hash1.as_bytes().is_empty(), "Hash should not be empty");
}

#[test]
fn test_hash_consistency() {
    let data = b"The quick brown fox jumps over the lazy dog";

    let hash1 = hash_bytes(data);
    let hash2 = hash_bytes(data);
    let hash3 = hash_bytes(data);

    assert_eq!(hash1, hash2);
    assert_eq!(hash2, hash3);
}

#[test]
fn test_hash_uniqueness() {
    let hash1 = hash_bytes(b"foo");
    let hash2 = hash_bytes(b"bar");
    let hash3 = hash_bytes(b"baz");

    assert_ne!(hash1, hash2);
    assert_ne!(hash2, hash3);
    assert_ne!(hash1, hash3);
}

#[test]
fn test_hash_sensitivity() {
    // Even a single bit difference should produce different hashes
    let hash1 = hash_bytes(b"test");
    let hash2 = hash_bytes(b"Test");
    let hash3 = hash_bytes(b"test ");
    let hash4 = hash_bytes(b"tes");

    assert_ne!(hash1, hash2, "Case difference should change hash");
    assert_ne!(hash1, hash3, "Trailing space should change hash");
    assert_ne!(hash1, hash4, "Missing character should change hash");
}

#[test]
fn test_incremental_hashing_equivalence() {
    let full_data = b"Hello, World! This is a test of incremental hashing.";

    // Hash all at once
    let hash_full = hash_bytes(full_data);

    // Hash incrementally
    let mut hasher = Hasher::new();
    hasher.update(b"Hello, World! ");
    hasher.update(b"This is a test ");
    hasher.update(b"of incremental hashing.");
    let hash_incremental = hasher.finalize();

    assert_eq!(
        hash_full, hash_incremental,
        "Incremental hashing should produce same result as full hashing"
    );
}

#[test]
fn test_file_hashing_small() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;
    let data = b"Small file content for testing";
    temp_file.write_all(data)?;
    temp_file.flush()?;

    let file_hash = hash_file(temp_file.path())?;
    let memory_hash = hash_bytes(data);

    assert_eq!(file_hash, memory_hash, "File hash should match in-memory hash");

    Ok(())
}

#[test]
fn test_file_hashing_large() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;

    // Create a 1MB file
    let chunk = vec![0x42u8; 1024];
    for _ in 0..1024 {
        temp_file.write_all(&chunk)?;
    }
    temp_file.flush()?;

    // Hash the file
    let file_hash = hash_file(temp_file.path())?;

    // Hash the same pattern in memory
    let full_data = vec![0x42u8; 1024 * 1024];
    let memory_hash = hash_bytes(&full_data);

    assert_eq!(file_hash, memory_hash, "Large file hash should match in-memory hash");

    Ok(())
}

#[test]
fn test_file_hashing_empty() -> std::io::Result<()> {
    let temp_file = NamedTempFile::new()?;
    // Don't write anything - empty file

    let file_hash = hash_file(temp_file.path())?;
    let empty_hash = hash_bytes(b"");

    assert_eq!(file_hash, empty_hash, "Empty file hash should match empty data hash");

    Ok(())
}

#[test]
fn test_hash_display_format() {
    let hash = hash_bytes(b"test data");
    let hash_str = format!("{}", hash);

    // Should be 64 hex characters (32 bytes * 2 chars per byte)
    assert_eq!(hash_str.len(), 64);

    // All characters should be valid hex
    for c in hash_str.chars() {
        assert!(c.is_ascii_hexdigit(), "Character '{}' is not hex", c);
    }

    // Should be lowercase
    for c in hash_str.chars() {
        if c.is_ascii_alphabetic() {
            assert!(c.is_lowercase(), "Hex should be lowercase");
        }
    }
}

#[test]
fn test_hash_algorithm_name() {
    let hash = hash_bytes(b"test");
    let algo = hash.algorithm();

    // Should be one of the supported algorithms
    assert!(algo == "BLAKE3" || algo == "SHA-256", "Unknown algorithm: {}", algo);
}

#[test]
fn test_hash_as_bytes() {
    let hash = hash_bytes(b"test data");
    let bytes = hash.as_bytes();

    // Should be 32 bytes (256 bits)
    assert_eq!(bytes.len(), 32);
}

#[test]
fn test_multiple_files_different_hashes() -> std::io::Result<()> {
    let mut file1 = NamedTempFile::new()?;
    let mut file2 = NamedTempFile::new()?;
    let mut file3 = NamedTempFile::new()?;

    file1.write_all(b"Content A")?;
    file2.write_all(b"Content B")?;
    file3.write_all(b"Content C")?;

    file1.flush()?;
    file2.flush()?;
    file3.flush()?;

    let hash1 = hash_file(file1.path())?;
    let hash2 = hash_file(file2.path())?;
    let hash3 = hash_file(file3.path())?;

    assert_ne!(hash1, hash2);
    assert_ne!(hash2, hash3);
    assert_ne!(hash1, hash3);

    Ok(())
}

#[test]
fn test_streaming_buffer_boundary() -> std::io::Result<()> {
    // Test with data that spans multiple buffer reads
    // Hash buffer is 64KB, so test with sizes around that boundary

    let mut temp_file = NamedTempFile::new()?;

    // Write exactly 64KB
    let data_64k = vec![0xAAu8; 64 * 1024];
    temp_file.write_all(&data_64k)?;
    temp_file.flush()?;

    let file_hash = hash_file(temp_file.path())?;
    let memory_hash = hash_bytes(&data_64k);

    assert_eq!(file_hash, memory_hash);

    // Test with 64KB + 1
    let mut temp_file2 = NamedTempFile::new()?;
    let mut data_64k_plus = vec![0xAAu8; 64 * 1024];
    data_64k_plus.push(0xBB);
    temp_file2.write_all(&data_64k_plus)?;
    temp_file2.flush()?;

    let file_hash2 = hash_file(temp_file2.path())?;
    let memory_hash2 = hash_bytes(&data_64k_plus);

    assert_eq!(file_hash2, memory_hash2);
    assert_ne!(file_hash, file_hash2, "Extra byte should change hash");

    Ok(())
}

#[test]
fn test_hash_determinism_across_runs() {
    // Run hashing multiple times to ensure determinism
    let data = b"Determinism test data";
    let mut hashes = Vec::new();

    for _ in 0..10 {
        let hash = hash_bytes(data);
        hashes.push(hash);
    }

    // All hashes should be identical
    for i in 1..hashes.len() {
        assert_eq!(hashes[0], hashes[i], "Hash should be deterministic across runs");
    }
}

#[test]
fn test_hash_clone_equality() {
    let hash1 = hash_bytes(b"test");
    let hash2 = hash1.clone();

    assert_eq!(hash1, hash2);
    assert_eq!(hash1.as_bytes(), hash2.as_bytes());
}

#[test]
fn test_very_large_file() -> std::io::Result<()> {
    let mut temp_file = NamedTempFile::new()?;

    // Create a 10MB file to test memory efficiency
    let chunk = vec![0x55u8; 1024 * 1024]; // 1MB chunks
    for _ in 0..10 {
        temp_file.write_all(&chunk)?;
    }
    temp_file.flush()?;

    // This should complete without running out of memory
    let hash = hash_file(temp_file.path())?;
    assert!(!hash.as_bytes().is_empty());

    Ok(())
}

#[test]
fn test_hasher_reuse() {
    // Ensure hasher is consumed after finalize
    let mut hasher = Hasher::new();
    hasher.update(b"test");
    let _hash = hasher.finalize();

    // Can't use hasher after finalize (it's consumed)
    // This test just ensures the API works as expected
}
