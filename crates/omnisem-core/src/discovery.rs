//! Safe, ignore-aware discovery of approved roots.
//!
//! Policy summary:
//! - only explicitly approved roots are scanned;
//! - the root path is canonicalized before traversal;
//! - `.gitignore` and `.git/info/exclude` inside the root are honored by default;
//! - parent-directory ignore files outside the root are not applied;
//! - host-global gitignore is not applied, to keep discovery host-portable;
//! - hidden paths are ignored by default (same default family as `ripgrep`/`ignore`);
//! - Omni-Sem exclude patterns are authoritative over anything discovery would keep;
//! - include patterns, when present, narrow the surviving set;
//! - symlinks are not followed by default and never escape the approved root;
//! - devices, sockets, FIFOs, and other special files are skipped;
//! - oversized files are skipped with a structured reason;
//! - discovery uses metadata only and does not read file contents.

use std::fs;
use std::path::{Component, Path, PathBuf};

use globset::{Glob, GlobSet, GlobSetBuilder};
use ignore::WalkBuilder;

use crate::domain::{DiscoveredDocument, Root, SupportedFileType, Timestamp};

/// Default maximum source size accepted during discovery (10 MiB).
pub const DEFAULT_MAX_FILE_SIZE_BYTES: u64 = 10 * 1024 * 1024;

/// Baseline exclude patterns for common secret-bearing and high-churn paths.
///
/// Filename exclusions do not detect secret content. They only avoid indexing
/// paths that are frequently sensitive or non-corpus.
#[must_use]
pub fn default_exclude_patterns() -> Vec<String> {
    vec![
        "**/.git/**".into(),
        "**/node_modules/**".into(),
        "**/.venv/**".into(),
        "**/target/**".into(),
        "**/.env".into(),
        "**/.env.*".into(),
        "**/*.pem".into(),
        "**/*.key".into(),
        "**/id_rsa".into(),
        "**/id_ed25519".into(),
        "**/credentials.*".into(),
        "**/secrets.*".into(),
    ]
}

/// Tunables for one discovery pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryOptions {
    pub max_file_size_bytes: u64,
    pub honor_gitignore: bool,
    pub ignore_hidden: bool,
    pub apply_default_excludes: bool,
}

impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            max_file_size_bytes: DEFAULT_MAX_FILE_SIZE_BYTES,
            honor_gitignore: true,
            ignore_hidden: true,
            apply_default_excludes: true,
        }
    }
}

/// Structured reason a path was not emitted as a discovered document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    DisabledRoot,
    NotAFile,
    Hidden,
    Symlink,
    SpecialFile,
    OutsideRoot,
    IgnoredByGit,
    ExcludedByRule,
    NotIncluded,
    UnsupportedType,
    Oversized { size_bytes: u64 },
    InvalidMetadata(String),
}

/// A path considered during discovery and not returned as a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedPath {
    pub relative_path: PathBuf,
    pub reason: SkipReason,
}

/// Deterministic discovery result for one root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryReport {
    pub documents: Vec<DiscoveredDocument>,
    pub skipped: Vec<SkippedPath>,
}

/// Discovery boundary failures that abort the scan.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum DiscoveryError {
    #[error("approved root does not exist: {0}")]
    RootMissing(PathBuf),
    #[error("approved root is not a directory: {0}")]
    RootNotDirectory(PathBuf),
    #[error("failed to canonicalize approved root {path}: {message}")]
    RootCanonicalize { path: PathBuf, message: String },
    #[error("invalid discovery pattern '{pattern}': {message}")]
    InvalidPattern { pattern: String, message: String },
    #[error("discovery I/O failed for {path}: {message}")]
    Io { path: PathBuf, message: String },
}

