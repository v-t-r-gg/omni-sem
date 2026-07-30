//! Milestone 3 corrective-pass integration coverage.
//!
//! Covers shared discovery policy for incremental indexing, snapshot lifecycle,
//! federated retrieval with provenance, and status HTTP method contracts.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;

use omnisem_core::config::{add_root, init_installation};
use omnisem_core::discovery::{DiscoveryOptions, is_safe_relative_path, validate_relative_path};
use omnisem_core::domain::{
    EvidenceOrigin, RetrievalLimit, RetrievalMode, RetrievalQuery, TokenBudget,
};
use omnisem_core::git::{GitChangeKind, collect_changes, detect_git_root};
use omnisem_core::index::{IndexOptions, index_roots, index_roots_with_options};
use omnisem_core::paths::AppPaths;
use omnisem_core::retrieval::retrieve;
use omnisem_core::snapshot::{export_snapshot, import_snapshot, list_snapshots, remove_snapshot};
use omnisem_core::status_server::serve_status;
use omnisem_core::storage::{list_active_source_files, open_database, status_snapshot};
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

fn git(path: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(args)
            .current_dir(path)
            .status()
            .unwrap()
            .success(),
        "git {args:?} failed"
    );
}

fn commit_all(path: &Path, message: &str) {
    git(path, &["add", "-A"]);
    git(path, &["commit", "-m", message]);
}

fn request(addr: std::net::SocketAddr, raw: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    let _ = stream.write_all(raw.as_bytes());
    let mut out = Vec::new();
    let _ = stream.read_to_end(&mut out);
    String::from_utf8_lossy(&out).into_owned()
}

#[test]
fn safe_relative_path_rejects_parent_and_absolute() {
    assert!(is_safe_relative_path(Path::new("docs/a.md")));
    assert!(!is_safe_relative_path(Path::new("../escape.md")));
    assert!(!is_safe_relative_path(Path::new("/abs.md")));
    assert!(!is_safe_relative_path(Path::new("")));
}

#[test]
fn incremental_respects_exclude_include_hidden_symlink_and_size() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    fs::write(repo.join("keep.md"), "# Keep\n\nvisible content\n").unwrap();
    fs::write(repo.join("secret.env"), "TOKEN=1\n").unwrap();
    fs::write(repo.join(".hidden.md"), "# Hidden\n").unwrap();
    commit_all(&repo, "init");

    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    let root = add_root(&mut config, &repo, Some("repo".into())).unwrap();
    // Narrow include and keep default excludes (*.env via **/.env.* / default patterns).
    if let Some(entry) = config.roots.iter_mut().find(|item| item.id == root.id) {
        entry.include = vec!["**/*.md".into(), "**/*.txt".into()];
        entry.exclude = vec!["**/skip.md".into()];
    }
    config.save(&paths.config_file).unwrap();
    let mut connection = open_database(&config.database_path().unwrap()).unwrap();
    let full = index_roots(&mut connection, &config, None).unwrap();
    assert_eq!(full.failures, 0);
    assert_eq!(full.additions, 1, "only keep.md should index on full scan");

    // Tracked excluded / hidden / oversized / symlink changes must not index.
    fs::write(repo.join("skip.md"), "# Skip me\n").unwrap();
    fs::write(repo.join("new.env"), "SECRET=1\n").unwrap();
    fs::write(repo.join(".secret.md"), "# still hidden\n").unwrap();
    let oversized = "x".repeat(11 * 1024 * 1024);
    fs::write(repo.join("huge.md"), oversized).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let _ = fs::remove_file(repo.join("link.md"));
        symlink("/etc/passwd", repo.join("link.md")).unwrap();
        symlink(repo.join("keep.md"), repo.join("inside-link.md")).unwrap();
    }
    fs::write(repo.join("ok.txt"), "plain incremental\n").unwrap();
    commit_all(&repo, "changes");

    let incremental = index_roots_with_options(
        &mut connection,
        &config,
        None,
        &IndexOptions { since: Some(None) },
    )
    .unwrap();
    assert_eq!(
        incremental.root_reports[0].mode,
        omnisem_core::index::IndexMode::Incremental
    );
    // only ok.txt should be accepted among the changed set
    let actives = list_active_source_files(&connection, &root.id.parse().unwrap()).unwrap();
    let names: Vec<String> = actives
        .iter()
        .map(|s| s.relative_path.display().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "keep.md"));
    assert!(names.iter().any(|n| n == "ok.txt"));
    assert!(!names.iter().any(|n| n == "skip.md"));
    assert!(!names.iter().any(|n| n.contains(".secret")));
    assert!(!names.iter().any(|n| n == "huge.md"));
    assert!(!names.iter().any(|n| n == "link.md"));
    assert!(!names.iter().any(|n| n == "inside-link.md"));
    assert!(!names.iter().any(|n| n.contains(".env")));
}

