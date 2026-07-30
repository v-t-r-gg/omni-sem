//! Bounded metadata-only root suggestions.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Serialize;

use crate::discovery::classify_supported_file;
use crate::error::ConfigError;

/// Hard bounds that keep suggestion from scanning a full home directory.
pub const MAX_CANDIDATE_ROOTS: usize = 16;
pub const MAX_DEPTH: u32 = 3;
pub const MAX_ENTRIES: usize = 4_000;
pub const MAX_DURATION_MS: u128 = 1_500;

/// One suggested workspace directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RootSuggestion {
    pub path: PathBuf,
    pub supported_files: u64,
    pub total_size_bytes: u64,
}

/// Deterministic, bounded root suggestion results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SuggestReport {
    pub suggestions: Vec<RootSuggestion>,
    pub entries_visited: u64,
    pub truncated: bool,
}

/// Inspects the current directory and a small set of common workspace parents.
///
/// Never approves roots or reads file contents.
///
/// # Errors
///
/// Returns filesystem errors when the starting directory cannot be read.
pub fn suggest_roots(start: &Path) -> Result<SuggestReport, ConfigError> {
    let started = Instant::now();
    let mut entries_visited = 0_u64;
    let mut truncated = false;
    let mut suggestions = Vec::new();

    let mut seeds = Vec::new();
    seeds.push(start.to_path_buf());
    if let Some(parent) = start.parent() {
        seeds.push(parent.to_path_buf());
        for name in ["Documents", "notes", "Notes", "projects", "Projects", "src"] {
            let candidate = parent.join(name);
            if candidate.is_dir() {
                seeds.push(candidate);
            }
        }
    }

    let mut seen = Vec::new();
    for seed in seeds {
        if !seed.is_dir() {
            continue;
        }
        if seen.iter().any(|path| path == &seed) {
            continue;
        }
        seen.push(seed.clone());
        match summarize_candidate(&seed, &mut entries_visited, started, &mut truncated) {
            Ok(Some(suggestion)) => suggestions.push(suggestion),
            Ok(None) => {}
            Err(_) => truncated = true,
        }
        if suggestions.len() >= MAX_CANDIDATE_ROOTS || truncated {
            break;
        }
    }

    suggestions.sort_by(|left, right| {
        right
            .supported_files
            .cmp(&left.supported_files)
            .then_with(|| left.path.cmp(&right.path))
    });
    suggestions.truncate(MAX_CANDIDATE_ROOTS);
    Ok(SuggestReport {
        suggestions,
        entries_visited,
        truncated,
    })
}

fn summarize_candidate(
    root: &Path,
    entries_visited: &mut u64,
    started: Instant,
    truncated: &mut bool,
) -> Result<Option<RootSuggestion>, ConfigError> {
    let mut supported_files = 0_u64;
    let mut total_size_bytes = 0_u64;
    let mut state = WalkState {
        entries_visited,
        started,
        truncated,
        supported_files: &mut supported_files,
        total_size_bytes: &mut total_size_bytes,
    };
    walk(root, 0, &mut state)?;
    if supported_files == 0 {
        return Ok(None);
    }
    Ok(Some(RootSuggestion {
        path: fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
        supported_files,
        total_size_bytes,
    }))
}

struct WalkState<'a> {
    entries_visited: &'a mut u64,
    started: Instant,
    truncated: &'a mut bool,
    supported_files: &'a mut u64,
    total_size_bytes: &'a mut u64,
}

fn walk(current: &Path, depth: u32, state: &mut WalkState<'_>) -> Result<(), ConfigError> {
    if *state.truncated
        || depth > MAX_DEPTH
        || usize::try_from(*state.entries_visited).unwrap_or(usize::MAX) >= MAX_ENTRIES
        || state.started.elapsed().as_millis() > MAX_DURATION_MS
    {
        *state.truncated = true;
        return Ok(());
    }
    let read = fs::read_dir(current).map_err(|error| ConfigError::Io {
        path: current.to_path_buf(),
        message: error.to_string(),
    })?;
    let mut entries = read.filter_map(Result::ok).collect::<Vec<_>>();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        *state.entries_visited += 1;
        if usize::try_from(*state.entries_visited).unwrap_or(usize::MAX) >= MAX_ENTRIES
            || state.started.elapsed().as_millis() > MAX_DURATION_MS
        {
            *state.truncated = true;
            break;
        }
        let path = entry.path();
        let file_type = entry.file_type().map_err(|error| ConfigError::Io {
            path: path.clone(),
            message: error.to_string(),
        })?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == ".git"
            {
                continue;
            }
            walk(&path, depth + 1, state)?;
            continue;
        }
        if file_type.is_file() && classify_supported_file(&path).is_some() {
            let metadata = entry.metadata().map_err(|error| ConfigError::Io {
                path: path.clone(),
                message: error.to_string(),
            })?;
            *state.supported_files += 1;
            *state.total_size_bytes = state.total_size_bytes.saturating_add(metadata.len());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn suggests_directory_with_supported_files() {
        let temp = TempDir::new().unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("a.md"), "hi\n").unwrap();
        let report = suggest_roots(temp.path()).unwrap();
        assert!(
            report
                .suggestions
                .iter()
                .any(|item| item.supported_files >= 1)
        );
    }
}
