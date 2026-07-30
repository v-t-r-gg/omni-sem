//! CLI binary end-to-end coverage with isolated data roots.

use std::fs;
use std::process::Command;

use tempfile::TempDir;

#[test]
fn init_root_index_status_changes() {
    let temp = TempDir::new().unwrap();
    let data_root = temp.path().join("app");
    let notes = temp.path().join("corpus");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("a.md"), "# A\n\ntext\n").unwrap();
    fs::write(notes.join("b.txt"), "plain\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_omnisem");
    let run = |args: &[&str]| {
        Command::new(bin)
            .args(["--data-root", data_root.to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    };

    let init = run(&["init", "--json"]);
    assert!(
        init.status.success(),
        "{:?}",
        String::from_utf8_lossy(&init.stderr)
    );
    let add = run(&[
        "root",
        "add",
        notes.to_str().unwrap(),
        "--name",
        "corpus",
        "--json",
    ]);
    assert!(
        add.status.success(),
        "{:?}",
        String::from_utf8_lossy(&add.stderr)
    );
    let index = run(&["index", "--json"]);
    assert!(
        index.status.success(),
        "{:?}",
        String::from_utf8_lossy(&index.stderr)
    );
    let status = run(&["status", "--json"]);
    assert!(status.status.success());
    let body = String::from_utf8_lossy(&status.stdout);
    assert!(body.contains("\"schema_version\": 2"));
    assert!(body.contains("\"active_source_files\": 2"));
    let changes = run(&["changes", "--json"]);
    assert!(changes.status.success());
    let reinit = run(&["init", "--json"]);
    assert!(reinit.status.success());
    let reinit_body = String::from_utf8_lossy(&reinit.stdout);
    assert!(reinit_body.contains("\"created\": false"));
}
