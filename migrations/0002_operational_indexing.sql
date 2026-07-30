ALTER TABLE roots ADD COLUMN sensitivity_tags_json TEXT NOT NULL DEFAULT '[]';
ALTER TABLE roots ADD COLUMN created_at_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE roots ADD COLUMN updated_at_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE roots ADD COLUMN config_fingerprint TEXT NOT NULL DEFAULT '';

ALTER TABLE source_files ADD COLUMN file_type TEXT NOT NULL DEFAULT 'markdown';
ALTER TABLE source_files ADD COLUMN modified_at_ms INTEGER;
ALTER TABLE source_files ADD COLUMN first_seen_at_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE source_files ADD COLUMN last_seen_at_ms INTEGER NOT NULL DEFAULT 0;

ALTER TABLE revisions ADD COLUMN extracted_text_hash TEXT;
ALTER TABLE revisions ADD COLUMN observed_at_ms INTEGER NOT NULL DEFAULT 0;
ALTER TABLE revisions ADD COLUMN indexed_at_ms INTEGER;
ALTER TABLE revisions ADD COLUMN error_message TEXT;

ALTER TABLE segments ADD COLUMN sensitivity_scope TEXT;

CREATE TABLE IF NOT EXISTS scan_runs (
    id TEXT PRIMARY KEY,
    root_id TEXT NOT NULL REFERENCES roots(id) ON DELETE CASCADE,
    started_at_ms INTEGER NOT NULL,
    completed_at_ms INTEGER,
    status TEXT NOT NULL,
    additions INTEGER NOT NULL DEFAULT 0 CHECK (additions >= 0),
    modifications INTEGER NOT NULL DEFAULT 0 CHECK (modifications >= 0),
    unchanged INTEGER NOT NULL DEFAULT 0 CHECK (unchanged >= 0),
    deletions INTEGER NOT NULL DEFAULT 0 CHECK (deletions >= 0),
    skipped INTEGER NOT NULL DEFAULT 0 CHECK (skipped >= 0),
    failures INTEGER NOT NULL DEFAULT 0 CHECK (failures >= 0),
    segments_indexed INTEGER NOT NULL DEFAULT 0 CHECK (segments_indexed >= 0),
    error_code TEXT
);

CREATE INDEX IF NOT EXISTS scan_runs_root_started ON scan_runs(root_id, started_at_ms DESC);
CREATE INDEX IF NOT EXISTS scan_runs_status_completed ON scan_runs(status, completed_at_ms);

CREATE VIRTUAL TABLE IF NOT EXISTS segments_fts USING fts5(
    text,
    segment_id UNINDEXED,
    revision_id UNINDEXED,
    source_file_id UNINDEXED,
    root_id UNINDEXED,
    relative_path UNINDEXED,
    anchor UNINDEXED,
    tokenize = 'porter unicode61'
);
