//! End-to-end operational indexing coverage with synthetic fixtures.

use std::fs;

use omnisem_core::config::{AppConfig, add_root, init_installation};
use omnisem_core::index::index_roots;
use omnisem_core::paths::AppPaths;
use omnisem_core::storage::{
    StorageError, delete_root_derived, list_changes, open_database, status_snapshot,
};
use rusqlite::Connection;
use tempfile::TempDir;

#[test]
fn init_root_index_status_changes_and_remove() {
    let temp = TempDir::new().unwrap();
    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, created) = init_installation(&paths).unwrap();
    assert!(created);
    let notes = temp.path().join("notes");
    fs::create_dir_all(&notes).unwrap();
    fs::write(notes.join("readme.md"), "# Readme\n\nBody.\n").unwrap();
    let root = add_root(&mut config, &notes, Some("notes".into())).unwrap();
    config.save(&paths.config_file).unwrap();

    let db_path = config.database_path().unwrap();
    let mut connection = open_database(&db_path).unwrap();
    let report = index_roots(&mut connection, &config, None).unwrap();
    assert_eq!(report.additions, 1);
    assert_eq!(report.failures, 0);

    let status = status_snapshot(&connection, &db_path).unwrap();
    assert_eq!(status.schema_version, 2);
    assert_eq!(status.active_source_files, 1);
    assert!(status.fts_rows >= 1);
    assert_eq!(status.fts_rows, status.active_segments);

    let changes = list_changes(&connection, None, None).unwrap();
    assert!(changes.iter().any(|event| event.kind == "addition"));

    let root_id = root.id.parse().unwrap();
    let counts = delete_root_derived(&mut connection, &root_id).unwrap();
    assert_eq!(counts.source_files, 1);
    let status = status_snapshot(&connection, &db_path).unwrap();
    assert_eq!(status.active_source_files, 0);
    assert_eq!(status.fts_rows, 0);
}

#[test]
fn failed_parse_preserves_prior_revision_and_fts() {
    let temp = TempDir::new().unwrap();
    let paths = AppPaths::for_base(temp.path().join("app"));
    let (mut config, _) = init_installation(&paths).unwrap();
    let notes = temp.path().join("notes");
    fs::create_dir_all(&notes).unwrap();
    let file = notes.join("doc.md");
    fs::write(&file, "# Ok\n\nGood.\n").unwrap();
    add_root(&mut config, &notes, Some("notes".into())).unwrap();
    config.save(&paths.config_file).unwrap();
    let db_path = config.database_path().unwrap();
    let mut connection = open_database(&db_path).unwrap();
    index_roots(&mut connection, &config, None).unwrap();
    let before = status_snapshot(&connection, &db_path).unwrap();

    fs::write(&file, b"# Bad\n\n\xff\n").unwrap();
    let report = index_roots(&mut connection, &config, None).unwrap();
    assert_eq!(report.failures, 1);
    let after = status_snapshot(&connection, &db_path).unwrap();
    assert_eq!(after.active_source_files, before.active_source_files);
    assert_eq!(after.fts_rows, before.fts_rows);
    assert!(after.fts_rows > 0);
}

#[test]
fn future_schema_rejected_on_open_migrate() {
    let temp = TempDir::new().unwrap();
    let path = temp.path().join("db.sqlite3");
    let mut connection = Connection::open(&path).unwrap();
    omnisem_core::storage::migrate(&mut connection).unwrap();
    connection
        .execute("UPDATE schema_metadata SET version = 99", [])
        .unwrap();
    drop(connection);
    let error = open_database(&path).unwrap_err();
    assert!(matches!(error, StorageError::FutureSchema(99)));
}

#[test]
fn config_unknown_field_and_permissions() {
    let temp = TempDir::new().unwrap();
    let paths = AppPaths::for_base(temp.path());
    paths.ensure_layout().unwrap();
    let bad = paths.config_file.clone();
    fs::write(
        &bad,
        r#"
[general]
database_path = "db.sqlite3"
unknown = 1
"#,
    )
    .unwrap();
    assert!(matches!(
        AppConfig::load(&bad),
        Err(omnisem_core::error::ConfigError::UnknownField(_))
    ));

    let (config, _) = {
        let temp2 = TempDir::new().unwrap();
        let paths2 = AppPaths::for_base(temp2.path());
        init_installation(&paths2).unwrap()
    };
    let _ = config;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let temp3 = TempDir::new().unwrap();
        let paths3 = AppPaths::for_base(temp3.path());
        init_installation(&paths3).unwrap();
        let mode = fs::metadata(&paths3.config_file)
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}
