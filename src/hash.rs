//! Content hashing with BLAKE3 (default) or SHA-256
//!
//! BLAKE3: ~10 GB/s single-threaded, highly parallelizable
//! SHA-256: ~500 MB/s single-threaded
//!
//! Streaming I/O ensures constant memory usage regardless of file size.

use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

// 256KB: optimal for SSD read-ahead and BLAKE3 chunk processing
const HASH_BUFFER_SIZE: usize = 256 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ContentHash {
    #[cfg(feature = "blake3")]
    Blake3([u8; 32]),

    #[cfg(feature = "sha256")]
    Sha256([u8; 32]),
}

impl ContentHash {
    /// Get hash bytes as a slice
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            #[cfg(feature = "blake3")]
            ContentHash::Blake3(bytes) => bytes,
            #[cfg(feature = "sha256")]
            ContentHash::Sha256(bytes) => bytes,
        }
    }

    /// Get hash algorithm name
    pub fn algorithm(&self) -> &'static str {
        match self {
            #[cfg(feature = "blake3")]
            ContentHash::Blake3(_) => "BLAKE3",
            #[cfg(feature = "sha256")]
            ContentHash::Sha256(_) => "SHA-256",
        }
    }
}

impl fmt::Display for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            #[cfg(feature = "blake3")]
            ContentHash::Blake3(bytes) => {
                for byte in bytes {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            },
            #[cfg(feature = "sha256")]
            ContentHash::Sha256(bytes) => {
                for byte in bytes {
                    write!(f, "{byte:02x}")?;
                }
                Ok(())
            },
        }
    }
}

/// A hasher that can compute content hashes using streaming I/O
///
/// The hasher uses the default algorithm based on feature flags:
/// - BLAKE3 if `blake3` feature is enabled (default)
/// - SHA-256 if `sha256` feature is enabled
///
/// ## Example
///
/// ```no_run
/// use janice::hash::Hasher;
/// use std::path::Path;
///
/// # fn main() -> std::io::Result<()> {
/// let mut hasher = Hasher::new();
/// hasher.hash_file(Path::new("file.txt"))?;
/// let hash = hasher.finalize();
/// println!("Hash: {}", hash);
/// # Ok(())
/// # }
/// ```
pub struct Hasher {
    inner: HasherImpl,
}

/// Internal hasher implementation
#[allow(dead_code)]
enum HasherImpl {
    #[cfg(feature = "blake3")]
    Blake3(Box<blake3::Hasher>),

    #[cfg(feature = "sha256")]
    Sha256(sha2::Sha256),
}

impl Hasher {
    /// Create a new hasher with the default algorithm
    pub fn new() -> Self {
        #[cfg(feature = "blake3")]
        {
            Self {
                inner: HasherImpl::Blake3(Box::new(blake3::Hasher::new())),
            }
        }

        #[cfg(all(feature = "sha256", not(feature = "blake3")))]
        {
            use sha2::Digest;
            Self {
                inner: HasherImpl::Sha256(sha2::Sha256::new()),
            }
        }

        #[cfg(not(any(feature = "blake3", feature = "sha256")))]
        {
            compile_error!("At least one hash algorithm must be enabled");
        }
    }

    /// Update hasher with data from a byte slice
    pub fn update(&mut self, data: &[u8]) {
        match &mut self.inner {
            #[cfg(feature = "blake3")]
            HasherImpl::Blake3(hasher) => {
                hasher.update(data);
            },
            #[cfg(feature = "sha256")]
            HasherImpl::Sha256(hasher) => {
                use sha2::Digest;
                hasher.update(data);
            },
        }
    }