#[test]
fn incremental_explicit_delete_and_rename() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    fs::write(repo.join("a.md"), "# A\n\nold path\n").unwrap();
    fs::write(repo.join("stay.md"), "# Stay\n\nstill here\n").unwrap();
    commit_all(&repo, "init");

    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    let root = add_root(&mut config, &repo, Some("repo".into())).unwrap();
    config.save(&paths.config_file).unwrap();
    let mut connection = open_database(&config.database_path().unwrap()).unwrap();
    index_roots(&mut connection, &config, None).unwrap();

    git(&repo, &["mv", "a.md", "b.md"]);
    fs::remove_file(repo.join("stay.md")).unwrap();
    commit_all(&repo, "rename-and-delete");

    let report = index_roots_with_options(
        &mut connection,
        &config,
        None,
        &IndexOptions { since: Some(None) },
    )
    .unwrap();
    assert_eq!(report.deletions, 2);
    let actives = list_active_source_files(&connection, &root.id.parse().unwrap()).unwrap();
    let names: Vec<_> = actives
        .iter()
        .map(|s| s.relative_path.display().to_string())
        .collect();
    assert!(names.iter().any(|n| n == "b.md"));
    assert!(!names.iter().any(|n| n == "a.md"));
    assert!(!names.iter().any(|n| n == "stay.md"));
}

#[test]
fn git_path_with_newline_and_invalid_utf8_policy() {
    // Newline bytes can appear in a UTF-8 path string from Git; path safety still
    // requires non-absolute Normal components. Invalid UTF-8 is the hard abort case.
    let _ = is_safe_relative_path(Path::new("weird\nname.md"));
    // Invalid UTF-8 aborts collect_changes via split_z.
    let bad = b"M\0\xff\xff.md\0";
    let temp = TempDir::new().unwrap();
    init_repo(temp.path());
    fs::write(temp.path().join("a.md"), "x\n").unwrap();
    commit_all(temp.path(), "init");
    let ctx = detect_git_root(temp.path()).expect("git");
    let err = omnisem_core::git::parse_name_status_z_for_test(bad, &ctx);
    assert!(
        err.is_err(),
        "invalid utf-8 must abort incremental collection"
    );
}

#[test]
fn nested_approved_root_prefix_mapping() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    let nested = repo.join("sub");
    fs::create_dir_all(&nested).unwrap();
    init_repo(&repo);
    fs::write(nested.join("n.md"), "# Nested\n\nnested body\n").unwrap();
    fs::write(repo.join("top.md"), "# Top\n\ntop body\n").unwrap();
    commit_all(&repo, "init");

    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    add_root(&mut config, &nested, Some("nested".into())).unwrap();
    config.save(&paths.config_file).unwrap();
    let mut connection = open_database(&config.database_path().unwrap()).unwrap();
    index_roots(&mut connection, &config, None).unwrap();

    fs::write(nested.join("n.md"), "# Nested\n\nchanged\n").unwrap();
    commit_all(&repo, "nested-change");
    let report = index_roots_with_options(
        &mut connection,
        &config,
        None,
        &IndexOptions { since: Some(None) },
    )
    .unwrap();
    assert_eq!(
        report.root_reports[0].mode,
        omnisem_core::index::IndexMode::Incremental
    );
    assert_eq!(report.modifications, 1);
}

#[test]
fn failed_git_base_advances_after_successful_full_fallback() {
    let temp = TempDir::new().unwrap();
    let repo = temp.path().join("repo");
    fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    fs::write(repo.join("a.md"), "# A\n\nbody\n").unwrap();
    commit_all(&repo, "init");

    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    add_root(&mut config, &repo, Some("repo".into())).unwrap();
    config.save(&paths.config_file).unwrap();
    let mut connection = open_database(&config.database_path().unwrap()).unwrap();
    index_roots(&mut connection, &config, None).unwrap();

    // Request a nonsense base → full fallback and advance head.
    let report = index_roots_with_options(
        &mut connection,
        &config,
        None,
        &IndexOptions {
            since: Some(Some("deadbeefdeadbeefdeadbeefdeadbeefdeadbeef".into())),
        },
    )
    .unwrap();
    assert_eq!(
        report.root_reports[0].mode,
        omnisem_core::index::IndexMode::Full
    );
    assert!(report.root_reports[0].fallback_reason.is_some());

    fs::write(repo.join("b.md"), "# B\n\nnew\n").unwrap();
    commit_all(&repo, "second");
    let second = index_roots_with_options(
        &mut connection,
        &config,
        None,
        &IndexOptions { since: Some(None) },
    )
    .unwrap();
    // Should not repeatedly full-scan due to obsolete base.
    assert_eq!(
        second.root_reports[0].mode,
        omnisem_core::index::IndexMode::Incremental
    );
    assert_eq!(second.additions, 1);
}

