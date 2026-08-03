CREATE TABLE embedding_spaces (
    id TEXT PRIMARY KEY,
    provider TEXT NOT NULL CHECK(provider IN ('ollama')),
    canonical_model TEXT NOT NULL,
    model_digest TEXT NOT NULL,
    dimensions INTEGER NOT NULL CHECK(dimensions > 0 AND dimensions <= 65536),
    normalization TEXT NOT NULL CHECK(normalization = 'l2'),
    input_contract_version TEXT NOT NULL,
    created_at_ms INTEGER NOT NULL,
    config_fingerprint TEXT NOT NULL,
    provider_metadata_json TEXT NOT NULL DEFAULT '{}',
    UNIQUE(provider, canonical_model, model_digest, dimensions, normalization, input_contract_version)
);

CREATE TABLE embedding_state (
    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
    active_embedding_space_id TEXT REFERENCES embedding_spaces(id)
);

CREATE TABLE embedding_vectors (
    embedding_space_id TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
    text_hash TEXT NOT NULL,
    vector_bytes BLOB NOT NULL,
    dimensions INTEGER NOT NULL CHECK(dimensions > 0 AND length(vector_bytes) = dimensions * 4),
    created_at_ms INTEGER NOT NULL,
    PRIMARY KEY(embedding_space_id, text_hash)
);

CREATE TABLE segment_embeddings (
    segment_id TEXT NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
    revision_id TEXT NOT NULL REFERENCES revisions(id) ON DELETE CASCADE,
    embedding_space_id TEXT NOT NULL,
    text_hash TEXT NOT NULL,
    linked_at_ms INTEGER NOT NULL,
    PRIMARY KEY(segment_id, embedding_space_id),
    FOREIGN KEY(embedding_space_id, text_hash)
        REFERENCES embedding_vectors(embedding_space_id, text_hash) ON DELETE CASCADE
);
CREATE INDEX segment_embeddings_space ON segment_embeddings(embedding_space_id);

CREATE TABLE embedding_failures (
    segment_id TEXT NOT NULL REFERENCES segments(id) ON DELETE CASCADE,
    embedding_space_id TEXT NOT NULL REFERENCES embedding_spaces(id) ON DELETE CASCADE,
    attempted_at_ms INTEGER NOT NULL,
    error_code TEXT NOT NULL,
    safe_message TEXT NOT NULL CHECK(length(safe_message) <= 512),
    retry_count INTEGER NOT NULL DEFAULT 1 CHECK(retry_count > 0 AND retry_count <= 1000000),
    PRIMARY KEY(segment_id, embedding_space_id)
);

CREATE TABLE embedding_sync_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    embedding_space_id TEXT REFERENCES embedding_spaces(id),
    started_at_ms INTEGER NOT NULL,
    completed_at_ms INTEGER,
    attempted_segments INTEGER NOT NULL DEFAULT 0 CHECK(attempted_segments >= 0),
    cache_hits INTEGER NOT NULL DEFAULT 0 CHECK(cache_hits >= 0),
    provider_inputs INTEGER NOT NULL DEFAULT 0 CHECK(provider_inputs >= 0),
    linked_segments INTEGER NOT NULL DEFAULT 0 CHECK(linked_segments >= 0),
    failures INTEGER NOT NULL DEFAULT 0 CHECK(failures >= 0),
    status TEXT NOT NULL CHECK(status IN ('completed','partial','failed')),
    failure_category TEXT
);
CREATE INDEX embedding_sync_runs_completed ON embedding_sync_runs(completed_at_ms DESC);
