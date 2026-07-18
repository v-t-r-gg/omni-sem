CREATE TABLE IF NOT EXISTS schema_metadata (
    singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
    version INTEGER NOT NULL CHECK (version >= 0)
);
CREATE TABLE roots (
    id TEXT PRIMARY KEY, canonical_path TEXT NOT NULL UNIQUE, display_name TEXT NOT NULL,
    include_patterns_json TEXT NOT NULL, exclude_patterns_json TEXT NOT NULL,
    follow_symlinks INTEGER NOT NULL DEFAULT 0 CHECK (follow_symlinks IN (0,1)),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0,1))
);
CREATE TABLE source_files (
    id TEXT PRIMARY KEY, root_id TEXT NOT NULL REFERENCES roots(id), relative_path TEXT NOT NULL,
    canonical_path_hash TEXT NOT NULL, size_bytes INTEGER NOT NULL CHECK (size_bytes >= 0),
    current_revision_id TEXT, state TEXT NOT NULL,
    UNIQUE(root_id, relative_path)
);
CREATE TABLE revisions (
    id TEXT PRIMARY KEY, source_file_id TEXT NOT NULL REFERENCES source_files(id),
    content_hash TEXT NOT NULL, parser_id TEXT NOT NULL, parser_version TEXT NOT NULL,
    status TEXT NOT NULL, error_code TEXT, UNIQUE(source_file_id, content_hash, parser_id, parser_version)
);
CREATE TABLE segments (
    id TEXT PRIMARY KEY, revision_id TEXT NOT NULL REFERENCES revisions(id), segment_type TEXT NOT NULL,
    anchor TEXT NOT NULL, ordinal INTEGER NOT NULL CHECK (ordinal >= 0), text TEXT NOT NULL,
    text_hash TEXT NOT NULL, token_count INTEGER, metadata_json TEXT NOT NULL DEFAULT '{}',
    UNIQUE(revision_id, ordinal), UNIQUE(revision_id, anchor)
);
CREATE INDEX source_files_root_state ON source_files(root_id, state);
CREATE INDEX revisions_source ON revisions(source_file_id);
CREATE INDEX segments_revision ON segments(revision_id);

