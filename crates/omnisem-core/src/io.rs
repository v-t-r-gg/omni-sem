//! Stable, size-bounded source file reads.

use std::fs;
use std::io::Read;
use std::path::Path;

use crate::domain::Timestamp;
use crate::error::ReadError;

/// Bytes read under a metadata stability check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableFileBytes {
    pub bytes: Vec<u8>,
    pub size_bytes: u64,
    pub modified_at: Timestamp,
}

/// Reads a regular file only when metadata is stable across the read.
///
/// # Errors
///
/// Returns [`ReadError::ChangedDuringRead`] when size or modification time changes,
/// [`ReadError::Oversized`] when the file exceeds the limit, or I/O failures.
pub fn read_stable_file(path: &Path, max_size_bytes: u64) -> Result<StableFileBytes, ReadError> {
    let before = metadata_snapshot(path)?;
    if before.size_bytes > max_size_bytes {
        return Err(ReadError::Oversized {
            size_bytes: before.size_bytes,
        });
    }

    let mut file = fs::File::open(path).map_err(|error| ReadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut bytes = Vec::new();
    file.by_ref()
        .take(max_size_bytes.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| ReadError::Io {
            path: path.to_path_buf(),
            message: error.to_string(),
        })?;

    let read_len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
    if read_len > max_size_bytes {
        return Err(ReadError::Oversized {
            size_bytes: read_len,
        });
    }

    let after = metadata_snapshot(path)?;
    if after.size_bytes != before.size_bytes
        || after.modified_at != before.modified_at
        || read_len != before.size_bytes
    {
        return Err(ReadError::ChangedDuringRead);
    }

    Ok(StableFileBytes {
        bytes,
        size_bytes: before.size_bytes,
        modified_at: before.modified_at,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MetaSnapshot {
    size_bytes: u64,
    modified_at: Timestamp,
}

fn metadata_snapshot(path: &Path) -> Result<MetaSnapshot, ReadError> {
    let metadata = fs::metadata(path).map_err(|error| ReadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if !metadata.is_file() {
        return Err(ReadError::NotRegularFile(path.to_path_buf()));
    }
    let modified = metadata.modified().map_err(|error| ReadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    let modified_at = Timestamp::try_from_system_time(modified).map_err(|error| ReadError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    Ok(MetaSnapshot {
        size_bytes: metadata.len(),
        modified_at,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn reads_stable_contents() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(b"stable-bytes").unwrap();
        file.flush().unwrap();
        let read = read_stable_file(file.path(), 1_024).unwrap();
        assert_eq!(read.bytes, b"stable-bytes");
        assert_eq!(read.size_bytes, 12);
    }

    #[test]
    fn rejects_oversized_files() {
        let mut file = NamedTempFile::new().unwrap();
        file.write_all(&[b'x'; 64]).unwrap();
        file.flush().unwrap();
        let error = read_stable_file(file.path(), 8).unwrap_err();
        assert!(matches!(error, ReadError::Oversized { .. }));
    }
}