/// Discovers supported documents under an approved root.
///
/// # Errors
///
/// Returns [`DiscoveryError`] when the root cannot be used or pattern configuration
/// is invalid. Individual file problems are reported as skips, not hard failures.
pub fn discover_root(
    root: &Root,
    options: &DiscoveryOptions,
) -> Result<DiscoveryReport, DiscoveryError> {
    if !root.enabled {
        return Ok(DiscoveryReport {
            documents: Vec::new(),
            skipped: vec![SkippedPath {
                relative_path: PathBuf::new(),
                reason: SkipReason::DisabledRoot,
            }],
        });
    }

    let root_path = canonicalize_root(&root.canonical_path)?;
    let exclude = compile_patterns(&merged_excludes(root, options))?;
    let include = compile_optional_patterns(&root.include_patterns)?;
    let walker = build_walker(&root_path, options);

    let mut documents = Vec::new();
    let mut skipped = Vec::new();

    for entry in walker {
        match process_entry(
            root,
            &root_path,
            options,
            &exclude,
            include.as_ref(),
            entry,
            &mut documents,
        ) {
            Ok(None) => {}
            Ok(Some(skip)) | Err(skip) => skipped.push(skip),
        }
    }

    documents.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    Ok(DiscoveryReport { documents, skipped })
}

fn build_walker(root_path: &Path, options: &DiscoveryOptions) -> ignore::Walk {
    let mut builder = WalkBuilder::new(root_path);
    builder
        .standard_filters(false)
        .hidden(options.ignore_hidden)
        .parents(false)
        .git_ignore(options.honor_gitignore)
        .git_global(false)
        .git_exclude(options.honor_gitignore)
        .require_git(false)
        .follow_links(false)
        .sort_by_file_path(std::cmp::Ord::cmp);
    builder.build()
}

fn process_entry(
    root: &Root,
    root_path: &Path,
    options: &DiscoveryOptions,
    exclude: &GlobSet,
    include: Option<&GlobSet>,
    entry: Result<ignore::DirEntry, ignore::Error>,
    documents: &mut Vec<DiscoveredDocument>,
) -> Result<Option<SkippedPath>, SkippedPath> {
    let ctx = EntryContext {
        root,
        root_path,
        options,
        exclude,
        include,
    };
    let entry = entry.map_err(|error| SkippedPath {
        relative_path: PathBuf::from(error.to_string()),
        reason: SkipReason::InvalidMetadata(error.to_string()),
    })?;

    let path = entry.path();
    if path == ctx.root_path {
        return Ok(None);
    }

    let relative = relative_within_root(ctx.root_path, path).map_err(|reason| SkippedPath {
        relative_path: path.to_path_buf(),
        reason,
    })?;

    if entry
        .file_type()
        .is_some_and(|file_type| file_type.is_dir())
    {
        return Ok(None);
    }

    if ctx.options.ignore_hidden && is_hidden_relative(&relative) {
        return Err(SkippedPath {
            relative_path: relative,
            reason: SkipReason::Hidden,
        });
    }

    let metadata = entry.metadata().map_err(|error| SkippedPath {
        relative_path: relative.clone(),
        reason: SkipReason::InvalidMetadata(error.to_string()),
    })?;

    let file_type = metadata.file_type();
    if file_type.is_symlink() {
        return handle_symlink(&ctx, path, relative, documents);
    }

    if !file_type.is_file() || is_special_file(file_type) {
        return Err(SkippedPath {
            relative_path: relative,
            reason: if file_type.is_file() {
                SkipReason::SpecialFile
            } else {
                SkipReason::NotAFile
            },
        });
    }

    Ok(classify_and_collect(
        ctx.root,
        ctx.root_path,
        path,
        &relative,
        &metadata,
        ctx.exclude,
        ctx.include,
        ctx.options,
        documents,
    ))
}

struct EntryContext<'a> {
    root: &'a Root,
    root_path: &'a Path,
    options: &'a DiscoveryOptions,
    exclude: &'a GlobSet,
    include: Option<&'a GlobSet>,
}

fn handle_symlink(
    ctx: &EntryContext<'_>,
    path: &Path,
    relative: PathBuf,
    documents: &mut Vec<DiscoveredDocument>,
) -> Result<Option<SkippedPath>, SkippedPath> {
    if !ctx.root.follow_symlinks {
        return Err(SkippedPath {
            relative_path: relative,
            reason: SkipReason::Symlink,
        });
    }

    match resolve_symlink_within_root(ctx.root_path, path) {
        Ok((resolved, resolved_meta)) => Ok(classify_and_collect(
            ctx.root,
            ctx.root_path,
            &resolved,
            &relative,
            &resolved_meta,
            ctx.exclude,
            ctx.include,
            ctx.options,
            documents,
        )),
        Err(reason) => Err(SkippedPath {
            relative_path: relative,
            reason,
        }),
    }
}

