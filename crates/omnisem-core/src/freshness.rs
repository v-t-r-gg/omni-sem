//! Filesystem freshness inspection without reading source contents.

use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::domain::{FreshnessStatus, Timestamp};

/// Classifies freshness for an indexed relative path under an approved root.
///
/// Does not follow symlinks outside the root and never reads file contents.
#[must_use]
pub fn inspect_freshness(
    root_canonical: &Path,
    relative_path: &Path,
    indexed_modified_at: Option<Timestamp>,
) -> FreshnessStatus {
    if relative_path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return FreshnessStatus::Unknown;
    }

    let candidate = root_canonical.join(relative_path);
    let Ok(metadata) = fs::symlink_metadata(&candidate) else {
        return FreshnessStatus::Unknown;
    };

    if metadata.file_type().is_symlink() {
        let Ok(resolved) = fs::canonicalize(&candidate) else {
            return FreshnessStatus::Unknown;
        };
        if !resolved.starts_with(root_canonical) {
            return FreshnessStatus::Unknown;
        }
    } else if !candidate.starts_with(root_canonical) {
        return FreshnessStatus::Unknown;
    }

    let Ok(modified) = metadata.modified() else {
        return FreshnessStatus::Unknown;
    };
    let Ok(fs_stamp) = Timestamp::try_from_system_time(modified) else {
        return FreshnessStatus::Unknown;
    };
    let Some(indexed) = indexed_modified_at else {
        return FreshnessStatus::Unknown;
    };

    // Allow one-second skew for filesystems with coarse mtime precision.
    if fs_stamp.as_millis() > indexed.as_millis().saturating_add(1_000) {
        FreshnessStatus::PendingReindex
    } else {
        FreshnessStatus::Current
    }
}

/// Builds a display path that remains relative to the root.
#[must_use]
pub fn relative_display(path: &Path) -> PathBuf {
    path.to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn current_when_unchanged() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("a.md");
        fs::write(&file, "hi\n").unwrap();
        let meta = fs::metadata(&file).unwrap();
        let stamp = Timestamp::try_from_system_time(meta.modified().unwrap()).unwrap();
        let status = inspect_freshness(temp.path(), Path::new("a.md"), Some(stamp));
        assert_eq!(status, FreshnessStatus::Current);
    }

    #[test]
    fn pending_when_newer() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("a.md");
        fs::write(&file, "old\n").unwrap();
        let old = Timestamp::try_from_system_time(fs::metadata(&file).unwrap().modified().unwrap())
            .unwrap();
        thread::sleep(Duration::from_millis(1_100));
        let mut handle = fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&file)
            .unwrap();
        handle.write_all(b"new\n").unwrap();
        handle.flush().unwrap();
        let status = inspect_freshness(temp.path(), Path::new("a.md"), Some(old));
        assert_eq!(status, FreshnessStatus::PendingReindex);
    }

    #[test]
    fn unknown_when_missing() {
        let temp = TempDir::new().unwrap();
        let status = inspect_freshness(
            temp.path(),
            Path::new("missing.md"),
            Some(Timestamp::from_millis(1)),
        );
        assert_eq!(status, FreshnessStatus::Unknown);
    }
}
