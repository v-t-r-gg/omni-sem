//! Optional Git integration for incremental indexing.
//!
//! Invokes the host `git` executable with fixed argv (no shell). Full indexing
//! remains available when Git is missing or a root is not a repository.

use std::path::{Component, Path, PathBuf};
use std::process::Command;

use crate::error::IndexError;
use crate::hash::blake3_hex;

/// Kind of path change reported by Git.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitChangeKind {
    Added,
    Modified,
    Deleted,
}

/// One root-relative path change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitPathChange {
    pub relative_path: PathBuf,
    pub kind: GitChangeKind,
}

/// Resolved Git context for an approved root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRootContext {
    pub git_dir: PathBuf,
    pub work_tree: PathBuf,
    pub root_prefix: PathBuf,
    pub head: String,
    pub repo_fingerprint: String,
}

/// Discovers an enclosing Git work tree for `root_path`.
///
/// # Errors
///
/// Returns [`None`] when Git is unavailable or the path is outside a repository.
pub fn detect_git_root(root_path: &Path) -> Option<GitRootContext> {
    let work_tree = run_git(root_path, &["rev-parse", "--show-toplevel"], true)?
        .lines()
        .next()?
        .trim()
        .to_owned();
    let work_tree = PathBuf::from(work_tree);
    let git_dir = run_git(root_path, &["rev-parse", "--git-dir"], true)?
        .lines()
        .next()?
        .trim()
        .to_owned();
    let git_dir = {
        let candidate = PathBuf::from(&git_dir);
        if candidate.is_absolute() {
            candidate
        } else {
            root_path.join(candidate)
        }
    };
    let head = run_git(root_path, &["rev-parse", "HEAD"], true)?
        .lines()
        .next()?
        .trim()
        .to_owned();
    if head.is_empty() {
        return None;
    }
    let root_canon = root_path.canonicalize().ok()?;
    let work_canon = work_tree.canonicalize().ok()?;
    let root_prefix = root_canon
        .strip_prefix(&work_canon)
        .ok()
        .map_or_else(PathBuf::new, Path::to_path_buf);
    let repo_fingerprint = blake3_hex(work_canon.to_string_lossy().as_bytes()).0;
    Some(GitRootContext {
        git_dir,
        work_tree: work_canon,
        root_prefix,
        head,
        repo_fingerprint,
    })
}

/// Lists root-relative changed paths since `base` including untracked files.
///
/// # Errors
///
/// Returns [`IndexError`] when Git commands fail after repository detection.
pub fn collect_changes(
    root_path: &Path,
    ctx: &GitRootContext,
    base: &str,
) -> Result<Vec<GitPathChange>, IndexError> {
    let mut changes = Vec::new();

    // Committed + unstaged + staged vs base: git diff --name-status -z base
    let diff = run_git_bytes(
        root_path,
        &[
            "-c",
            "core.quotepath=false",
            "diff",
            "--name-status",
            "--find-renames",
            "--no-ext-diff",
            "--no-textconv",
            "-z",
            base,
        ],
    )?;
    parse_name_status_z(&diff, ctx, &mut changes);

    // Also include unstaged/staged against HEAD when base == HEAD is wrong for WIP:
    // `git diff -z --name-status HEAD` captures working tree + index vs HEAD.
    // When base is not HEAD, the base diff already includes history; still capture
    // dirty tree relative to HEAD for tracked files.
    if base != ctx.head {
        let dirty = run_git_bytes(
            root_path,
            &[
                "-c",
                "core.quotepath=false",
                "diff",
                "--name-status",
                "--find-renames",
                "--no-ext-diff",
                "--no-textconv",
                "-z",
                "HEAD",
            ],
        )?;
        parse_name_status_z(&dirty, ctx, &mut changes);
    }

    // Untracked eligible files
    let untracked = run_git_bytes(
        root_path,
        &[
            "-c",
            "core.quotepath=false",
            "ls-files",
            "--others",
            "--exclude-standard",
            "-z",
        ],
    )?;
    for path in split_z(&untracked) {
        if let Some(rel) = map_repo_path_to_root(ctx, &path) {
            changes.push(GitPathChange {
                relative_path: rel,
                kind: GitChangeKind::Added,
            });
        }
    }

    dedupe_changes(&mut changes);
    Ok(changes)
}