#[allow(clippy::too_many_arguments)]
fn classify_and_collect(
    root: &Root,
    root_path: &Path,
    absolute: &Path,
    relative: &Path,
    metadata: &fs::Metadata,
    exclude: &GlobSet,
    include: Option<&GlobSet>,
    options: &DiscoveryOptions,
    documents: &mut Vec<DiscoveredDocument>,
) -> Option<SkippedPath> {
    let relative_str = relative_to_match_str(relative);

    if exclude.is_match(&relative_str) {
        return Some(SkippedPath {
            relative_path: relative.to_path_buf(),
            reason: SkipReason::ExcludedByRule,
        });
    }

    if let Some(include) = include
        && !include.is_match(&relative_str)
    {
        return Some(SkippedPath {
            relative_path: relative.to_path_buf(),
            reason: SkipReason::NotIncluded,
        });
    }

    let size_bytes = metadata.len();
    if size_bytes > options.max_file_size_bytes {
        return Some(SkippedPath {
            relative_path: relative.to_path_buf(),
            reason: SkipReason::Oversized { size_bytes },
        });
    }

    let Some(file_class) = classify_supported_file(relative) else {
        return Some(SkippedPath {
            relative_path: relative.to_path_buf(),
            reason: SkipReason::UnsupportedType,
        });
    };

    let modified_at = match metadata.modified().map_err(|error| error.to_string()) {
        Ok(system_time) => match Timestamp::try_from_system_time(system_time) {
            Ok(stamp) => stamp,
            Err(error) => {
                return Some(SkippedPath {
                    relative_path: relative.to_path_buf(),
                    reason: SkipReason::InvalidMetadata(error.to_string()),
                });
            }
        },
        Err(message) => {
            return Some(SkippedPath {
                relative_path: relative.to_path_buf(),
                reason: SkipReason::InvalidMetadata(message),
            });
        }
    };

    let canonical_path = match fs::canonicalize(absolute) {
        Ok(path) => path,
        Err(error) => {
            return Some(SkippedPath {
                relative_path: relative.to_path_buf(),
                reason: SkipReason::InvalidMetadata(error.to_string()),
            });
        }
    };

    if !path_is_within_root(root_path, &canonical_path) {
        return Some(SkippedPath {
            relative_path: relative.to_path_buf(),
            reason: SkipReason::OutsideRoot,
        });
    }

    documents.push(DiscoveredDocument {
        root_id: root.id,
        canonical_path,
        relative_path: relative.to_path_buf(),
        size_bytes,
        modified_at,
        file_type: file_class,
    });
    None
}

fn merged_excludes(root: &Root, options: &DiscoveryOptions) -> Vec<String> {
    let mut patterns = Vec::new();
    if options.apply_default_excludes {
        patterns.extend(default_exclude_patterns());
    }
    patterns.extend(root.exclude_patterns.iter().cloned());
    patterns
}

fn compile_optional_patterns(patterns: &[String]) -> Result<Option<GlobSet>, DiscoveryError> {
    if patterns.is_empty() {
        Ok(None)
    } else {
        Ok(Some(compile_patterns(patterns)?))
    }
}

fn compile_patterns(patterns: &[String]) -> Result<GlobSet, DiscoveryError> {
    let mut builder = GlobSetBuilder::new();
    for pattern in patterns {
        let glob = Glob::new(pattern).map_err(|error| DiscoveryError::InvalidPattern {
            pattern: pattern.clone(),
            message: error.to_string(),
        })?;
        builder.add(glob);
    }
    builder
        .build()
        .map_err(|error| DiscoveryError::InvalidPattern {
            pattern: "*".into(),
            message: error.to_string(),
        })
}

fn canonicalize_root(path: &Path) -> Result<PathBuf, DiscoveryError> {
    if !path.exists() {
        return Err(DiscoveryError::RootMissing(path.to_path_buf()));
    }
    let metadata = fs::metadata(path).map_err(|error| DiscoveryError::Io {
        path: path.to_path_buf(),
        message: error.to_string(),
    })?;
    if !metadata.is_dir() {
        return Err(DiscoveryError::RootNotDirectory(path.to_path_buf()));
    }
    fs::canonicalize(path).map_err(|error| DiscoveryError::RootCanonicalize {
        path: path.to_path_buf(),
        message: error.to_string(),
    })
}