#[test]
fn validate_relative_path_symlink_policies() {
    let temp = TempDir::new().unwrap();
    let root_path = temp.path().join("root");
    fs::create_dir_all(&root_path).unwrap();
    fs::write(root_path.join("real.md"), "# real\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(root_path.join("real.md"), root_path.join("inside.md")).unwrap();
        symlink("/etc/passwd", root_path.join("escape.md")).unwrap();
    }
    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    let root_cfg = add_root(&mut config, &root_path, Some("r".into())).unwrap();
    let domain = config.domain_roots().unwrap();
    let root = domain
        .iter()
        .find(|r| r.id.to_string() == root_cfg.id)
        .unwrap();
    let options = DiscoveryOptions::default();
    let canon = root_path.canonicalize().unwrap();

    let inside = validate_relative_path(root, &canon, Path::new("inside.md"), &options)
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        inside,
        omnisem_core::discovery::SkipReason::Symlink
    ));

    let mut follow = root.clone();
    follow.follow_symlinks = true;
    let allowed = validate_relative_path(&follow, &canon, Path::new("inside.md"), &options)
        .unwrap()
        .unwrap();
    assert_eq!(allowed.relative_path, PathBuf::from("inside.md"));

    let escaped = validate_relative_path(&follow, &canon, Path::new("escape.md"), &options)
        .unwrap()
        .unwrap_err();
    assert!(matches!(
        escaped,
        omnisem_core::discovery::SkipReason::OutsideRoot
            | omnisem_core::discovery::SkipReason::Symlink
            | omnisem_core::discovery::SkipReason::InvalidMetadata(_)
    ));
}