fn parse_name_status_z(bytes: &[u8], ctx: &GitRootContext, out: &mut Vec<GitPathChange>) {
    let parts = split_z(bytes);
    let mut index = 0;
    while index < parts.len() {
        let status = &parts[index];
        if status.is_empty() {
            index += 1;
            continue;
        }
        let code = status.chars().next().unwrap_or('M');
        match code {
            'R' | 'C' => {
                // rename/copy: status, old, new
                if index + 2 >= parts.len() {
                    break;
                }
                let old = &parts[index + 1];
                let new = &parts[index + 2];
                if let Some(rel) = map_repo_path_to_root(ctx, old) {
                    out.push(GitPathChange {
                        relative_path: rel,
                        kind: GitChangeKind::Deleted,
                    });
                }
                if let Some(rel) = map_repo_path_to_root(ctx, new) {
                    out.push(GitPathChange {
                        relative_path: rel,
                        kind: GitChangeKind::Added,
                    });
                }
                index += 3;
            }
            'D' => {
                if index + 1 >= parts.len() {
                    break;
                }
                let path = &parts[index + 1];
                if let Some(rel) = map_repo_path_to_root(ctx, path) {
                    out.push(GitPathChange {
                        relative_path: rel,
                        kind: GitChangeKind::Deleted,
                    });
                }
                index += 2;
            }
            'A' => {
                if index + 1 >= parts.len() {
                    break;
                }
                let path = &parts[index + 1];
                if let Some(rel) = map_repo_path_to_root(ctx, path) {
                    out.push(GitPathChange {
                        relative_path: rel,
                        kind: GitChangeKind::Added,
                    });
                }
                index += 2;
            }
            _ => {
                if index + 1 >= parts.len() {
                    break;
                }
                let path = &parts[index + 1];
                if let Some(rel) = map_repo_path_to_root(ctx, path) {
                    out.push(GitPathChange {
                        relative_path: rel,
                        kind: GitChangeKind::Modified,
                    });
                }
                index += 2;
            }
        }
    }
}

fn map_repo_path_to_root(ctx: &GitRootContext, repo_relative: &str) -> Option<PathBuf> {
    if repo_relative.is_empty() || repo_relative.starts_with('/') {
        return None;
    }
    let path = PathBuf::from(repo_relative);
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return None;
    }
    let prefix = &ctx.root_prefix;
    if prefix.as_os_str().is_empty() {
        return Some(path);
    }
    path.strip_prefix(prefix).ok().map(Path::to_path_buf)
}

fn split_z(bytes: &[u8]) -> Vec<String> {
    bytes
        .split(|byte| *byte == 0)
        .filter(|chunk| !chunk.is_empty())
        .filter_map(|chunk| std::str::from_utf8(chunk).ok().map(str::to_owned))
        .collect()
}

fn dedupe_changes(changes: &mut Vec<GitPathChange>) {
    changes.sort_by(|left, right| {
        left.relative_path
            .cmp(&right.relative_path)
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
    });
    changes.dedup_by(|left, right| {
        left.relative_path == right.relative_path && left.kind == right.kind
    });
}

fn run_git(cwd: &Path, args: &[&str], quiet: bool) -> Option<String> {
    let mut command = Command::new("git");
    command.current_dir(cwd).args(args);
    let output = command.output().ok()?;
    if !output.status.success() {
        if !quiet {
            return None;
        }
        return None;
    }
    String::from_utf8(output.stdout).ok()
}

fn run_git_bytes(cwd: &Path, args: &[&str]) -> Result<Vec<u8>, IndexError> {
    let output = Command::new("git")
        .current_dir(cwd)
        .args(args)
        .output()
        .map_err(|error| IndexError::Internal(format!("failed to invoke git: {error}")))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(IndexError::Internal(format!("git failed: {stderr}")));
    }
    Ok(output.stdout)
}

/// Resolves whether a revision exists.
#[must_use]
pub fn revision_exists(cwd: &Path, rev: &str) -> bool {
    run_git(
        cwd,
        &["rev-parse", "--verify", &format!("{rev}^{{commit}}")],
        true,
    )
    .is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn init_repo(path: &Path) {
        assert!(
            Command::new("git")
                .args(["init"])
                .current_dir(path)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.email", "test@example.com"])
                .current_dir(path)
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["config", "user.name", "Test"])
                .current_dir(path)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn detects_changes_and_deletes() {
        let temp = TempDir::new().unwrap();
        init_repo(temp.path());
        fs::write(temp.path().join("a.md"), "# A\n").unwrap();
        assert!(
            Command::new("git")
                .args(["add", "a.md"])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "init"])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        let ctx = detect_git_root(temp.path()).expect("git root");
        let base = ctx.head.clone();
        fs::write(temp.path().join("b.md"), "# B\n").unwrap();
        fs::remove_file(temp.path().join("a.md")).unwrap();
        assert!(
            Command::new("git")
                .args(["add", "-A"])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["commit", "-m", "change"])
                .current_dir(temp.path())
                .status()
                .unwrap()
                .success()
        );
        let changes =
            collect_changes(temp.path(), &detect_git_root(temp.path()).unwrap(), &base).unwrap();
        assert!(
            changes
                .iter()
                .any(|item| item.relative_path.as_path() == Path::new("a.md")
                    && item.kind == GitChangeKind::Deleted)
        );
        assert!(
            changes
                .iter()
                .any(|item| item.relative_path.as_path() == Path::new("b.md")
                    && matches!(item.kind, GitChangeKind::Added | GitChangeKind::Modified))
        );
    }
}