fn relative_within_root(root: &Path, candidate: &Path) -> Result<PathBuf, SkipReason> {
    let relative = candidate
        .strip_prefix(root)
        .map_err(|_| SkipReason::OutsideRoot)?;
    if relative
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::RootDir))
    {
        return Err(SkipReason::OutsideRoot);
    }
    Ok(relative.to_path_buf())
}

fn path_is_within_root(root: &Path, candidate: &Path) -> bool {
    candidate.starts_with(root)
}

fn relative_to_match_str(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn is_hidden_relative(path: &Path) -> bool {
    path.components()
        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
}

fn is_special_file(file_type: fs::FileType) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        file_type.is_block_device()
            || file_type.is_char_device()
            || file_type.is_fifo()
            || file_type.is_socket()
    }
    #[cfg(not(unix))]
    {
        let _ = file_type;
        false
    }
}

fn resolve_symlink_within_root(
    root: &Path,
    symlink: &Path,
) -> Result<(PathBuf, fs::Metadata), SkipReason> {
    let resolved = fs::canonicalize(symlink)
        .map_err(|error| SkipReason::InvalidMetadata(error.to_string()))?;
    if !path_is_within_root(root, &resolved) {
        return Err(SkipReason::OutsideRoot);
    }
    let metadata =
        fs::metadata(&resolved).map_err(|error| SkipReason::InvalidMetadata(error.to_string()))?;
    if !metadata.is_file() || is_special_file(metadata.file_type()) {
        return Err(SkipReason::SpecialFile);
    }
    Ok((resolved, metadata))
}

