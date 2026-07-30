CREATE TABLE IF NOT EXISTS root_git_state (
    root_id TEXT PRIMARY KEY REFERENCES roots(id) ON DELETE CASCADE,
    repo_fingerprint TEXT,
    last_indexed_commit TEXT,
    observed_head TEXT,
    last_incremental_base TEXT,
    last_incremental_at_ms INTEGER
);

CREATE TABLE IF NOT EXISTS snapshots (
    id TEXT PRIMARY KEY,
    logical_name TEXT NOT NULL,
    format_version INTEGER NOT NULL,
    imported_at_ms INTEGER NOT NULL,
    payload_path TEXT NOT NULL UNIQUE,
    manifest_json TEXT NOT NULL,
    checksum TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS snapshot_root_maps (
    snapshot_id TEXT NOT NULL REFERENCES snapshots(id) ON DELETE CASCADE,
    snapshot_root_id TEXT NOT NULL,
    local_root_id TEXT NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
    PRIMARY KEY (snapshot_id, snapshot_root_id)
);

CREATE TABLE IF NOT EXISTS query_activity (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    observed_at_ms INTEGER NOT NULL,
    mode TEXT NOT NULL,
    result_count INTEGER NOT NULL,
    elapsed_ms INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS query_activity_observed ON query_activity(observed_at_ms DESC);
