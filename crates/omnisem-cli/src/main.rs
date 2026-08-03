//! Omni-Sem operational CLI.

use std::path::{Path, PathBuf};
use std::process::ExitCode as StdExitCode;

use clap::{Parser, Subcommand};
use omnisem_core::config::{AppConfig, add_root, init_installation, remove_root};
use omnisem_core::domain::{
    RetrievalMode, RetrievalQuery, RootId, SupportedFileType, Timestamp, parse_duration_to_millis,
};
use omnisem_core::embedding::configured_provider;
use omnisem_core::error::{ConfigError, ExitCode, IndexError, RetrievalError};
use omnisem_core::eval::{
    compare_evaluation_with_provider, run_evaluation, run_evaluation_with_provider,
};
use omnisem_core::index::{IndexOptions, IndexReport, index_roots_with_options};
use omnisem_core::paths::AppPaths;
use omnisem_core::retrieval::{resolve_budget_args, retrieve};
use omnisem_core::snapshot::{
    export_snapshot, import_snapshot, inspect_snapshot, list_snapshots, parse_root_maps,
    remove_snapshot,
};
use omnisem_core::status_server::serve_status;
use omnisem_core::storage::record_query_activity;
use omnisem_core::storage::{
    RootRemovalCounts, count_active_sources, delete_root_derived, list_changes, open_database,
    status_snapshot,
};
use omnisem_core::suggest::suggest_roots;
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "omnisem",
    version,
    about = "Private, source-grounded local context for AI agents"
)]
struct Cli {
    /// Override the application base directory (tests and advanced installs).
    #[arg(long, global = true, value_name = "PATH")]
    data_root: Option<PathBuf>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create configuration and database layout without indexing.
    Init {
        #[arg(long)]
        json: bool,
    },
    /// Manage approved roots.
    Root {
        #[command(subcommand)]
        action: RootCommand,
    },
    /// Index approved roots into immutable revisions and active FTS.
    Index {
        #[arg(long)]
        root: Option<String>,
        /// Git-aware incremental indexing. Optional revision value uses last recorded base when omitted.
        #[arg(long, num_args = 0..=1, default_missing_value = "__LAST__")]
        since: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Show operational index status or serve a local read-only HTTP view.
    Status {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        serve: bool,
        #[arg(long, default_value_t = 0)]
        port: u16,
    },
    /// Run configuration, database, parser, snapshot, and optional provider diagnostics.
    Doctor {
        #[arg(long)]
        json: bool,
    },
    /// Export or import portable derived-data snapshots.
    Snapshot {
        #[command(subcommand)]
        action: SnapshotCommand,
    },
    /// Show addition, modification, and deletion history.
    Changes {
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Lexical retrieval over the active index.
    Query {
        /// Natural-language query text.
        query: String,
        #[arg(long, default_value = "auto")]
        mode: String,
        #[arg(long)]
        root: Option<String>,
        #[arg(long = "file-type")]
        file_type: Option<String>,
        #[arg(long)]
        limit: Option<u16>,
        #[arg(long = "token-budget")]
        token_budget: Option<u32>,
        #[arg(long)]
        budget: Option<String>,
        #[arg(long = "include-sensitive")]
        include_sensitive: bool,
        #[arg(long)]
        explain: bool,
        #[arg(long)]
        json: bool,
    },
    /// Run isolated retrieval evaluation against a fixture bundle.
    Eval {
        #[arg(long)]
        corpus: Option<PathBuf>,
        #[arg(long, default_value = "lexical")]
        mode: String,
        /// Compare lexical, semantic, and hybrid over the evaluation bundle.
        #[arg(long)]
        compare: bool,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum SnapshotCommand {
    /// Export a sensitive derived-data snapshot directory.
    Export {
        path: PathBuf,
        #[arg(long)]
        root: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Import a snapshot after explicit root mapping.
    Import {
        path: PathBuf,
        /// Mapping `SNAPSHOT_ROOT_ID=LOCAL_ROOT_ID` (repeatable).
        #[arg(long = "map")]
        map: Vec<String>,
        #[arg(long)]
        json: bool,
    },
    /// List registered snapshots.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Inspect one registered snapshot.
    Inspect {
        snapshot_id: String,
        #[arg(long)]
        json: bool,
    },
    /// Remove a registered snapshot and its managed payload.
    Remove {
        snapshot_id: String,
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Subcommand)]
enum RootCommand {
    /// Approve a directory root without reading file contents.
    Add {
        path: PathBuf,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// List approved roots.
    List {
        #[arg(long)]
        json: bool,
    },
    /// Suggest candidate roots from nearby directories (metadata only).
    Suggest {
        #[arg(long)]
        json: bool,
    },
    /// Revoke a root and delete its derived index data.
    Remove {
        root_id: String,
        #[arg(long)]
        json: bool,
    },
}

fn main() -> StdExitCode {
    let code = run().unwrap_or_else(|code| code);
    StdExitCode::from(u8::try_from(code.code()).unwrap_or(70))
}

fn run() -> Result<ExitCode, ExitCode> {
    let cli = Cli::parse();
    let paths = resolve_paths(cli.data_root.as_ref()).map_err(|error| print_config_err(&error))?;
    match cli.command {
        None => {
            println!("Run `omnisem --help` for available commands.");
            Ok(ExitCode::Success)
        }
        Some(Command::Init { json }) => {
            cmd_init(&paths, json).map_err(|error| print_config_err(&error))
        }
        Some(Command::Root { action }) => cmd_root(&paths, action),
        Some(Command::Index { root, since, json }) => {
            cmd_index(&paths, root.as_deref(), since, json)
        }
        Some(Command::Status { json, serve, port }) => cmd_status(&paths, json, serve, port),
        Some(Command::Doctor { json }) => cmd_doctor(&paths, json),
        Some(Command::Snapshot { action }) => cmd_snapshot(&paths, action),
        Some(Command::Changes { since, root, json }) => {
            cmd_changes(&paths, since.as_deref(), root.as_deref(), json)
        }
        Some(Command::Query {
            query,
            mode,
            root,
            file_type,
            limit,
            token_budget,
            budget,
            include_sensitive,
            explain,
            json,
        }) => cmd_query(
            &paths,
            &query,
            &mode,
            root.as_deref(),
            file_type.as_deref(),
            limit,
            token_budget,
            budget.as_deref(),
            include_sensitive,
            explain,
            json,
        ),
        Some(Command::Eval {
            corpus,
            mode,
            compare,
            json,
        }) => cmd_eval(&paths, corpus.as_deref(), &mode, compare, json),
    }
}

fn resolve_paths(data_root: Option<&PathBuf>) -> Result<AppPaths, ConfigError> {
    match data_root {
        Some(path) => Ok(AppPaths::for_base(path)),
        None => AppPaths::discover(),
    }
}

fn print_config_err(error: &ConfigError) -> ExitCode {
    eprintln!("error: {error}");
    error.exit_code()
}

fn print_index_err(error: &IndexError) -> ExitCode {
    eprintln!("error: {error}");
    error.exit_code()
}

fn print_retrieval_err(error: &RetrievalError) -> ExitCode {
    eprintln!("error: {error}");
    error.exit_code()
}

fn cmd_init(paths: &AppPaths, json: bool) -> Result<ExitCode, ConfigError> {
    let (config, created) = init_installation(paths)?;
    let database_path = config.database_path()?;
    open_database(&database_path).map_err(|error| ConfigError::Io {
        path: database_path.clone(),
        message: error.to_string(),
    })?;
    if json {
        print_json(&InitOutput {
            created,
            config_path: paths.config_file.display().to_string(),
            database_path: database_path.display().to_string(),
            roots: config.roots.len(),
        })?;
    } else {
        println!(
            "Omni-Sem {} at {}",
            if created {
                "initialized"
            } else {
                "already initialized"
            },
            paths.config_file.display()
        );
        println!("database: {}", database_path.display());
        println!("roots: {}", config.roots.len());
    }
    Ok(ExitCode::Success)
}

fn cmd_root(paths: &AppPaths, action: RootCommand) -> Result<ExitCode, ExitCode> {
    match action {
        RootCommand::Add { path, name, json } => {
            cmd_root_add(paths, &path, name, json).map_err(|error| print_config_err(&error))
        }
        RootCommand::List { json } => {
            cmd_root_list(paths, json).map_err(|error| print_config_err(&error))
        }
        RootCommand::Suggest { json } => {
            cmd_root_suggest(json).map_err(|error| print_config_err(&error))
        }
        RootCommand::Remove { root_id, json } => {
            cmd_root_remove(paths, &root_id, json).map_err(|error| print_config_err(&error))
        }
    }
}

fn cmd_root_add(
    paths: &AppPaths,
    path: &Path,
    name: Option<String>,
    json: bool,
) -> Result<ExitCode, ConfigError> {
    let mut config = load_or_init(paths)?;
    let entry = add_root(&mut config, path, name)?;
    config.save(&paths.config_file)?;
    let database_path = config.database_path()?;
    let connection = open_database(&database_path).map_err(|error| ConfigError::Io {
        path: database_path.clone(),
        message: error.to_string(),
    })?;
    let root = entry.to_domain()?;
    omnisem_core::storage::upsert_root(&connection, &root).map_err(|error| ConfigError::Io {
        path: database_path,
        message: error.to_string(),
    })?;
    if json {
        print_json(&entry)?;
    } else {
        println!("approved root {}", entry.id);
        println!("name: {}", entry.name);
        println!("path: {}", entry.path);
        println!("follow_symlinks: {}", entry.follow_symlinks);
    }
    Ok(ExitCode::Success)
}

fn cmd_root_list(paths: &AppPaths, json: bool) -> Result<ExitCode, ConfigError> {
    let config = load_or_init(paths)?;
    let database_path = config.database_path()?;
    let connection = open_database(&database_path).map_err(|error| ConfigError::Io {
        path: database_path.clone(),
        message: error.to_string(),
    })?;
    let mut rows = Vec::new();
    for root in &config.roots {
        let id = root
            .id
            .parse::<RootId>()
            .map_err(|_| ConfigError::Invalid {
                path: paths.config_file.clone(),
                message: format!("invalid root id {}", root.id),
            })?;
        let indexed = count_active_sources(&connection, &id).unwrap_or(0);
        rows.push(RootListItem {
            id: root.id.clone(),
            name: root.name.clone(),
            path: root.path.clone(),
            enabled: root.enabled,
            include: root.include.clone(),
            exclude: root.exclude.clone(),
            follow_symlinks: root.follow_symlinks,
            sensitivity_tag_count: root.sensitivity.len(),
            indexed_file_count: indexed,
        });
    }
    if json {
        print_json(&rows)?;
    } else if rows.is_empty() {
        println!("No approved roots.");
    } else {
        for row in rows {
            println!(
                "{}  {}  {}  enabled={}  indexed={}  sensitivity={}",
                row.id,
                row.name,
                row.path,
                row.enabled,
                row.indexed_file_count,
                row.sensitivity_tag_count
            );
        }
    }
    Ok(ExitCode::Success)
}

fn cmd_root_suggest(json: bool) -> Result<ExitCode, ConfigError> {
    let cwd = std::env::current_dir().map_err(|error| ConfigError::Io {
        path: PathBuf::from("."),
        message: error.to_string(),
    })?;
    let report = suggest_roots(&cwd)?;
    if json {
        print_json(&report)?;
    } else if report.suggestions.is_empty() {
        println!("No candidate roots found near {}.", cwd.display());
    } else {
        for item in &report.suggestions {
            println!(
                "{}  files={}  bytes={}",
                item.path.display(),
                item.supported_files,
                item.total_size_bytes
            );
        }
        if report.truncated {
            println!("suggestion scan truncated by safety bounds");
        }
    }
    Ok(ExitCode::Success)
}

fn cmd_root_remove(paths: &AppPaths, root_id: &str, json: bool) -> Result<ExitCode, ConfigError> {
    let mut config = load_or_init(paths)?;
    let removed = remove_root(&mut config, root_id)?;
    config.save(&paths.config_file)?;
    let database_path = config.database_path()?;
    let mut connection = open_database(&database_path).map_err(|error| ConfigError::Io {
        path: database_path.clone(),
        message: error.to_string(),
    })?;
    let id = root_id
        .parse::<RootId>()
        .map_err(|_| ConfigError::RootNotFound(root_id.into()))?;
    let counts = delete_root_derived(&mut connection, &id).map_err(|error| ConfigError::Io {
        path: database_path,
        message: error.to_string(),
    })?;
    if json {
        print_json(&RootRemoveOutput {
            root_id: removed.id,
            name: removed.name,
            path: removed.path,
            removed: counts,
        })?;
    } else {
        println!("removed root {}", removed.id);
        println!(
            "derived removed: sources={} revisions={} segments={}",
            counts.source_files, counts.revisions, counts.segments
        );
    }
    Ok(ExitCode::Success)
}

fn cmd_index(
    paths: &AppPaths,
    root: Option<&str>,
    since: Option<String>,
    json: bool,
) -> Result<ExitCode, ExitCode> {
    let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
    let database_path = config
        .database_path()
        .map_err(|error| print_config_err(&error))?;
    let mut connection = open_database(&database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    let filter = root
        .map(str::parse::<RootId>)
        .transpose()
        .map_err(|error| {
            eprintln!("error: {error}");
            ExitCode::InvalidInput
        })?;
    let options = IndexOptions {
        since: since.map(|value| {
            if value == "__LAST__" {
                None
            } else {
                Some(value)
            }
        }),
    };
    let report = index_roots_with_options(&mut connection, &config, filter.as_ref(), &options)
        .map_err(|error| print_index_err(&error))?;
    if json {
        print_json(&report).map_err(|error| print_config_err(&error))?;
    } else {
        print_index_human(&report);
    }
    if report.failures > 0
        || report
            .embedding
            .as_ref()
            .is_some_and(|embedding| matches!(embedding.status.as_str(), "partial" | "failed"))
    {
        Ok(ExitCode::PartialIndexing)
    } else {
        Ok(ExitCode::Success)
    }
}

fn print_index_human(report: &IndexReport) {
    for root in &report.root_reports {
        println!(
            "root {} mode={:?} base={:?} head={:?} changed={} fallback={:?}",
            root.root_name,
            root.mode,
            root.resolved_base,
            root.current_head,
            root.changed_paths,
            root.fallback_reason
        );
    }
    println!("roots scanned: {}", report.roots_scanned);
    println!("files discovered: {}", report.files_discovered);
    println!("additions: {}", report.additions);
    println!("modifications: {}", report.modifications);
    println!("unchanged: {}", report.unchanged);
    println!("deletions: {}", report.deletions);
    println!("skipped: {}", report.skipped);
    println!("failed: {}", report.failures);
    println!("segments indexed: {}", report.segments_indexed);
    println!("duration_ms: {}", report.duration_ms);
    if let Some(embedding) = &report.embedding {
        println!("embedding provider: {}", embedding.provider);
        println!(
            "embedding space: {}",
            embedding.embedding_space.as_deref().unwrap_or("none")
        );
        println!("embedding status: {}", embedding.status);
        println!("embedding active segments: {}", embedding.active_segments);
        println!("embedding cache hits: {}", embedding.cache_hits);
        println!("embedding provider inputs: {}", embedding.provider_inputs);
        println!("embedding linked segments: {}", embedding.linked_segments);
        println!("embedding missing segments: {}", embedding.missing_segments);
        println!("embedding failed segments: {}", embedding.failed_segments);
    }
}

#[allow(clippy::too_many_lines)]
fn cmd_status(paths: &AppPaths, json: bool, serve: bool, port: u16) -> Result<ExitCode, ExitCode> {
    let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
    let database_path = config
        .database_path()
        .map_err(|error| print_config_err(&error))?;
    let connection = open_database(&database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    if serve {
        drop(connection);
        let server = serve_status(&database_path, &config.embeddings, port)
            .map_err(|error| print_config_err(&error))?;
        println!("Omni-Sem status listening on http://{}/", server.addr());
        println!("Read-only local view. Press Ctrl+C to stop.");
        loop {
            std::thread::park();
        }
    }
    let snapshot = status_snapshot(&connection, &database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    let compatibility =
        omnisem_core::storage::embedding_compatibility(&config.embeddings, &snapshot.embedding);
    let output = StatusOutput {
        config_path: paths.config_file.display().to_string(),
        database_path: database_path.display().to_string(),
        schema_version: snapshot.schema_version,
        root_count: snapshot.root_count,
        enabled_roots: snapshot.enabled_root_count,
        active_source_files: snapshot.active_source_files,
        active_revisions: snapshot.active_revisions,
        active_segments: snapshot.active_segments,
        fts_rows: snapshot.fts_rows,
        failed_sources: snapshot.failed_sources,
        last_successful_scan_ms: snapshot.last_successful_scan_ms,
        last_failed_scan_ms: snapshot.last_failed_scan_ms,
        database_size_bytes: snapshot.database_size_bytes,
        sensitivity_tag_count: snapshot.sensitivity_tag_count,
        embedding: snapshot.embedding,
        embedding_compatibility: compatibility,
    };
    if json {
        print_json(&output).map_err(|error| print_config_err(&error))?;
    } else {
        println!("config: {}", output.config_path);
        println!("database: {}", output.database_path);
        println!("schema_version: {}", output.schema_version);
        println!(
            "roots: {} (enabled {})",
            output.root_count, output.enabled_roots
        );
        println!("active_source_files: {}", output.active_source_files);
        println!("active_revisions: {}", output.active_revisions);
        println!("active_segments: {}", output.active_segments);
        println!("fts_rows: {}", output.fts_rows);
        println!("failed_sources: {}", output.failed_sources);
        println!(
            "embedding_space: {}",
            output
                .embedding
                .active_space_id
                .as_deref()
                .unwrap_or("none")
        );
        println!(
            "embedding_provider: {}",
            output.embedding.provider.as_deref().unwrap_or("none")
        );
        println!(
            "embedding_model: {}",
            output
                .embedding
                .canonical_model
                .as_deref()
                .unwrap_or("none")
        );
        println!(
            "embedding_digest: {}",
            output
                .embedding
                .model_digest
                .as_deref()
                .map_or("none", |value| &value[..value.len().min(12)])
        );
        println!(
            "embedding_coverage: {}/{}",
            output.embedding.linked_active_segments, output.embedding.active_segments
        );
        println!(
            "embedding_compatibility: {}",
            output.embedding_compatibility.state
        );
        println!(
            "last_successful_scan_ms: {}",
            output
                .last_successful_scan_ms
                .map_or_else(|| "-".into(), |value| value.to_string())
        );
        println!(
            "last_failed_scan_ms: {}",
            output
                .last_failed_scan_ms
                .map_or_else(|| "-".into(), |value| value.to_string())
        );
        println!("database_size_bytes: {}", output.database_size_bytes);
        println!("sensitivity_tag_count: {}", output.sensitivity_tag_count);
    }
    Ok(ExitCode::Success)
}

#[allow(clippy::too_many_lines)]
fn cmd_doctor(paths: &AppPaths, json: bool) -> Result<ExitCode, ExitCode> {
    let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
    let database_path = config
        .database_path()
        .map_err(|error| print_config_err(&error))?;
    let connection = open_database(&database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    let mut checks = vec![
        DoctorCheck::pass("configuration", "configuration is valid"),
        DoctorCheck::pass("database", "database is available"),
    ];
    let snapshot = status_snapshot(&connection, &database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    checks.push(DoctorCheck::pass(
        "schema",
        &format!("schema version {}", snapshot.schema_version),
    ));
    let fts = connection
        .query_row("SELECT COUNT(*) FROM segments_fts", [], |row| {
            row.get::<_, i64>(0)
        })
        .is_ok();
    checks.push(if fts {
        DoctorCheck::pass("fts5", "FTS5 is available")
    } else {
        DoctorCheck::failure("fts5", "FTS5 is unavailable")
    });
    let inaccessible = config
        .roots
        .iter()
        .filter(|root| root.enabled && !Path::new(&root.path).is_dir())
        .count();
    checks.push(if inaccessible == 0 {
        DoctorCheck::pass("roots", "enabled roots are accessible")
    } else {
        DoctorCheck::failure(
            "roots",
            &format!("{inaccessible} enabled roots are inaccessible"),
        )
    });
    checks.push(
        match omnisem_core::parsing::ParserRegistry::with_defaults() {
            Ok(_) => DoctorCheck::pass("parsers", "parsers are available"),
            Err(_) => DoctorCheck::failure("parsers", "parser registry is unavailable"),
        },
    );
    if config.embeddings.enabled {
        match omnisem_core::embedding::diagnose_provider(&config.embeddings) {
            Ok(Some(model)) => {
                checks.push(DoctorCheck::pass(
                    "embedding_provider",
                    &format!(
                        "model {} digest {}",
                        model.canonical_name,
                        &model.model_digest[..model.model_digest.len().min(12)]
                    ),
                ));
                let active = &snapshot.embedding;
                let dimensions_match = model
                    .dimensions
                    .is_none_or(|value| active.dimensions == Some(value));
                let compatible = active.provider.as_deref() == Some("ollama")
                    && active.canonical_model.as_deref() == Some(model.canonical_name.as_str())
                    && active.model_digest.as_deref() == Some(model.model_digest.as_str())
                    && dimensions_match
                    && active.dimensions.is_some_and(|value| value > 0);
                checks.push(if compatible {
                    DoctorCheck::pass("embedding_space_compatibility", "resolved provider, canonical model, digest, and dimensions agree with the active space")
                } else {
                    DoctorCheck::failure("embedding_space_compatibility", "resolved provider, canonical model, digest, or dimensions do not agree with the active persisted space")
                });
            }
            Ok(None) => checks.push(DoctorCheck::disabled(
                "embedding_provider",
                "embeddings are disabled",
            )),
            Err(error) => checks.push(DoctorCheck::failure("embedding_provider", error.code())),
        }
    } else {
        checks.push(DoctorCheck::disabled(
            "embedding_provider",
            "embeddings are disabled; no provider request made",
        ));
        checks.push(DoctorCheck::disabled(
            "embedding_space_compatibility",
            "embeddings are disabled",
        ));
    }
    checks.push(DoctorCheck {
        name: "embedding_coverage".into(),
        status: if snapshot.embedding.missing_active_segments == 0 {
            "pass"
        } else {
            "warning"
        }
        .into(),
        message: format!(
            "{}/{} active segments linked; {} failures",
            snapshot.embedding.linked_active_segments,
            snapshot.embedding.active_segments,
            snapshot.embedding.current_failure_count
        ),
    });
    let snapshot_health = omnisem_core::snapshot::list_snapshots(&connection).is_ok();
    checks.push(if snapshot_health {
        DoctorCheck::pass("snapshots", "snapshot registry is healthy")
    } else {
        DoctorCheck::failure("snapshots", "snapshot registry is unhealthy")
    });
    let failed = checks.iter().any(|check| check.status == "failure");
    let output = DoctorOutput {
        overall: if failed { "failure" } else { "pass" }.into(),
        checks,
    };
    if json {
        print_json(&output).map_err(|error| print_config_err(&error))?;
    } else {
        for check in &output.checks {
            println!("{}: {} - {}", check.name, check.status, check.message);
        }
        println!("overall: {}", output.overall);
    }
    Ok(if failed {
        ExitCode::PartialIndexing
    } else {
        ExitCode::Success
    })
}

fn cmd_changes(
    paths: &AppPaths,
    since: Option<&str>,
    root: Option<&str>,
    json: bool,
) -> Result<ExitCode, ExitCode> {
    let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
    let database_path = config
        .database_path()
        .map_err(|error| print_config_err(&error))?;
    let connection = open_database(&database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    let since_ms = match since {
        None => None,
        Some(token) => {
            let duration = parse_duration_to_millis(token).map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::InvalidInput
            })?;
            let now = Timestamp::now().map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::Internal
            })?;
            Some(now.as_millis().saturating_sub(duration))
        }
    };
    let root_id = root
        .map(str::parse::<RootId>)
        .transpose()
        .map_err(|error| {
            eprintln!("error: {error}");
            ExitCode::InvalidInput
        })?;
    let events = list_changes(&connection, root_id.as_ref(), since_ms).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    if json {
        print_json(&events).map_err(|error| print_config_err(&error))?;
    } else if events.is_empty() {
        println!("No changes.");
    } else {
        for event in events {
            println!(
                "{}  {}  {}  prev={}  curr={}",
                event.kind,
                event.root_id,
                event.relative_path,
                event.previous_content_hash.as_deref().unwrap_or("-"),
                event.current_content_hash.as_deref().unwrap_or("-")
            );
        }
    }
    Ok(ExitCode::Success)
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
fn cmd_query(
    paths: &AppPaths,
    query: &str,
    mode: &str,
    root: Option<&str>,
    file_type: Option<&str>,
    limit: Option<u16>,
    token_budget: Option<u32>,
    budget: Option<&str>,
    include_sensitive: bool,
    explain: bool,
    json: bool,
) -> Result<ExitCode, ExitCode> {
    let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
    let database_path = config
        .database_path()
        .map_err(|error| print_config_err(&error))?;
    let connection = open_database(&database_path).map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::Database
    })?;
    let mode = mode.parse::<RetrievalMode>().map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::InvalidInput
    })?;
    let root_ids = match root {
        None => Vec::new(),
        Some(value) => {
            let id = value.parse::<RootId>().map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::InvalidInput
            })?;
            vec![id]
        }
    };
    let file_types = match file_type {
        None => Vec::new(),
        Some(value) => {
            let parsed = value.parse::<SupportedFileType>().map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::InvalidInput
            })?;
            vec![parsed]
        }
    };
    let (limit, token_budget, preset) = resolve_budget_args(&config, budget, limit, token_budget)
        .map_err(|error| print_retrieval_err(&error))?;
    let request = RetrievalQuery {
        query: query.to_owned(),
        root_ids,
        file_types,
        mode,
        limit,
        token_budget,
        include_sensitive,
        budget_preset: preset,
    };
    let response =
        retrieve(&connection, &config, &request).map_err(|error| print_retrieval_err(&error))?;
    let _ = record_query_activity(
        &connection,
        omnisem_core::domain::Timestamp::now()
            .map_or(0, omnisem_core::domain::Timestamp::as_millis),
        response.mode.as_str(),
        i64::try_from(response.results.len()).unwrap_or(0),
        i64::try_from(response.elapsed_ms).unwrap_or(0),
    );
    if json {
        print_json(&QueryEnvelope {
            schema_version: 2,
            response,
        })
        .map_err(|error| print_config_err(&error))?;
    } else if response.results.is_empty() {
        println!("No matches.");
        println!(
            "mode={} limit={} budget={} elapsed_ms={}",
            response.mode.as_str(),
            response.applied_limit,
            response.applied_token_budget,
            response.elapsed_ms
        );
    } else {
        for (index, hit) in response.results.iter().enumerate() {
            let origin = match &hit.origin {
                omnisem_core::domain::EvidenceOrigin::LocalIndex => "local".to_owned(),
                omnisem_core::domain::EvidenceOrigin::Snapshot { snapshot_id, .. } => {
                    format!("snapshot:{snapshot_id}")
                }
            };
            println!(
                "{}. {}#{}  score={:.4}  freshness={}  tokens={}  origin={}",
                index + 1,
                hit.relative_path.display(),
                hit.anchor,
                hit.score,
                hit.freshness.as_str(),
                hit.token_estimate,
                origin
            );
            println!("   score_kind={}", response.score_kind);
            if let Some(raw) = hit.signals.raw_bm25 {
                println!("   raw_bm25={raw:.6}");
            }
            if let Some(cosine) = hit.signals.cosine_similarity {
                println!("   cosine_similarity={cosine:.6}");
            }
            if let Some(fusion) = hit.signals.fusion_score {
                println!("   fusion_score={fusion:.6}");
            }
            println!("   revision={}", hit.revision_id);
            if let Some(scope) = hit.sensitivity_scope {
                println!("   sensitivity={}", scope.as_str());
            }
            println!("   {}", hit.text.replace('\n', " "));
            if explain {
                println!(
                    "   matched_terms={}",
                    hit.explanation.matched_terms.join(", ")
                );
                if let Some(excerpt) = &hit.explanation.matched_excerpt {
                    println!("   excerpt={}", excerpt.replace('\n', " "));
                }
            }
        }
        println!(
            "requested_mode={} mode={} results={} tokens={} duplicates_suppressed={} truncated={} elapsed_ms={}",
            response.requested_mode.as_str(),
            response.mode.as_str(),
            response.results.len(),
            response.token_estimate,
            response.duplicates_suppressed,
            response.truncated,
            response.elapsed_ms
        );
        println!(
            "query_embedding_ms={:.3} vector_scan_ms={:.3} vectors_examined={} corrupt_excluded={} local_lexical={} snapshot_lexical={} semantic={} fusion_admitted={} fusion_unique={} fusion_duplicates={}",
            response.telemetry.query_embedding_ms,
            response.telemetry.vector_scan_ms,
            response.telemetry.active_vectors_examined,
            response.telemetry.corrupt_vectors_excluded,
            response.telemetry.local_lexical_candidates,
            response.telemetry.snapshot_lexical_candidates,
            response.telemetry.semantic_candidates,
            response.telemetry.candidates_admitted_to_fusion,
            response.telemetry.unique_fused_candidates,
            response.telemetry.fusion_duplicates_suppressed,
        );
    }
    Ok(ExitCode::Success)
}

fn cmd_eval(
    paths: &AppPaths,
    corpus: Option<&Path>,
    mode: &str,
    compare: bool,
    json: bool,
) -> Result<ExitCode, ExitCode> {
    let mode = mode.parse::<RetrievalMode>().map_err(|error| {
        eprintln!("error: {error}");
        ExitCode::InvalidInput
    })?;
    let bundle = if let Some(path) = corpus {
        path.to_path_buf()
    } else {
        let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../evals");
        if dev.exists() {
            dev
        } else {
            PathBuf::from("evals")
        }
    };
    if compare || mode != RetrievalMode::Lexical {
        let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
        let provider = configured_provider(&config.embeddings).map_err(|error| {
            eprintln!("error: {error}");
            ExitCode::Protocol
        })?;
        if compare {
            let report =
                compare_evaluation_with_provider(&bundle, &config.embeddings, provider.as_ref())
                    .map_err(|error| print_retrieval_err(&error))?;
            print_json(&report).map_err(|error| print_config_err(&error))?;
            return Ok(ExitCode::Success);
        }
        let report =
            run_evaluation_with_provider(&bundle, mode, &config.embeddings, provider.as_ref())
                .map_err(|error| print_retrieval_err(&error))?;
        if json {
            print_json(&report).map_err(|error| print_config_err(&error))?;
        } else {
            print_eval_report(&report);
        }
        return Ok(ExitCode::Success);
    }
    let report = run_evaluation(&bundle, mode).map_err(|error| print_retrieval_err(&error))?;
    if json {
        print_json(&report).map_err(|error| print_config_err(&error))?;
    } else {
        print_eval_report(&report);
    }
    Ok(ExitCode::Success)
}

fn print_eval_report(report: &omnisem_core::eval::EvalReport) {
    println!("run_id={}", report.run_id);
    println!(
        "mode={} corpus={} queries={}",
        report.mode, report.corpus_size, report.query_count
    );
    println!(
        "recall@5={:.3} recall@10={:.3} mrr={:.3} ndcg={:.3}",
        report.metrics.recall_at_5,
        report.metrics.recall_at_10,
        report.metrics.mrr,
        report.metrics.ndcg
    );
    println!(
        "dup={:.3} stale={:.3} misleading={:.3} diversity={:.3} tokens={:.1}",
        report.metrics.duplicate_result_rate,
        report.metrics.stale_result_rate,
        report.metrics.misleading_result_rate,
        report.metrics.source_diversity,
        report.metrics.returned_tokens
    );
    println!(
        "p50_ms={:.2} p95_ms={:.2}",
        report.metrics.p50_latency_ms, report.metrics.p95_latency_ms
    );
    println!(
        "query_embedding_p50_ms={:.2} query_embedding_p95_ms={:.2} vector_scan_p50_ms={:.2} vector_scan_p95_ms={:.2}",
        report.metrics.p50_query_embedding_ms,
        report.metrics.p95_query_embedding_ms,
        report.metrics.p50_vector_scan_ms,
        report.metrics.p95_vector_scan_ms,
    );
}

#[allow(clippy::too_many_lines)]
fn cmd_snapshot(paths: &AppPaths, action: SnapshotCommand) -> Result<ExitCode, ExitCode> {
    let config = load_or_init(paths).map_err(|error| print_config_err(&error))?;
    let database_path = config
        .database_path()
        .map_err(|error| print_config_err(&error))?;
    match action {
        SnapshotCommand::Export { path, root, json } => {
            let mut connection = open_database(&database_path).map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::Database
            })?;
            let filter = root
                .as_deref()
                .map(str::parse::<RootId>)
                .transpose()
                .map_err(|error| {
                    eprintln!("error: {error}");
                    ExitCode::InvalidInput
                })?;
            let report = export_snapshot(&connection, &path, filter.as_ref())
                .map_err(|error| print_config_err(&error))?;
            eprintln!(
                "warning: snapshot contains derived indexed text and may include substantially all approved corpus content"
            );
            if json {
                print_json(&report).map_err(|error| print_config_err(&error))?;
            } else {
                println!("exported snapshot to {}", report.path);
                println!(
                    "segments={} checksum={}",
                    report.segments, report.payload_checksum
                );
            }
            let _ = &mut connection;
            Ok(ExitCode::Success)
        }
        SnapshotCommand::Import { path, map, json } => {
            let mut connection = open_database(&database_path).map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::Database
            })?;
            let maps = parse_root_maps(&map).map_err(|error| print_config_err(&error))?;
            let report = import_snapshot(&mut connection, &path, &maps)
                .map_err(|error| print_config_err(&error))?;
            if json {
                print_json(&report).map_err(|error| print_config_err(&error))?;
            } else {
                println!("imported snapshot {}", report.snapshot_id);
                println!("maps: {}", report.mapped_roots.join(", "));
            }
            Ok(ExitCode::Success)
        }
        SnapshotCommand::List { json } => {
            let connection = open_database(&database_path).map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::Database
            })?;
            let items = list_snapshots(&connection).map_err(|error| print_config_err(&error))?;
            if json {
                print_json(&items).map_err(|error| print_config_err(&error))?;
            } else if items.is_empty() {
                println!("No snapshots registered.");
            } else {
                for item in items {
                    println!(
                        "{}  {}  queryable={} healthy={} segments={} maps={}/{}",
                        item.snapshot_id,
                        item.logical_name,
                        item.queryable,
                        item.payload_healthy,
                        item.segment_count,
                        item.mapped_roots,
                        item.total_roots
                    );
                }
            }
            Ok(ExitCode::Success)
        }
        SnapshotCommand::Inspect { snapshot_id, json } => {
            let connection = open_database(&database_path).map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::Database
            })?;
            let item = inspect_snapshot(&connection, &snapshot_id)
                .map_err(|error| print_config_err(&error))?;
            if json {
                print_json(&item).map_err(|error| print_config_err(&error))?;
            } else {
                println!("snapshot {}", item.snapshot_id);
                println!(
                    "queryable={} healthy={}",
                    item.queryable, item.payload_healthy
                );
                println!("segments={}", item.counts.segments);
                println!("mappings: {}", item.mappings.join(", "));
                println!("{}", item.warning);
            }
            Ok(ExitCode::Success)
        }
        SnapshotCommand::Remove { snapshot_id, json } => {
            let mut connection = open_database(&database_path).map_err(|error| {
                eprintln!("error: {error}");
                ExitCode::Database
            })?;
            let report = remove_snapshot(&mut connection, &snapshot_id)
                .map_err(|error| print_config_err(&error))?;
            if json {
                print_json(&report).map_err(|error| print_config_err(&error))?;
            } else {
                println!("removed snapshot {}", report.snapshot_id);
                println!("segments_were={}", report.segments);
            }
            Ok(ExitCode::Success)
        }
    }
}