    /// Hash the contents of a file using streaming I/O
    ///
    /// This method reads the file in chunks of HASH_BUFFER_SIZE bytes,
    /// ensuring constant memory usage regardless of file size.
    ///
    /// # Performance
    ///
    /// - Uses double-buffered I/O (BufReader + manual buffer) for optimal throughput
    /// - 256KB buffer size optimized for modern SSDs and BLAKE3
    /// - Minimal allocations (single reusable buffer)
    /// - Efficient for files of any size (KB to TB)
    /// - Typical throughput: 2-4 GB/s on modern hardware
    pub fn hash_file(&mut self, path: &Path) -> io::Result<()> {
        let file = File::open(path)?;
        let mut reader = BufReader::with_capacity(HASH_BUFFER_SIZE, file);
        let mut buffer = vec![0u8; HASH_BUFFER_SIZE];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }
            self.update(&buffer[..bytes_read]);
        }

        Ok(())
    }

    /// Finalize the hash and return the result
    ///
    /// This consumes the hasher and returns the computed hash.
    pub fn finalize(self) -> ContentHash {
        match self.inner {
            #[cfg(feature = "blake3")]
            HasherImpl::Blake3(hasher) => {
                let hash = hasher.finalize();
                ContentHash::Blake3(*hash.as_bytes())
            },
            #[cfg(feature = "sha256")]
            HasherImpl::Sha256(hasher) => {
                use sha2::Digest;
                let hash = hasher.finalize();
                let mut bytes = [0u8; 32];
                bytes.copy_from_slice(&hash);
                ContentHash::Sha256(bytes)
            },
        }
    }
}

impl Default for Hasher {
    fn default() -> Self {
        Self::new()
    }
}

/// Hash a file and return the content hash
///
/// Convenience function that creates a hasher, hashes the file, and returns the result.
///
/// # Example
///
/// ```no_run
/// use janice::hash::hash_file;
/// use std::path::Path;
///
/// # fn main() -> std::io::Result<()> {
/// let hash = hash_file(Path::new("file.txt"))?;
/// println!("File hash: {}", hash);
/// # Ok(())
/// # }
/// ```
pub fn hash_file(path: &Path) -> io::Result<ContentHash> {
    let mut hasher = Hasher::new();
    hasher.hash_file(path)?;
    Ok(hasher.finalize())
}

/// Hash bytes and return the content hash
///
/// Convenience function for hashing in-memory data.
pub fn hash_bytes(data: &[u8]) -> ContentHash {
    let mut hasher = Hasher::new();
    hasher.update(data);
    hasher.finalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_hash_empty() {
        let hash = hash_bytes(b"");
        assert!(!hash.as_bytes().is_empty());
    }

    #[test]
    fn test_hash_consistency() {
        let data = b"Hello, Janice!";
        let hash1 = hash_bytes(data);
        let hash2 = hash_bytes(data);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_hash_different_data() {
        let hash1 = hash_bytes(b"foo");
        let hash2 = hash_bytes(b"bar");
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_file_streaming() -> io::Result<()> {
        // Create a temporary file
        let mut temp_file = NamedTempFile::new()?;
        let data = b"This is test data for streaming hash";
        temp_file.write_all(data)?;
        temp_file.flush()?;

        // Hash the file
        let file_hash = hash_file(temp_file.path())?;

        // Hash the same data in memory
        let memory_hash = hash_bytes(data);

        // Should produce the same hash
        assert_eq!(file_hash, memory_hash);

        Ok(())
    }

    #[test]
    fn test_hash_display() {
        let hash = hash_bytes(b"test");
        let hash_str = format!("{}", hash);
        assert_eq!(hash_str.len(), 64); // 32 bytes = 64 hex chars
        assert!(hash_str.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_incremental_hashing() {
        let mut hasher1 = Hasher::new();
        hasher1.update(b"Hello, ");
        hasher1.update(b"World!");
        let hash1 = hasher1.finalize();

        let hash2 = hash_bytes(b"Hello, World!");

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_large_file_streaming() -> io::Result<()> {
        // Create a large temporary file (larger than buffer size)
        let mut temp_file = NamedTempFile::new()?;
        let chunk = vec![0x42u8; HASH_BUFFER_SIZE];

        // Write multiple chunks
        for _ in 0..10 {
            temp_file.write_all(&chunk)?;
        }
        temp_file.flush()?;

        // Hash should complete without error
        let hash = hash_file(temp_file.path())?;
        assert!(!hash.as_bytes().is_empty());

        Ok(())
    }

    #[test]
    fn test_algorithm_name() {
        let hash = hash_bytes(b"test");
        let algo = hash.algorithm();
        assert!(algo == "BLAKE3" || algo == "SHA-256");
    }
}