/// Classifies a path using extension only. Contents are not inspected.
#[must_use]
pub fn classify_supported_file(path: &Path) -> Option<SupportedFileType> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)?;

    match extension.as_str() {
        "md" | "markdown" | "mdown" | "mkd" | "mdwn" => Some(SupportedFileType::Markdown),
        // Deterministic plain-text fallback. Not language-aware.
        "txt" | "text" | "log" | "rst" | "adoc" | "asciidoc" | "org" | "csv" | "tsv" | "json"
        | "jsonl" | "yaml" | "yml" | "toml" | "ini" | "cfg" | "conf" | "rs" | "py" | "pyi"
        | "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "go" | "java" | "kt" | "kts" | "c"
        | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "cs" | "rb" | "php" | "swift" | "scala"
        | "sh" | "bash" | "zsh" | "fish" | "ps1" | "bat" | "cmd" | "sql" | "html" | "htm"
        | "css" | "scss" | "less" | "xml" | "svg" | "r" | "jl" | "lua" | "pl" | "pm" | "ex"
        | "exs" | "erl" | "hrl" | "hs" | "ml" | "mli" | "nim" | "zig" | "dart" | "groovy"
        | "gradle" | "cmake" | "make" | "proto" | "thrift" | "graphql" | "tf" | "hcl" | "nix"
        | "vim" | "el" | "lisp" | "clj" | "cljs" | "edn" | "sbt" | "lock" | "sum" | "mod"
        | "cabal" | "v" | "sv" | "vhdl" | "asm" | "s" => Some(SupportedFileType::PlainText),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RootId, Timestamp};
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::time::{Duration, SystemTime};
    use tempfile::TempDir;

    fn root_at(path: &Path, exclude: &[&str], include: &[&str], follow: bool) -> Root {
        Root {
            id: RootId::new(),
            canonical_path: path.to_path_buf(),
            display_name: "test".into(),
            include_patterns: include.iter().map(|item| (*item).to_owned()).collect(),
            exclude_patterns: exclude.iter().map(|item| (*item).to_owned()).collect(),
            sensitivity_tags: Vec::new(),
            follow_symlinks: follow,
            enabled: true,
            created_at: Timestamp::from_millis(1),
            updated_at: Timestamp::from_millis(1),
            config_fingerprint: "test".into(),
        }
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, contents).unwrap();
    }

    #[test]
    fn discovers_markdown_and_plain_text_under_approved_root() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("notes/a.md"), "# A\n");
        write_file(&temp.path().join("notes/b.txt"), "plain\n");
        write_file(&temp.path().join("notes/c.rs"), "fn main() {}\n");
        write_file(&temp.path().join("notes/image.png"), "not-text");

        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        let relatives: Vec<_> = report
            .documents
            .iter()
            .map(|document| document.relative_path.to_string_lossy().into_owned())
            .collect();

        assert_eq!(
            relatives,
            vec![
                "notes/a.md".to_owned(),
                "notes/b.txt".to_owned(),
                "notes/c.rs".to_owned()
            ]
        );
        assert_eq!(report.documents[0].file_type, SupportedFileType::Markdown);
        assert_eq!(report.documents[1].file_type, SupportedFileType::PlainText);
        assert_eq!(report.documents[2].file_type, SupportedFileType::PlainText);
        assert!(
            report
                .skipped
                .iter()
                .any(|skip| skip.reason == SkipReason::UnsupportedType)
        );
        assert!(
            report
                .documents
                .iter()
                .all(|document| document.size_bytes > 0)
        );
        assert!(
            report
                .documents
                .iter()
                .all(|document| document.modified_at.as_millis() > 0)
        );
    }

    #[test]
    fn honors_nested_gitignore() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join(".gitignore"), "secret.md\n");
        write_file(&temp.path().join("visible.md"), "ok\n");
        write_file(&temp.path().join("secret.md"), "nope\n");
        write_file(&temp.path().join("nested/.gitignore"), "local.txt\n");
        write_file(&temp.path().join("nested/keep.md"), "keep\n");
        write_file(&temp.path().join("nested/local.txt"), "skip\n");

        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        let relatives: Vec<_> = report
            .documents
            .iter()
            .map(|document| document.relative_path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            relatives,
            vec!["nested/keep.md".to_owned(), "visible.md".to_owned()]
        );
    }

    #[test]
    fn omnisem_exclude_takes_precedence() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("keep.md"), "keep\n");
        write_file(&temp.path().join("drop.md"), "drop\n");
        write_file(&temp.path().join("vendor/lib.md"), "vendor\n");

        let root = root_at(temp.path(), &["drop.md", "vendor/**"], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert_eq!(report.documents[0].relative_path, PathBuf::from("keep.md"));
        assert!(
            report
                .skipped
                .iter()
                .any(|skip| skip.reason == SkipReason::ExcludedByRule)
        );
    }

    #[test]
    fn include_patterns_narrow_discovery() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("a.md"), "a\n");
        write_file(&temp.path().join("b.txt"), "b\n");
        write_file(&temp.path().join("c.rs"), "c\n");

        let root = root_at(temp.path(), &[], &["**/*.md"], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert_eq!(report.documents[0].file_type, SupportedFileType::Markdown);
    }

    #[test]
    fn hidden_files_are_skipped_by_default() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("visible.md"), "v\n");
        write_file(&temp.path().join(".hidden.md"), "h\n");
        write_file(&temp.path().join(".cache/x.md"), "c\n");

        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert_eq!(
            report.documents[0].relative_path,
            PathBuf::from("visible.md")
        );
    }

    #[test]
    fn symlink_escape_is_rejected_when_following() {
        let temp = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        write_file(&outside.path().join("escape.md"), "outside\n");
        write_file(&temp.path().join("inside.md"), "inside\n");
        symlink(
            outside.path().join("escape.md"),
            temp.path().join("link.md"),
        )
        .unwrap();

        let root = root_at(temp.path(), &[], &[], true);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert_eq!(
            report.documents[0].relative_path,
            PathBuf::from("inside.md")
        );
        assert!(report.skipped.iter().any(
            |skip| skip.reason == SkipReason::OutsideRoot || skip.reason == SkipReason::Symlink
        ));
    }

    #[test]
    fn symlink_inside_root_is_skipped_when_not_following() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("target.md"), "target\n");
        symlink(temp.path().join("target.md"), temp.path().join("link.md")).unwrap();

        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert_eq!(
            report.documents[0].relative_path,
            PathBuf::from("target.md")
        );
        assert!(
            report
                .skipped
                .iter()
                .any(|skip| skip.reason == SkipReason::Symlink)
        );
    }

    #[test]
    fn symlink_inside_root_is_accepted_when_following() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("target.md"), "target\n");
        symlink(temp.path().join("target.md"), temp.path().join("link.md")).unwrap();

        let root = root_at(temp.path(), &[], &[], true);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        let relatives: Vec<_> = report
            .documents
            .iter()
            .map(|document| document.relative_path.to_string_lossy().into_owned())
            .collect();
        assert!(relatives.contains(&"target.md".to_owned()));
        assert!(relatives.contains(&"link.md".to_owned()));
    }

    #[test]
    fn oversized_files_are_skipped() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("small.md"), "ok\n");
        write_file(&temp.path().join("large.md"), &"x".repeat(100));

        let root = root_at(temp.path(), &[], &[], false);
        let options = DiscoveryOptions {
            max_file_size_bytes: 10,
            ..DiscoveryOptions::default()
        };
        let report = discover_root(&root, &options).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert_eq!(report.documents[0].relative_path, PathBuf::from("small.md"));
        assert!(report.skipped.iter().any(|skip| matches!(
            skip.reason,
            SkipReason::Oversized { size_bytes } if size_bytes >= 100
        )));
    }

    #[test]
    fn discovery_is_deterministic() {
        let temp = TempDir::new().unwrap();
        for name in ["c.md", "a.md", "b.txt"] {
            write_file(&temp.path().join(name), "body\n");
        }
        let root = root_at(temp.path(), &[], &[], false);
        let first = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        let second = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(first.documents, second.documents);
        assert_eq!(
            first
                .documents
                .iter()
                .map(|document| document.relative_path.clone())
                .collect::<Vec<_>>(),
            vec![
                PathBuf::from("a.md"),
                PathBuf::from("b.txt"),
                PathBuf::from("c.md")
            ]
        );
    }

    #[test]
    fn relative_paths_and_root_identity_are_preserved() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("docs/readme.md"), "hi\n");
        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        let document = &report.documents[0];
        assert_eq!(document.root_id, root.id);
        assert_eq!(document.relative_path, PathBuf::from("docs/readme.md"));
        assert!(document.canonical_path.ends_with("docs/readme.md"));
        assert!(document.canonical_path.is_absolute());
    }

    #[test]
    fn default_excludes_skip_env_files() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("notes.md"), "ok\n");
        write_file(&temp.path().join(".env"), "SECRET=1\n");
        write_file(&temp.path().join("secrets.yaml"), "x\n");

        let root = root_at(temp.path(), &[], &[], false);
        let options = DiscoveryOptions {
            ignore_hidden: false,
            ..DiscoveryOptions::default()
        };
        let report = discover_root(&root, &options).unwrap();
        let relatives: Vec<_> = report
            .documents
            .iter()
            .map(|document| document.relative_path.to_string_lossy().into_owned())
            .collect();
        assert_eq!(relatives, vec!["notes.md".to_owned()]);
    }

    #[test]
    fn special_fifo_is_skipped_on_unix() {
        let temp = TempDir::new().unwrap();
        write_file(&temp.path().join("ok.md"), "ok\n");
        let fifo = temp.path().join("pipe.md");
        let status = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo available");
        assert!(status.success());

        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert_eq!(report.documents.len(), 1);
        assert!(report.skipped.iter().any(|skip| {
            skip.reason == SkipReason::SpecialFile
                || skip.reason == SkipReason::NotAFile
                || matches!(skip.reason, SkipReason::InvalidMetadata(_))
        }));
    }

    #[test]
    fn root_containment_rejects_parent_components() {
        let temp = TempDir::new().unwrap();
        let root = temp.path();
        assert!(relative_within_root(root, &root.join("a.md")).is_ok());
        assert_eq!(
            relative_within_root(root, Path::new("/tmp/other")),
            Err(SkipReason::OutsideRoot)
        );
    }

    #[test]
    fn modified_at_reflects_filesystem_time() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("timed.md");
        write_file(&path, "t\n");
        let before = Timestamp::try_from_system_time(SystemTime::now() - Duration::from_secs(5))
            .unwrap()
            .as_millis();
        let root = root_at(temp.path(), &[], &[], false);
        let report = discover_root(&root, &DiscoveryOptions::default()).unwrap();
        assert!(report.documents[0].modified_at.as_millis() >= before);
    }

    #[test]
    fn classify_distinguishes_markdown_and_plain_text() {
        assert_eq!(
            classify_supported_file(Path::new("a.MD")),
            Some(SupportedFileType::Markdown)
        );
        assert_eq!(
            classify_supported_file(Path::new("main.rs")),
            Some(SupportedFileType::PlainText)
        );
        assert_eq!(classify_supported_file(Path::new("x.png")), None);
    }
}