fn load_or_init(paths: &AppPaths) -> Result<AppConfig, ConfigError> {
    if paths.config_file.exists() {
        AppConfig::load(&paths.config_file)
    } else {
        let (config, _) = init_installation(paths)?;
        Ok(config)
    }
}

fn print_json<T: Serialize>(value: &T) -> Result<(), ConfigError> {
    println!(
        "{}",
        serde_json::to_string_pretty(value).map_err(|error| ConfigError::Invalid {
            path: PathBuf::from("stdout"),
            message: error.to_string(),
        })?
    );
    Ok(())
}

#[derive(Debug, Serialize)]
struct InitOutput {
    created: bool,
    config_path: String,
    database_path: String,
    roots: usize,
}

#[derive(Debug, Serialize)]
struct RootListItem {
    id: String,
    name: String,
    path: String,
    enabled: bool,
    include: Vec<String>,
    exclude: Vec<String>,
    follow_symlinks: bool,
    sensitivity_tag_count: usize,
    indexed_file_count: i64,
}

#[derive(Debug, Serialize)]
struct RootRemoveOutput {
    root_id: String,
    name: String,
    path: String,
    removed: RootRemovalCounts,
}

#[derive(Debug, Serialize)]
struct QueryEnvelope {
    schema_version: u32,
    response: omnisem_core::domain::RetrievalResponse,
}