#[test]
#[allow(clippy::too_many_lines)]
fn snapshot_federation_local_precedence_and_remove() {
    let temp = TempDir::new().unwrap();
    // Installation A
    let a_paths = AppPaths::for_base(temp.path().join("a"));
    let (mut a_config, _) = init_installation(&a_paths).unwrap();
    let a_notes = temp.path().join("a-notes");
    fs::create_dir_all(&a_notes).unwrap();
    fs::write(
        a_notes.join("shared.md"),
        "# Shared\n\nfederation unique token alphazebra\n",
    )
    .unwrap();
    let a_root = add_root(&mut a_config, &a_notes, Some("notes".into())).unwrap();
    a_config.save(&a_paths.config_file).unwrap();
    let mut a_conn = open_database(&a_config.database_path().unwrap()).unwrap();
    index_roots(&mut a_conn, &a_config, None).unwrap();
    let snap_dir = temp.path().join("snap");
    export_snapshot(&a_conn, &snap_dir, None).unwrap();

    // Installation B with different path
    let b_paths = AppPaths::for_base(temp.path().join("b"));
    let (mut b_config, _) = init_installation(&b_paths).unwrap();
    let b_notes = temp.path().join("b-notes");
    fs::create_dir_all(&b_notes).unwrap();
    // Local file with different content first — snapshot-only evidence.
    fs::write(b_notes.join("local-only.md"), "# Local\n\nlocalonlytoken\n").unwrap();
    let b_root = add_root(&mut b_config, &b_notes, Some("notes".into())).unwrap();
    b_config.save(&b_paths.config_file).unwrap();
    let mut b_conn = open_database(&b_config.database_path().unwrap()).unwrap();
    index_roots(&mut b_conn, &b_config, None).unwrap();

    let maps = vec![(a_root.id.clone(), b_root.id.clone())];
    let imported = import_snapshot(&mut b_conn, &snap_dir, &maps).unwrap();
    let listed = list_snapshots(&b_conn).unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].queryable);

    let response = retrieve(
        &b_conn,
        &b_config,
        &RetrievalQuery {
            query: "alphazebra".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Lexical,
            limit: RetrievalLimit::new(10).unwrap(),
            token_budget: TokenBudget::new(4_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        },
    )
    .unwrap();
    assert!(
        response.results.iter().any(|hit| {
            matches!(hit.origin, EvidenceOrigin::Snapshot { .. }) && hit.text.contains("alphazebra")
        }),
        "expected snapshot evidence with origin"
    );
    assert!(
        response.results.iter().all(|hit| hit.freshness
            == omnisem_core::domain::FreshnessStatus::Unknown
            || matches!(hit.origin, EvidenceOrigin::LocalIndex)),
        "snapshot freshness must be unknown"
    );

    // Local exact duplicate wins: copy same content into local index.
    fs::write(
        b_notes.join("shared.md"),
        "# Shared\n\nfederation unique token alphazebra\n",
    )
    .unwrap();
    index_roots(&mut b_conn, &b_config, None).unwrap();
    let response2 = retrieve(
        &b_conn,
        &b_config,
        &RetrievalQuery {
            query: "alphazebra".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Lexical,
            limit: RetrievalLimit::new(10).unwrap(),
            token_budget: TokenBudget::new(4_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        },
    )
    .unwrap();
    let alpha_hits: Vec<_> = response2
        .results
        .iter()
        .filter(|h| h.text.contains("alphazebra"))
        .collect();
    assert!(!alpha_hits.is_empty());
    assert!(
        alpha_hits
            .iter()
            .any(|h| matches!(h.origin, EvidenceOrigin::LocalIndex)),
        "local evidence should win exact text-hash duplicate"
    );

    remove_snapshot(&mut b_conn, &imported.snapshot_id).unwrap();
    let response3 = retrieve(
        &b_conn,
        &b_config,
        &RetrievalQuery {
            query: "alphazebra".into(),
            root_ids: Vec::new(),
            file_types: Vec::new(),
            mode: RetrievalMode::Lexical,
            limit: RetrievalLimit::new(10).unwrap(),
            token_budget: TokenBudget::new(4_000).unwrap(),
            include_sensitive: false,
            budget_preset: None,
        },
    )
    .unwrap();
    assert!(
        response3
            .results
            .iter()
            .all(|h| matches!(h.origin, EvidenceOrigin::LocalIndex)),
        "snapshot evidence must disappear after remove"
    );
    assert!(list_snapshots(&b_conn).unwrap().is_empty());
}

#[test]
fn status_http_methods_and_snapshot_health() {
    let temp = TempDir::new().unwrap();
    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    let notes = temp.path().join("notes");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("a.md"), "# A\n\nstatus body\n").unwrap();
    add_root(&mut config, &notes, Some("notes".into())).unwrap();
    config.save(&paths.config_file).unwrap();
    let db = config.database_path().unwrap();
    let mut connection = open_database(&db).unwrap();
    index_roots(&mut connection, &config, None).unwrap();
    let dest = temp.path().join("snap");
    export_snapshot(&connection, &dest, None).unwrap();
    let root_id = config.roots[0].id.clone();
    import_snapshot(&mut connection, &dest, &[(root_id.clone(), root_id)]).unwrap();
    drop(connection);

    let server = serve_status(&db, 0).unwrap();
    let addr = server.addr();
    assert!(addr.ip().is_loopback());

    let get = request(addr, "GET /status.json HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(get.contains("200 OK"));
    assert!(get.contains("\"registered\""));
    assert!(get.contains("X-Content-Type-Options: nosniff"));
    assert!(get.contains("X-Frame-Options: DENY"));
    assert!(get.contains("Content-Security-Policy"));
    assert!(get.contains("Cache-Control: no-store"));
    assert!(!get.contains("status body"));
    assert!(!get.to_lowercase().contains("select "));

    let head = request(
        addr,
        "HEAD /status.json HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert!(head.contains("200 OK"));
    assert!(!head.contains("\"registered\""));

    let post = request(
        addr,
        "POST /status.json HTTP/1.1\r\nHost: localhost\r\n\r\n",
    );
    assert!(post.contains("405"));
    assert!(post.contains("Allow: GET, HEAD"));

    let bad_method = request(addr, "FOO /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(bad_method.contains("405"));

    let malformed = request(addr, "NOTAVALID\r\n\r\n");
    assert!(malformed.contains("400"));

    let unknown = request(addr, "GET /nope HTTP/1.1\r\nHost: localhost\r\n\r\n");
    assert!(unknown.contains("404"));

    let oversized = format!(
        "GET /{} HTTP/1.1\r\nHost: localhost\r\n\r\n",
        "x".repeat(10_000)
    );
    let big = request(addr, &oversized);
    assert!(big.contains("431") || big.contains("400"));

    server.shutdown();
    let _ = status_snapshot;
    let _ = collect_changes;
    let _ = GitChangeKind::Added;
}