#[derive(Debug, Serialize)]
struct StatusOutput {
    config_path: String,
    database_path: String,
    schema_version: i64,
    root_count: i64,
    enabled_roots: i64,
    active_source_files: i64,
    active_revisions: i64,
    active_segments: i64,
    fts_rows: i64,
    failed_sources: i64,
    last_successful_scan_ms: Option<i64>,
    last_failed_scan_ms: Option<i64>,
    database_size_bytes: u64,
    sensitivity_tag_count: i64,
    embedding: omnisem_core::storage::EmbeddingStatus,
    embedding_compatibility: omnisem_core::storage::EmbeddingCompatibility,
}

#[derive(Debug, Serialize)]
struct DoctorOutput {
    overall: String,
    checks: Vec<DoctorCheck>,
}

#[derive(Debug, Serialize)]
struct DoctorCheck {
    name: String,
    status: String,
    message: String,
}
impl DoctorCheck {
    fn pass(name: &str, message: &str) -> Self {
        Self {
            name: name.into(),
            status: "pass".into(),
            message: message.into(),
        }
    }
    fn failure(name: &str, message: &str) -> Self {
        Self {
            name: name.into(),
            status: "failure".into(),
            message: message.into(),
        }
    }
    fn disabled(name: &str, message: &str) -> Self {
        Self {
            name: name.into(),
            status: "disabled".into(),
            message: message.into(),
        }
    }
}
