# Omni-Sem Development Blueprint

**Project:** Omni-Sem
**Expanded name:** Omni-Semantic
**Document version:** 0.3
**Project stage:** Pre-implementation architecture and foundation

**Version 0.3 changelog:** Incorporates a set of usefulness-focused feature additions identified during a product-viability review of v0.2. Each addition is scoped to reuse data or mechanisms the architecture already captures (revisions, timestamps, retrieval signals, root/exclude rules) rather than introducing new subsystems, and each is placed at the earliest milestone where it can ship without violating the measured-complexity and evidence-before-inference principles in Section 5. See ADR-0007 through ADR-0010 for the rationale behind each addition.
**Primary implementation language:** Rust
**Optional research tooling:** Python
**Runtime distribution target:** Native executable with no required Python runtime
**Public product name:** Omni-Sem
**Primary CLI:** `omnisem`
**Future persistent daemon:** `omnisemd`
**Future MCP bridge:** `omnisem-mcp`

---

# 1. Agent Role and Operating Instructions

You are the founding development agent for Omni-Sem.

Your task is to establish the project’s technical foundation without prematurely implementing speculative graph intelligence, a graphical interface, autonomous agent behavior, cloud synchronization, or unnecessary infrastructure.

Work incrementally. Prefer small, testable, reversible changes.

Before introducing a significant dependency or architectural decision:

1. Explain the concrete problem it solves.
2. Compare it against the simplest viable alternative.
3. Record the decision in an Architecture Decision Record.
4. Add tests that establish the expected behavior.
5. Keep domain logic independent of the selected vendor or library.
6. Avoid introducing a new crate boundary without a demonstrated ownership or dependency reason.

Do not claim a feature or milestone is complete unless:

* its tests pass;
* its behavior is documented;
* its failure modes are handled;
* its acceptance criteria are demonstrated;
* all required checks pass;
* the repository is left in a clean and reproducible state.

Do not silently alter the product scope described in this blueprint.

When uncertain, favor correctness, provenance, and simple deterministic behavior over speculative intelligence.

Do not include any unnecessary comments. If you deem a comment absolutely necessary, consider the code should be rewritten to be self-explainable.

---

# 2. Executive Summary

Omni-Sem is a local-first, read-only semantic indexing service for AI agents.

It converts explicitly approved local files into a structured, provenance-rich search index and exposes precise context through the Model Context Protocol.

Omni-Sem is not:

* an AI assistant;
* a chat application;
* a file manager;
* an autonomous agent;
* a source-code editor;
* a cloud knowledge platform;
* a general-purpose graph database.

Its purpose is to provide trustworthy local context to any compatible AI system.

The initial implementation will prove the following pipeline:

```text
Approved Markdown files
    → immutable source revisions
    → structure-aware segments
    → SQLite full-text indexing
    → token-bounded context retrieval
    → read-only MCP interface
```

Semantic embeddings, persistent watching, PDF extraction, source-code parsing, entity resolution, and graph traversal are later extensions.

None of those extensions may block the deterministic retrieval foundation.

---

# 3. Product Thesis

Local files contain valuable context, but AI agents frequently access that context inefficiently.

Typical agent workflows require the model to:

* list directories;
* guess which files are relevant;
* repeatedly open unchanged files;
* consume large portions of the context window;
* miss evidence spread across multiple documents;
* lose track of source revisions;
* rediscover the same information during every session.

Existing solutions often require one or more of the following:

* uploading documents into a specific application;
* using a bundled chat interface;
* manually organizing data into collections;
* relying on cloud infrastructure;
* maintaining an external database service;
* accepting opaque retrieval without strong source evidence.

Omni-Sem addresses this by maintaining a reusable local index independently of any particular AI client.

It should give agents direct access to relevant evidence without granting them unrestricted access to the filesystem.

---

# 4. Product Definition

## 4.1 One-sentence definition

Omni-Sem is a local, read-only context substrate that transforms approved files into source-grounded, efficiently retrievable evidence for AI agents.

## 4.2 Core value proposition

Omni-Sem gives AI agents fast access to relevant local context without requiring the agent to repeatedly explore the filesystem or requiring the user to manually tag every document.

## 4.3 Primary users

### Solo developers

Developers with projects, technical documentation, notes, and source code spread across local workspaces.

### Researchers and writers

Users managing papers, citations, research notes, drafts, and source material.

### Privacy-conscious power users

Users who want AI-assisted retrieval without transferring their private corpus to a hosted knowledge service.

### Local-model users

Users operating local inference infrastructure through Ollama or similar model runners.

## 4.4 Initial product wedge

The first product should target mixed personal workspaces centered on Markdown documentation and notes.

Later versions may add:

* PDFs;
* Python source code;
* TypeScript source code;
* source citations;
* deterministic document relationships;
* deterministic code relationships;
* conservative entity relationships.

Omni-Sem must not initially attempt to replace specialized code intelligence platforms or full document-management applications.

---

# 5. Product Principles

## 5.1 Local first

All essential functionality must work without sending document contents to an external service.

Optional remote or network-accessible model providers may be supported later, but they must be:

* explicitly configured;
* disabled by default;
* clearly identified in status output;
* prevented from receiving data outside their declared purpose.

## 5.2 Native runtime

Users must not need Python, Node.js, Docker, or a separate database server to run the core product.

The preferred runtime distribution is a native Rust executable or small set of native executables.

## 5.3 Read only

Omni-Sem may:

* read files inside approved roots;
* create and update its own configuration;
* create and update its own derived database;
* write logs and operational metadata.

Omni-Sem must not:

* edit source documents;
* rename source documents;
* delete source documents;
* execute code found in source documents;
* accept arbitrary filesystem mutation through MCP.

## 5.4 Source grounded

Every retrieved result must identify its origin.

A result must include enough information to locate and verify the source passage.

At minimum, retrieval evidence should include:

* source identifier;
* relative path;
* revision identifier;
* segment identifier;
* structural anchor;
* original text;
* retrieval score or ranking information.

## 5.5 Derived data is disposable

The filesystem is the source of truth.

Search indexes, parsed segments, embeddings, entities, and relationships must be rebuildable from approved source files.

The user must be able to delete the derived index and reconstruct it without modifying source content.

## 5.6 Retrieval before graph intelligence

A knowledge graph is not the foundation of the project.

Omni-Sem must first demonstrate that lexical and semantic retrieval provide measurable value.

Graph traversal will only be prioritized when evaluation shows that it improves retrieval enough to justify:

* implementation complexity;
* maintenance cost;
* storage overhead;
* correctness risk;
* retrieval latency.

## 5.7 Evidence before inference

Deterministic structure and original text are more trustworthy than model-generated summaries or relationships.

Inferred information must never be stored as unquestioned fact.

Every inference must retain:

* supporting evidence;
* extraction method;
* extractor version;
* confidence;
* current status.

## 5.8 Agent neutral

Omni-Sem must not depend on:

* a particular agent;
* a particular IDE;
* a particular chat interface;
* a particular model provider;
* a particular embedding model.

## 5.9 Headless by default

The primary interfaces are:

* command-line tools;
* local IPC;
* MCP resources;
* MCP tools.

A graphical dashboard is outside the initial scope.

## 5.10 Graceful degradation

Omni-Sem must remain useful when:

* no embedding model is configured;
* Ollama is unavailable;
* semantic indexing is disabled;
* one parser fails;
* enrichment jobs remain incomplete;
* an optional feature was not compiled.

Lexical retrieval is the minimum operational baseline.

## 5.11 Measured complexity

Do not add infrastructure because it might eventually be useful.

New subsystems must be justified by:

* an observed limitation;
* a benchmark;
* an evaluation result;
* a concrete product requirement.

## 5.12 Interoperate before replacing

Omni-Sem must respect structure a user already maintains rather than asking them to reorganize around a new system.

This includes:

* honoring existing `.gitignore` and workspace ignore rules during discovery, rather than requiring a separate exclusion list to be rebuilt from scratch;
* reading existing personal-knowledge structures (for example, an Obsidian vault's folder and link layout) as-is, without requiring migration or import into an Omni-Sem-specific format;
* preferring existing citation and reference conventions already present in the source content over inventing new ones.

A user should never have to restructure their files to make Omni-Sem work. This principle exists because switching cost, not feature count, is the most common reason a local tool in this category goes uninstalled.

---

# 6. Language and Runtime Strategy

## 6.1 Primary language

Rust is the primary language for all runtime product components.

Rust should implement:

* CLI;
* configuration;
* filesystem discovery;
* hashing;
* parsing;
* SQLite persistence;
* full-text indexing;
* retrieval;
* context packing;
* MCP serving;
* background daemon;
* file watching;
* job scheduling;
* local IPC;
* embedding-provider integrations.

## 6.2 Why Rust is preferred

Omni-Sem is primarily a systems and infrastructure product.

Its core requirements favor:

* predictable resource use;
* durable background execution;
* safe concurrency;
* native packaging;
* low idle overhead;
* explicit error handling;
* strong ownership boundaries;
* reliable long-running processes;
* no mandatory language runtime.

Rust is therefore preferable to Python for the production core.

## 6.3 Optional Python usage

Python may be used for offline development tooling such as:

* retrieval-analysis notebooks;
* corpus generation;
* model comparison;
* plotting benchmark results;
* experimental NLP research;
* one-off dataset conversion.

Python tooling must remain outside the runtime dependency chain.

A user installing Omni-Sem must not need Python unless they explicitly choose to use development or research tools.

## 6.4 No premature polyglot architecture

Do not build a Python microservice for entity extraction during the initial phases.

Potential future Python integrations must be evaluated against simpler alternatives:

* Ollama API;
* ONNX models callable from Rust;
* external command adapters;
* optional offline enrichment tools.

A second runtime language must not become a hidden mandatory dependency.

---

# 7. Scope

## 7.1 Included in the initial implementation

* Cargo project foundation;
* explicit local-root configuration;
* first-run root suggestions during `omnisem init` (inspection and display only, never automatic indexing — see 22.1a);
* per-root and per-segment exclusion tagging for content that should be indexable but never returned unless explicitly requested (see 13.1a);
* Markdown discovery and parsing;
* plain-text and unparsed source-file discovery and deterministic chunking (no language-aware parsing; see 17.x), so the initial wedge is not limited to Markdown alone;
* immutable file revisions;
* stable document segments;
* human-readable revision history (`omnisem changes`, see 22.7) built directly from existing revision records — no new subsystem required;
* SQLite-backed metadata;
* SQLite FTS5 retrieval;
* command-line indexing;
* command-line querying;
* source-grounded retrieval;
* human-readable match explanation (which contributing terms or signals produced a result, see 18.4a);
* freshness and staleness indicators on retrieval results, derived from existing `modified_at` / `indexed_at` timestamps (see 18.8a);
* named, per-client context-budget presets (see 18.9);
* configurable result and token budgets;
* read-only MCP server over STDIO;
* reproducible retrieval evaluation exposed as a stable, user-facing CLI command (`omnisem eval`, see 22.8), not only an internal harness;
* structured logging;
* tests and documentation.

## 7.2 Included in later phases

* git-aware incremental indexing (`omnisem index --since`, optional git hook) as an interim freshness mechanism prior to the full daemon and watcher (see 17.x and Milestone 3);
* exportable and importable read-only index snapshots for small-team sharing, still fully local-first (see 22.9);
* optional embedding-provider abstraction;
* Ollama embedding provider;
* optional in-process embedding provider;
* hybrid lexical and vector retrieval;
* persistent background daemon;
* incremental file watcher;
* lightweight MCP-to-daemon bridge;
* Streamable HTTP MCP transport;
* PDF parsing;
* Python source parsing;
* TypeScript source parsing;
* deterministic document relationships;
* deterministic code relationships;
* entity mentions;
* conservative entity resolution;
* optional local-model enrichment;
* bounded graph expansion;
* optional local status interface (a lightweight, read-only, non-interactive local view of indexed roots, counts, and last-index time — not a general dashboard or GUI; see 22.1b).

## 7.3 Explicitly excluded

Do not implement these without a deliberate future scope revision:

* chat interface;
* bundled AI assistant;
* autonomous planning;
* file editing through MCP;
* shell-command execution;
* arbitrary filesystem access;
* automatic cloud synchronization;
* multi-device replication;
* enterprise RBAC;
* shared collaborative workspaces;
* browser extension;
* mobile application;
* automatic external API usage;
* unbounded graph traversal;
* opaque LLM summaries as canonical knowledge;
* Docker as an end-user requirement;
* mandatory Python sidecar services.

---

# 8. Success Definition

The first meaningful success state is:

> An MCP-compatible AI client can query an approved Markdown workspace and receive a small set of relevant, source-grounded passages without manually exploring or reading the complete directory.

The final answer produced by the AI client is outside Omni-Sem’s responsibility.

Omni-Sem is responsible for:

* identifying relevant evidence;
* preserving provenance;
* respecting the requested context budget;
* excluding stale or deleted revisions;
* exposing predictable interfaces;
* failing safely;
* avoiding unrestricted filesystem access.

---

# 9. High-Level Architecture

Omni-Sem will eventually contain three major runtime layers.

```text
                    AI client / IDE / agent
                              |
                         MCP over STDIO
                              |
                    +-------------------+
                    |   omnisem-mcp     |
                    | lightweight       |
                    | client bridge     |
                    +---------+---------+
                              |
                  authenticated local IPC
                              |
  +---------------------------v----------------------------+
  |                        omnisemd                        |
  |                                                       |
  | Query service        Job scheduler      Health API    |
  |      |                     |                           |
  | Retrieval engine     Ingestion pipeline               |
  |      |                     |                           |
  | Context packer       Parser adapters                  |
  |      |                     |                           |
  | Vector provider      Extraction and enrichment        |
  +---------------------------+----------------------------+
                              |
                    +---------v----------+
                    | Derived local data |
                    | SQLite, FTS,       |
                    | vectors, evidence  |
                    +--------------------+
                              |
                    Approved source roots
```

## 9.1 Initial implementation architecture

Do not begin with all three runtime layers.

The initial implementation should operate through one CLI executable:

```text
omnisem init
omnisem root add
omnisem index
omnisem query
omnisem status
omnisem changes
omnisem eval
omnisem mcp
```

`omnisem changes` and `omnisem eval` are detailed in Section 22. Both read existing derived data (revisions, evaluation-corpus results) and require no new runtime subsystem.

The MCP command may open the local database directly during the first implementation.

## 9.2 Future process separation

Once continuous indexing is introduced:

* `omnisemd` becomes the persistent owner of writes, jobs, and model resources;
* `omnisem-mcp` becomes a lightweight STDIO process launched by the MCP client;
* the bridge forwards requests through authenticated local IPC;
* multiple MCP clients share one daemon;
* client shutdown does not stop indexing;
* embedding models are loaded once rather than once per MCP client.

Do not build this IPC layer before static indexing and retrieval quality are proven.

---

# 10. Rust Project Structure

## 10.1 Initial recommendation

Begin with a small Cargo workspace containing one core library and one executable crate.

```text
omni-sem/
├── Cargo.toml
├── Cargo.lock
├── crates/
│   ├── omnisem-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   └── omnisem-cli/
│       ├── Cargo.toml
│       └── src/
├── docs/
├── evals/
├── tests/
├── tools/
└── scripts/
```

Responsibilities:

### `omnisem-core`

Contains reusable application logic:

* domain models;
* configuration;
* discovery;
* parsing;
* persistence interfaces;
* SQLite implementation;
* retrieval;
* context packing;
* MCP service implementation.

### `omnisem-cli`

Contains:

* CLI argument parsing;
* command dispatch;
* process startup;
* terminal formatting;
* exit-code mapping;
* application-level dependency wiring.

## 10.2 Future crate extraction

Do not immediately create separate crates for every conceptual layer.

Extract crates only when there is a demonstrated reason, such as:

* independent compilation features;
* incompatible dependency weight;
* clear process boundaries;
* reusable public interfaces;
* excessive compile times;
* cyclic architectural pressure.

Potential future crates may include:

```text
omnisem-domain
omnisem-storage
omnisem-ingest
omnisem-retrieval
omnisem-mcp
omnisem-daemon
omnisem-embedding
```

These are possible future boundaries, not required starting boundaries.

## 10.3 Rust edition and toolchain

Use a stable Rust edition supported by the selected dependencies.

Record:

* Rust edition;
* minimum supported Rust version, once known;
* formatting requirements;
* lint policy;
* required Cargo features.

Do not set an artificially old minimum supported Rust version before dependency compatibility is established.

---

# 11. Recommended Rust Stack

Candidate dependencies should remain replaceable and justified.

## 11.1 Core runtime

* Async runtime: `tokio`
* CLI: `clap`
* Serialization: `serde`, `serde_json`, `toml`
* Structured logging: `tracing`, `tracing-subscriber`
* Domain errors: `thiserror`
* Application-boundary errors: `anyhow` where appropriate
* Hashing: `blake3`
* Temporary test files: `tempfile`

## 11.2 Persistence

* SQLite: `rusqlite`
* Prefer bundled SQLite support where practical
* Verify FTS5 availability in automated tests
* Use a project-owned migration runner initially
* Avoid adding an ORM unless manual SQL becomes an observed maintenance problem

## 11.3 Filesystem

* Recursive discovery: `ignore` or `walkdir`
* File watching later: `notify`
* Event coalescing later: a compatible debouncer or project-owned debounce layer
* Platform paths: `directories` or equivalent

## 11.4 Markdown

Candidate parser:

* `pulldown-cmark`, or
* `comrak` if source-position and extension requirements justify it.

The parser decision must consider:

* source offsets;
* heading structure;
* tables;
* code fences;
* links;
* predictable event output;
* preservation of evidence anchors.

## 11.5 MCP

Use the official Rust MCP SDK when practical.

Keep MCP transport types out of the core retrieval domain.

MCP handlers should translate protocol requests into application service calls rather than embedding retrieval logic directly.

## 11.6 Embeddings

Initial provider order:

1. disabled provider;
2. Ollama-compatible HTTP provider;
3. optional in-process embedding provider.

Potential in-process implementation:

* ONNX-based embedding through a Rust-compatible library.

In-process embeddings must be optional because they may:

* significantly increase binary size;
* increase build complexity;
* require model downloads;
* increase memory usage;
* complicate cross-platform packaging.

## 11.7 Code parsing later

Use Tree-sitter-based parsers for structured source-code ingestion.

Do not add code-parser dependencies to the initial Markdown-only build.

## 11.8 Testing

* built-in Rust tests;
* `proptest` for property-based tests;
* `insta` for carefully reviewed snapshot tests;
* `tempfile` for isolated integration environments;
* `criterion` for benchmarks when performance work begins.

---

# 12. Feature Flags

Optional functionality should use Cargo features where this improves installation and dependency isolation.

Potential future features:

```toml
[features]
default = ["markdown", "mcp"]

markdown = []
mcp = []
embeddings-ollama = []
embeddings-local = []
pdf = []
code-python = []
code-typescript = []
daemon = []
http = []
full = [
  "markdown",
  "mcp",
  "embeddings-ollama",
  "embeddings-local",
  "pdf",
  "code-python",
  "code-typescript",
  "daemon",
  "http"
]
```

Do not create feature flags with no implementation purpose.

The default build should remain small and useful.

---

# 13. Canonical Domain Model

The canonical model is based on roots, sources, revisions, segments, evidence, and derived retrieval signals.

The graph is a derived view, not the authoritative representation.

## 13.1 Root

Represents an approved filesystem boundary.

Suggested Rust model:

```rust
pub struct Root {
    pub id: RootId,
    pub canonical_path: PathBuf,
    pub display_name: String,
    pub include_patterns: Vec<String>,
    pub exclude_patterns: Vec<String>,
    pub follow_symlinks: bool,
    pub enabled: bool,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
```

Rules:

* roots must be explicitly configured;
* paths must be canonicalized;
* following symlinks defaults to false;
* a resolved path must never escape its root;
* include and exclude rules must be inspectable;
* a broad home-directory root must never be added automatically.

## 13.1a Sensitive-content exclusion

`include_patterns` and `exclude_patterns` control whether a file is indexed at all. A separate, finer-grained control is needed for content that should be indexed (so it remains locally searchable by the user) but must never be surfaced to an MCP client or included in retrieval results unless the request explicitly opts in.

```rust
pub struct SensitivityTag {
    pub pattern: String,
    pub scope: SensitivityScope,
}

pub enum SensitivityScope {
    NeverReturnToMcp,
    RequireExplicitQuery,
}
```

Rules:

* sensitivity tags are per-root and may be overridden per-segment via frontmatter or a recognized in-document marker;
* a segment tagged `NeverReturnToMcp` must never appear in an MCP tool response, even as a low-ranked or truncated result;
* sensitivity tags are inspectable through `omnisem status` and `omnisem doctor`;
* sensitivity tags never delete or modify indexed data — they only gate retrieval visibility, consistent with the read-only and derived-data-is-disposable principles.

## 13.2 Source file

Represents the current filesystem identity of one file.

```rust
pub struct SourceFile {
    pub id: SourceFileId,
    pub root_id: RootId,
    pub relative_path: PathBuf,
    pub canonical_path_hash: ContentHash,
    pub filesystem_identity: Option<FilesystemIdentity>,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
    pub modified_at: Timestamp,
    pub current_revision_id: Option<RevisionId>,
    pub state: SourceState,
    pub first_seen_at: Timestamp,
    pub last_seen_at: Timestamp,
}
```

Possible states:

```rust
pub enum SourceState {
    Active,
    Deleted,
    Excluded,
    Unsupported,
    Error,
}
```

## 13.3 Revision

An immutable record of one observed content version.

```rust
pub struct Revision {
    pub id: RevisionId,
    pub source_file_id: SourceFileId,
    pub content_hash: ContentHash,
    pub parser_id: String,
    pub parser_version: String,
    pub extracted_text_hash: Option<ContentHash>,
    pub observed_at: Timestamp,
    pub indexed_at: Option<Timestamp>,
    pub status: RevisionStatus,
    pub error: Option<StoredError>,
}
```

Requirements:

* revisions are immutable;
* identical content should not create duplicate revisions unnecessarily;
* parser-version changes may invalidate derived parsing output;
* stale revisions must not appear in normal retrieval;
* current-revision promotion must be transactional.

## 13.4 Segment

An addressable unit of source evidence.

```rust
pub struct Segment {
    pub id: SegmentId,
    pub revision_id: RevisionId,
    pub segment_type: SegmentType,
    pub anchor: SegmentAnchor,
    pub ordinal: u32,
    pub text: String,
    pub text_hash: ContentHash,
    pub token_count: Option<u32>,
    pub metadata: serde_json::Value,
}
```

Possible Markdown segment types:

```rust
pub enum SegmentType {
    DocumentTitle,
    Heading,
    Paragraph,
    List,
    Blockquote,
    CodeFence,
    Table,
    Frontmatter,
}
```

Example anchors:

```text
heading:architecture
heading:architecture/storage
paragraph:12
code-fence:rust:3
```

Anchors should remain as stable as reasonably possible across small edits.

## 13.5 Embedding

A vector representation of a segment.

```rust
pub struct EmbeddingRecord {
    pub segment_id: SegmentId,
    pub model_id: String,
    pub model_version: Option<String>,
    pub dimensions: usize,
    pub vector: Vec<f32>,
    pub created_at: Timestamp,
}
```

Embedding data is derived and rebuildable.

Vectors from incompatible models or dimensions must never be mixed silently.

## 13.6 Entity

A tentative canonical concept.

```rust
pub struct Entity {
    pub id: EntityId,
    pub entity_type: EntityType,
    pub canonical_label: String,
    pub normalization_key: String,
    pub resolution_status: ResolutionStatus,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}
```

Entities are not part of the first implementation milestone.

## 13.7 Mention

An observed occurrence of an entity candidate inside a segment.

```rust
pub struct Mention {
    pub id: MentionId,
    pub entity_id: EntityId,
    pub segment_id: SegmentId,
    pub surface_text: String,
    pub start_offset: u32,
    pub end_offset: u32,
    pub extractor_id: String,
    pub extractor_version: String,
    pub confidence: f32,
}
```

A matching string does not automatically establish entity identity.

## 13.8 Relationship

An evidence-backed relationship between entities or source structures.

```rust
pub struct Relationship {
    pub id: RelationshipId,
    pub subject_id: NodeId,
    pub predicate: String,
    pub object_id: NodeId,
    pub evidence_segment_id: SegmentId,
    pub origin: RelationshipOrigin,
    pub extractor_id: String,
    pub extractor_version: String,
    pub confidence: f32,
    pub status: RelationshipStatus,
    pub created_at: Timestamp,
    pub superseded_by: Option<RelationshipId>,
}
```

Possible origins:

```rust
pub enum RelationshipOrigin {
    Deterministic,
    Nlp,
    Llm,
    User,
}
```

Possible statuses:

```rust
pub enum RelationshipStatus {
    Observed,
    Proposed,
    Validated,
    Rejected,
    Superseded,
}
```

## 13.9 Job

Represents asynchronous ingestion or enrichment work.

```rust
pub struct Job {
    pub id: JobId,
    pub job_type: JobType,
    pub target_id: String,
    pub priority: i32,
    pub state: JobState,
    pub attempt_count: u32,
    pub queued_at: Timestamp,
    pub started_at: Option<Timestamp>,
    pub finished_at: Option<Timestamp>,
    pub error: Option<StoredError>,
}
```

Possible states:

```rust
pub enum JobState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Superseded,
}
```

---

# 14. Domain Type Policy

Use domain-specific newtypes for identifiers and values where confusion would create correctness risk.

Examples:

```rust
pub struct RootId(Uuid);
pub struct SourceFileId(Uuid);
pub struct RevisionId(Uuid);
pub struct SegmentId(Uuid);
pub struct ContentHash(String);
pub struct TokenBudget(u32);
pub struct RetrievalLimit(u16);
```

Requirements:

* avoid passing unvalidated strings throughout the application;
* parse and validate protocol inputs at boundaries;
* keep database row conversion separate from domain logic;
* keep MCP schema types separate from internal domain types;
* avoid unnecessary cloning of large source text;
* do not expose internal database row IDs as filesystem authority.

---

# 15. Storage Architecture

## 15.1 Initial system of record

Use SQLite as the initial system of record.

SQLite should store:

* roots;
* source files;
* revisions;
* segments;
* parser metadata;
* configuration fingerprints;
* job state;
* FTS indexes;
* embedding metadata;
* schema version;
* operational metadata where appropriate.

## 15.2 SQLite integration

Use `rusqlite` initially.

Prefer explicit SQL and explicit transaction boundaries.

Create a storage abstraction at the application boundary, but do not create an excessively generic database framework.

Suggested traits may include:

```rust
pub trait RootRepository {
    fn insert_root(&self, root: &Root) -> Result<()>;
    fn list_roots(&self) -> Result<Vec<Root>>;
}

pub trait RevisionRepository {
    fn find_by_hash(
        &self,
        source_id: &SourceFileId,
        hash: &ContentHash,
    ) -> Result<Option<Revision>>;

    fn commit_revision(
        &mut self,
        revision: &Revision,
        segments: &[Segment],
    ) -> Result<()>;
}
```

Use traits where they establish meaningful application boundaries.

Do not create traits only to mock every internal function.

## 15.3 Full-text retrieval

Use SQLite FTS5 for the lexical baseline.

The lexical index should support:

* keyword search;
* phrase search;
* prefix search where appropriate;
* relevance ranking;
* snippets or highlighting;
* filtering by root;
* filtering by file type;
* filtering to active revisions.

The application must verify FTS5 availability at startup or through `omnisem doctor`.

## 15.4 Initial vector strategy

For the first semantic evaluation corpus:

* keep vector operations behind a clear trait;
* exact similarity search is acceptable;
* do not add a heavyweight vector database;
* do not optimize for millions of vectors before measuring a need.

Suggested trait:

```rust
#[async_trait::async_trait]
pub trait VectorIndex: Send + Sync {
    async fn upsert(
        &self,
        records: &[VectorRecord],
    ) -> Result<(), VectorError>;

    async fn delete(
        &self,
        segment_ids: &[SegmentId],
    ) -> Result<(), VectorError>;

    async fn search(
        &self,
        vector: &[f32],
        limit: usize,
        filters: &SearchFilters,
    ) -> Result<Vec<VectorHit>, VectorError>;
}
```

An async trait is only justified if implementations may perform network or asynchronous I/O.

If all initial implementations are synchronous, prefer a synchronous trait and isolate blocking work appropriately.

## 15.5 Future vector implementations

Potential later implementations may include:

* a SQLite vector extension;
* an embedded columnar/vector engine;
* another embedded vector implementation demonstrated to be necessary.

Do not introduce:

* Milvus;
* Chroma;
* Elasticsearch;
* Neo4j;
* a required Docker service;

into the initial user installation.

## 15.6 Database ownership

Before the daemon exists, CLI commands may open the database directly.

After the daemon is introduced:

* the daemon should become the primary writer;
* query access should follow a defined concurrency model;
* direct CLI write access should either route through the daemon or require exclusive ownership;
* concurrent-write behavior must be tested.

## 15.7 Migrations

Implement a small project-owned migration system initially.

Each migration should include:

* monotonic schema version;
* SQL statements;
* transactional application where possible;
* clear failure handling;
* migration tests.

Do not silently continue after a partially applied migration.

---

# 16. Configuration

Use TOML for human-readable configuration.

Use platform-aware directories rather than hard-coded Linux-only paths.

Example configuration:

```toml
[general]
database_path = "~/.local/share/omnisem/index.sqlite3"
log_level = "info"

[[roots]]
name = "notes"
path = "~/Documents/Notes"
include = ["**/*.md"]
exclude = [
  "**/.git/**",
  "**/.obsidian/cache/**",
  "**/node_modules/**",
  "**/.venv/**"
]
follow_symlinks = false

[retrieval]
default_mode = "lexical"
default_limit = 8
default_token_budget = 4000

[embeddings]
enabled = false
provider = "none"
model = ""
endpoint = ""
```

Initial configuration behavior:

* `omnisem init` creates a safe default configuration;
* no broad root is automatically added;
* roots require explicit local action;
* invalid configuration returns actionable errors;
* configuration affecting derived data receives a fingerprint;
* unknown fields should produce warnings or errors according to a documented compatibility policy.

Do not expand `~` or environment variables in multiple unrelated layers. Normalize configuration paths at one clear boundary.

---

# 17. Ingestion Pipeline

## 17.1 Discovery

The discovery stage must:

* scan configured roots;
* apply include and exclude patterns;
* canonicalize paths;
* reject path traversal;
* reject root escapes;
* reject unsupported file types;
* detect new, changed, unchanged, and deleted files;
* calculate content hashes;
* avoid reprocessing unchanged content.

Modification time alone is not a sufficient content identity check.

Discovery should honor an existing `.gitignore` (and equivalent workspace ignore files) within a root by default, using the same `ignore` crate that already provides recursive discovery (Section 11.3), so a user's existing exclusion rules are respected without being restated in Omni-Sem configuration. This is consistent with Section 5.12.

## 17.2 Discovery representation

Suggested type:

```rust
pub struct DiscoveredDocument {
    pub root_id: RootId,
    pub canonical_path: PathBuf,
    pub relative_path: PathBuf,
    pub size_bytes: u64,
    pub modified_at: SystemTime,
    pub file_type: SupportedFileType,
}
```

Discovery should not automatically read complete file content unless required for hashing or parsing.

## 17.3 Parsing

Parsing must use adapter-style boundaries.

Suggested trait:

```rust
pub trait DocumentParser: Send + Sync {
    fn parser_id(&self) -> &'static str;

    fn parser_version(&self) -> &'static str;

    fn supports(
        &self,
        document: &DiscoveredDocument,
    ) -> bool;

    fn parse(
        &self,
        source: &SourceDocument,
    ) -> Result<ParsedDocument, ParseError>;
}
```

A parser should return:

* normalized document metadata;
* ordered segments;
* stable anchors;
* deterministic structural relationships where available;
* parser warnings;
* parser identity and version.

## 17.4 Markdown parser

The first parser supports Markdown.

It should recognize:

* frontmatter;
* document title;
* heading hierarchy;
* paragraphs;
* lists;
* blockquotes;
* code fences;
* Markdown links;
* tables where practical.

Chunking must follow document structure.

Do not begin by splitting text into arbitrary fixed-size character windows.

Large structural sections may be subdivided using token-aware or size-aware boundaries while retaining their parent heading path.

## 17.4a Plain-text and unparsed source discovery

To avoid limiting the initial wedge to Markdown alone, a minimal parser may treat any discovered text file that no structured parser claims (plain `.txt`, and source files in languages without a Tree-sitter adapter yet) as a single-segment or size-bounded-chunk document, with no language-aware structure.

Requirements:

* this must not require any new dependency — it reuses the existing chunking and hashing machinery;
* it must not attempt code-aware parsing (imports, symbols, calls); that remains explicitly deferred to Tree-sitter adapters in Milestone 7;
* segments produced this way are tagged with a distinct `parser_id` (for example, `plain-text-v1`) so they are never confused with structure-aware Markdown or future code segments in evaluation results;
* this parser is intentionally simple: its purpose is corpus coverage for the first release, not code intelligence.

## 17.5 Transactional revision promotion

A file revision should become current only after all required baseline stages succeed.

Recommended flow:

```text
discover file
    → read stable content
    → hash content
    → parse content
    → prepare segments
    → begin transaction
    → insert revision
    → insert segments
    → update FTS
    → promote current revision
    → commit transaction
```

If parsing or database insertion fails:

* preserve the previous active revision;
* record the failure;
* do not expose a partial new revision;
* allow the rest of the scan to continue.

## 17.6 Lexical indexing

Every valid text segment must enter FTS before optional semantic processing.

A corpus must remain queryable when semantic features are disabled.

## 17.7 Embedding generation

Embedding generation is an optional derived stage.

Suggested trait:

```rust
#[async_trait::async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn model_id(&self) -> &str;

    fn dimensions(&self) -> usize;

    async fn embed(
        &self,
        texts: &[String],
    ) -> Result<Vec<Vec<f32>>, EmbeddingError>;
}
```

Initial providers:

* `DisabledEmbeddingProvider`;
* `OllamaEmbeddingProvider`.

Later provider:

* optional in-process ONNX embedding provider.

Embedding caching must use:

* segment text hash;
* model identity;
* model version when available;
* dimensions;
* provider configuration affecting output.

## 17.8 Future watcher behavior

When a watcher is added:

* watcher events are hints, not authoritative database mutations;
* rapid save events must be coalesced;
* atomic-save rename patterns must be supported;
* temporary files must be ignored;
* deletion and rename behavior must be tested;
* periodic reconciliation scans must correct missed events;
* blocking filesystem work must not stall the async runtime.

---

# 18. Retrieval Pipeline

## 18.1 Retrieval modes

Support these conceptual modes:

```rust
pub enum RetrievalMode {
    Lexical,
    Semantic,
    Hybrid,
    Auto,
}
```

For the first milestone, lexical mode is mandatory.

Hybrid mode becomes available after embeddings exist.

`Auto` should select the best currently available safe mode rather than failing when semantic search is unavailable.

## 18.2 Retrieval service

Suggested application interface:

```rust
pub trait RetrievalService {
    fn search(
        &self,
        query: &RetrievalQuery,
    ) -> Result<RetrievalResponse, RetrievalError>;
}
```

The implementation may become asynchronous when network embedding providers or daemon IPC are involved.

Do not make all domain logic asynchronous unless actual I/O requires it.

## 18.3 Hybrid retrieval flow

```text
User query
    |
    +-- lexical search
    |
    +-- query embedding, when available
    |
    +-- vector search
    |
    +-- metadata filtering
    |
    +-- rank fusion
    |
    +-- optional bounded structural expansion
    |
    +-- duplicate suppression
    |
    +-- source diversification
    |
    +-- token-budget packing
    |
    +-- provenance-rich response
```

## 18.4 Ranking

Begin with a transparent rank-fusion method such as Reciprocal Rank Fusion.

Do not build an opaque learned ranking system before a labeled evaluation corpus exists.

Each result should retain its contributing retrieval signals.

```rust
pub struct RetrievalSignals {
    pub lexical_score: Option<f32>,
    pub semantic_score: Option<f32>,
    pub structural_score: Option<f32>,
    pub fused_rank: usize,
}
```

## 18.4a Match explanation

`RetrievalSignals` records *how strongly* each channel contributed; it does not record *what* contributed. A user or downstream agent deciding whether to trust a result benefits from knowing which specific terms or matched spans drove the score, not only a numeric breakdown.

```rust
pub struct MatchExplanation {
    pub matched_terms: Vec<String>,
    pub matched_span: Option<SegmentAnchor>,
    pub explanation_kind: ExplanationKind,
}

pub enum ExplanationKind {
    LexicalTermOverlap,
    SemanticNeighbor,
    StructuralExpansion,
}
```

This is populated from data the retrieval engine already computes during ranking (FTS5 match offsets for lexical hits; nearest-neighbor identity for semantic hits) and must not require a separate inference step or model call. Human-readable CLI output should surface `matched_terms` inline; JSON output includes the full structure.

## 18.5 Duplicate suppression

The retrieval engine should detect:

* identical segments;
* overlapping segments;
* duplicate content across copied files;
* adjacent segments that waste the context budget;
* repeated evidence from stale revisions.

It may merge adjacent results from the same document when doing so improves coherence without obscuring provenance.

## 18.6 Context packing

The context packer must respect a caller-supplied token budget.

It should prioritize:

* highly relevant evidence;
* non-duplicative passages;
* source diversity;
* coherent neighboring context;
* direct source text.

Default retrieval must not use an LLM to summarize evidence.

Original text is preferred because it preserves trust and avoids another inference layer.

## 18.7 Token estimation

Token estimation must be isolated behind an interface.

Different models tokenize text differently, so the initial implementation may use:

* a documented conservative estimator;
* a configured tokenizer where available;
* explicit indication that counts are estimates.

Suggested interface:

```rust
pub trait TokenEstimator {
    fn estimate(&self, text: &str) -> usize;
}
```

The context packer must never exceed its internal byte or segment safety limits even if token estimation is imperfect.

## 18.8 Retrieval response

A normalized response should resemble:

```json
{
  "query": "What storage design was selected?",
  "mode": "hybrid",
  "results": [
    {
      "segment_uri": "omnisem://segment/SEGMENT_ID",
      "document_uri": "omnisem://document/DOCUMENT_ID",
      "path": "notes/architecture.md",
      "revision": "blake3:CONTENT_HASH",
      "anchor": "heading:storage",
      "text": "SQLite will be used as the initial system of record...",
      "score": 0.89,
      "signals": {
        "lexical": 0.73,
        "semantic": 0.92,
        "structural": null
      },
      "explanation": {
        "matched_terms": ["storage", "architecture"],
        "explanation_kind": "lexical_term_overlap"
      },
      "freshness": {
        "indexed_at": "2026-07-20T14:03:00Z",
        "source_modified_at": "2026-07-20T14:01:12Z",
        "status": "current"
      }
    }
  ],
  "token_estimate": 1240,
  "truncated": false,
  "warnings": []
}
```

## 18.8a Freshness and staleness indicators

Every result already carries a revision identifier; the domain model already records `SourceFile.modified_at` and `Revision.observed_at` / `Revision.indexed_at`. Freshness status is a read-only comparison of these existing fields — no new tracking mechanism is required.

```rust
pub enum FreshnessStatus {
    Current,
    PendingReindex,
    Unknown,
}
```

`PendingReindex` indicates the filesystem's `modified_at` is newer than the indexed revision — for example, because no watcher is running and the user has not re-run `omnisem index`. This must never block a result from being returned; it only informs the caller that a newer version may exist on disk. This directly mitigates the absence of a watcher in early milestones by making staleness visible instead of silent.

## 18.9 Context-budget presets

Raw token budgets require the caller to know the target model's effective context window and reserved overhead. Named presets remove that burden for common clients without hardcoding model-specific behavior into retrieval logic itself.

```rust
pub struct BudgetPreset {
    pub name: String,
    pub token_budget: usize,
    pub max_results: usize,
}
```

Presets are plain configuration, not special-cased logic — they resolve to the same `RetrievalQuery` fields a caller could set manually with `--token-budget` and `--limit`. Initial presets may include conservative, mid-sized, and small-context profiles; exact names and values are a configuration decision, not an architectural one, and must remain user-editable rather than hardcoded.

---

# 19. Graph and Relationship Policy

The word “semantic” must not be treated as permission to generate an unreliable ontology.

## 19.1 Deterministic relationships

Relationships derived directly from source structure may include:

* Markdown links;
* heading containment;
* document containment;
* citations;
* code imports;
* code definitions;
* code calls where statically identifiable;
* revision lineage;
* parent-child segment relationships.

These relationships may receive high trust.

## 19.2 Extracted relationships

Relationships produced through NLP or a language model may include:

* mentions;
* possible aliases;
* discusses;
* supports;
* contradicts;
* conceptually depends on.

These must retain:

* evidence;
* confidence;
* extraction method;
* extractor version;
* model identity where applicable;
* validation status.

## 19.3 Graph retrieval

Graph expansion must be:

* optional;
* bounded by maximum depth;
* bounded by maximum nodes;
* limited by relation type;
* limited by confidence;
* limited by context budget;
* explainable in results.

No unbounded multi-hop traversal is permitted.

## 19.4 Graph implementation gate

Do not prioritize graph traversal until evaluation demonstrates improvement over lexical and semantic search.

Required comparison:

1. lexical retrieval;
2. semantic retrieval;
3. fused lexical and semantic retrieval;
4. fused retrieval plus deterministic structural expansion;
5. fused retrieval plus inferred relationships.

If graph-enhanced retrieval does not produce meaningful improvement, retain relationships only as inspectable metadata or experimental functionality.

---

# 20. MCP Interface

## 20.1 Initial transport

Use MCP over STDIO for the first release.

The MCP process must use standard output exclusively for protocol messages.

Logs must go to:

* standard error;
* a configured log file;
* or another non-protocol sink.

## 20.2 Future transport

Streamable HTTP may be added later for:

* local network clients;
* daemon-native connections;
* clients that do not launch STDIO subprocesses.

Do not implement legacy standalone SSE as the primary architecture.

## 20.3 MCP architecture boundary

MCP handlers must remain thin adapters.

They should:

1. validate protocol input;
2. convert it into domain requests;
3. call application services;
4. convert domain output into MCP responses;
5. map errors into safe protocol errors.

They must not:

* contain SQL;
* perform arbitrary file reads;
* implement retrieval ranking directly;
* bypass root restrictions.

## 20.4 MCP resources

Suggested resource URI patterns:

```text
omnisem://document/{document_id}
omnisem://revision/{revision_id}
omnisem://segment/{segment_id}
omnisem://entity/{entity_id}
omnisem://status
```

Resources must resolve only to:

* content already indexed from approved roots;
* Omni-Sem’s own operational metadata.

## 20.5 Initial MCP tools

### `search_context`

Purpose:

Retrieve source-grounded passages relevant to a natural-language query.

Suggested input:

```json
{
  "query": "string",
  "root_ids": [],
  "file_types": [],
  "mode": "auto",
  "limit": 8,
  "token_budget": 4000
}
```

### `get_context`

Purpose:

Retrieve one or more known Omni-Sem resources, optionally with bounded neighboring segments.

Suggested input:

```json
{
  "uris": [
    "omnisem://segment/SEGMENT_ID"
  ],
  "neighbor_segments": 1,
  "token_budget": 4000
}
```

### `index_status`

Purpose:

Return operational state without exposing unnecessary source content.

Suggested output includes:

* configured roots;
* indexed-document count;
* active-revision count;
* failed-document count;
* pending jobs;
* last completed scan;
* embedding-provider availability;
* schema version.

### `find_connections`

Do not implement until relationship storage exists.

Purpose:

Retrieve bounded, evidence-backed relationships around a concept or resource.

## 20.6 Prohibited MCP tools

Do not expose:

* arbitrary `read_file(path)`;
* `write_file`;
* `delete_file`;
* shell execution;
* arbitrary SQL;
* unrestricted directory listing;
* dynamic root addition without explicit local user action.

MCP callers must use Omni-Sem resource identifiers rather than arbitrary filesystem paths.

## 20.7 Untrusted-content warning

MCP tool descriptions and responses must make clear that retrieved document text is untrusted source material.

Text found inside indexed documents must never override:

* system instructions;
* developer instructions;
* user instructions;
* tool-security policy.

---

# 21. Security Model

## 21.1 Filesystem controls

Omni-Sem must:

* require explicit root approval;
* canonicalize every path before reading;
* reject root escapes;
* default to not following symlinks;
* reject device files;
* reject sockets;
* reject named pipes;
* enforce maximum file sizes;
* enforce extraction limits;
* support exclusions;
* avoid secret-bearing files by default;
* never return content outside approved roots.

## 21.2 Sensitive-file defaults

Default exclusions should consider common examples such as:

```text
.env
.env.*
*.pem
*.key
id_rsa
id_ed25519
credentials.*
secrets.*
.git/
node_modules/
.venv/
target/
```

These defaults must be documented and configurable.

Filename exclusions do not guarantee secret detection. Omni-Sem must not claim otherwise.

## 21.3 Database protection

The derived database may contain sensitive extracted text.

Requirements:

* use user-only file permissions where supported;
* do not log complete document contents by default;
* prevent error messages from leaking source text unnecessarily;
* require explicit action for exports;
* document backup sensitivity;
* require authentication for future remote transports.

## 21.4 Local IPC

When the persistent daemon is introduced:

* use user-scoped local IPC;
* authenticate bridge connections;
* enforce protocol-version negotiation;
* enforce request-size limits;
* enforce timeouts;
* reject malformed requests;
* avoid public network binding by default.

Potential platform transports:

* Unix-domain socket on Linux and macOS;
* named pipe on Windows;
* authenticated loopback only when platform constraints justify it.

## 21.5 Prompt injection

Retrieved source content may contain malicious or misleading instructions.

Omni-Sem must:

* treat indexed content as data;
* label it as untrusted evidence;
* avoid interpreting embedded commands;
* never execute retrieved content;
* preserve source locations;
* avoid autonomous actions.

---

# 22. CLI Design

Initial commands:

```bash
omnisem init
omnisem root add PATH
omnisem root suggest
omnisem root list
omnisem root remove ROOT_ID
omnisem index
omnisem index --since REVISION
omnisem query QUERY
omnisem changes
omnisem eval
omnisem status
omnisem status --serve
omnisem doctor
omnisem snapshot export PATH
omnisem snapshot import PATH
omnisem mcp
omnisem rebuild
```

## 22.1 `omnisem init`

Responsibilities:

* create configuration directories;
* create a default configuration;
* initialize the database;
* apply initial migrations;
* print resulting paths;
* avoid indexing anything automatically.

## 22.1a `omnisem root suggest`

Lowers first-run setup friction without weakening the explicit-root-approval rule in Section 5.1 and 13.1.

Responsibilities:

* scan common project and notes locations (for example, the current directory, a configured list of typical workspace parents) for candidate directories containing supported file types;
* display candidates with file-type counts and estimated sizes;
* never index, read file contents beyond what is needed for type detection, or add a root automatically;
* require the user to run `omnisem root add` explicitly for any suggested path.

This command performs discovery only. It must not be a shortcut around explicit root approval.

## 22.1b `omnisem status --serve`

An optional flag on the existing `status` command that starts a local, read-only, non-interactive HTTP view (bound to localhost only, no external network exposure) showing indexed roots, file and segment counts, last-index time, and recent query activity.

Constraints:

* read-only — no mutation is possible through this interface;
* no authentication is implemented because it is not exposed beyond localhost by default;
* this is not a dashboard, admin panel, or query interface — it exists solely to make index state visible without requiring `omnisem status` to be re-run repeatedly on the command line;
* must be entirely optional and off by default.

## 22.2 `omnisem root add`

Responsibilities:

* canonicalize the path;
* verify it exists;
* verify it is a directory;
* show the resolved path;
* save safe include and exclude defaults;
* honor existing `.gitignore` and workspace ignore files during discovery by default, consistent with Section 5.12;
* reject invalid or dangerous paths clearly.

## 22.3 `omnisem index`

Responsibilities:

* discover documents;
* identify changes;
* parse supported files;
* write immutable revisions;
* update current-revision pointers transactionally;
* update FTS;
* optionally queue embeddings;
* print a concise summary;
* return an appropriate exit code.

A failure in one document must not corrupt the index or abort unrelated files unless the database itself becomes unavailable.

### 22.3a `omnisem index --since`

Within a git-managed root, `--since REVISION` (defaulting to the last indexed git commit) limits discovery to files changed according to `git diff --name-only`, falling back to full filesystem discovery when the root is not a git repository or the reference is unavailable.

This is an interim freshness mechanism — see Section 17 and Milestone 3 — that gives near-real-time index freshness for the common case of a git-tracked workspace without requiring the persistent daemon and filesystem watcher planned for Milestone 6. It may optionally be wired to a `post-commit` git hook by the user; Omni-Sem must not install hooks automatically.

## 22.4 `omnisem query`

Suggested usage:

```bash
omnisem query "storage architecture"
omnisem query "storage architecture" --mode lexical
omnisem query "storage architecture" --limit 5
omnisem query "storage architecture" --token-budget 2000
omnisem query "storage architecture" --budget claude-code
omnisem query "storage architecture" --explain
omnisem query "storage architecture" --json
```

`--budget NAME` resolves a configured preset (Section 18.9) to its underlying `token_budget` and `max_results` values; it is equivalent to setting both explicitly and does not add new retrieval behavior.

`--explain` includes the match-explanation data (Section 18.4a) in human-readable output; it is otherwise always present in JSON output.

Human-readable output should show:

* path;
* anchor;
* score;
* excerpt;
* revision identifier;
* freshness status (Section 18.8a).

JSON output should use the normalized retrieval schema, including `explanation` and `freshness`.

## 22.5 `omnisem doctor`

Checks should include:

* configuration validity;
* database availability;
* schema version;
* FTS5 availability;
* root accessibility;
* parser availability;
* embedding-provider status;
* stale locks;
* incomplete jobs;
* MCP support;
* current executable version.

## 22.7 `omnisem changes`

Surfaces revision history that the domain model already records; this command adds no new tracked state.

```bash
omnisem changes
omnisem changes --since 7d
omnisem changes --root ROOT_ID
omnisem changes --json
```

Responsibilities:

* list source files with a new current revision since the requested window;
* show previous and current content hash, and observed/indexed timestamps;
* distinguish additions, modifications, and deletions using existing `SourceState` and `Revision` records;
* never display full document text — this is a change summary, not a diff viewer, consistent with the read-only and evidence-before-inference principles.

## 22.8 `omnisem eval`

Promotes the retrieval-evaluation harness (Section 26) from an internal development tool to a stable, user-facing command, so a user or downstream integrator can measure retrieval quality on their own corpus rather than trusting an unverifiable claim.

```bash
omnisem eval
omnisem eval --corpus PATH
omnisem eval --mode lexical
omnisem eval --json
```

Responsibilities:

* run the configured query/relevance-judgment set (or a user-supplied corpus in the same schema) against the current index;
* report standard retrieval metrics (for example, recall@k, mean reciprocal rank) per the schema defined in Section 26;
* never fabricate a metric when the judgment set is empty or incompatible — fail clearly instead;
* remain deterministic for a fixed index state and fixed corpus, so results are reproducible and comparable across runs.

## 22.9 `omnisem snapshot export` / `omnisem snapshot import`

Enables sharing a built index within a small team without introducing a server, a sync protocol, or any change to the local-first principle — the snapshot is a file, not a service.

```bash
omnisem snapshot export ./omnisem-snapshot.tar
omnisem snapshot import ./omnisem-snapshot.tar
```

Responsibilities:

* export must package derived data only (schema-versioned database contents, FTS index, optional embeddings) — never raw source-file bytes, keeping the export bounded and consistent with "derived data is disposable";
* import must validate schema and embedding-model compatibility before merging, and must refuse to silently merge incompatible embedding spaces;
* imported snapshots are read-only relative to roots the importing user has not themselves approved — an imported snapshot must not grant retrieval access to source content the local `Root` configuration does not already permit, preserving the security boundary in Section 21;
* this feature explicitly does not implement multi-device sync, conflict resolution, or automatic replication, which remain excluded per Section 7.3.

## 22.10 Exit codes

Define stable application exit-code categories.

At minimum distinguish:

* success;
* invalid user input;
* configuration failure;
* filesystem failure;
* database failure;
* partial indexing failure;
* protocol failure;
* internal error.

Document the mapping.

---

# 23. Concurrency and Async Policy

Do not make the entire application asynchronous by default.

Use async where it provides concrete value:

* MCP transport;
* network embedding providers;
* daemon IPC;
* background scheduling;
* file-watcher event handling;
* Streamable HTTP.

Use synchronous code where appropriate:

* SQLite transactions through `rusqlite`;
* Markdown parsing;
* hashing;
* ranking;
* context packing;
* domain validation.

Blocking work must not run directly on Tokio executor threads when it can cause latency or starvation.

Use:

* dedicated blocking tasks;
* bounded worker pools;
* explicit queues;
* backpressure.

Avoid unbounded task spawning.

---

# 24. Observability

Use structured tracing from the beginning.

Suggested event categories:

* scan started;
* scan completed;
* document discovered;
* document excluded;
* revision prepared;
* revision committed;
* document unchanged;
* parse failed;
* FTS update completed;
* embedding job started;
* embedding job completed;
* embedding job failed;
* query started;
* query completed;
* MCP request handled;
* database migration applied.

Relevant fields may include:

* root ID;
* source ID;
* revision ID;
* parser ID;
* duration;
* segment count;
* result count;
* retrieval mode;
* token estimate;
* error code.

Do not log:

* full source text;
* full embedding vectors;
* secret configuration values;
* authentication credentials.

Later metrics may include:

* indexed file count;
* active segment count;
* pending jobs;
* failed jobs;
* query p50 and p95 latency;
* indexing throughput;
* parser failure rate;
* embedding cache hit rate;
* database size;
* watcher reconciliation count.

---

# 25. Performance Targets

Performance targets must be measured under documented conditions.

Initial targets:

* idle CPU before daemon introduction: effectively zero;
* daemon idle CPU after watcher introduction: under 2% on reference hardware;
* warm lexical retrieval p95: under 500 milliseconds on the reference corpus;
* warm hybrid daemon retrieval p95: under 750 milliseconds, excluding cold model startup;
* unchanged files: no reparsing;
* incremental save: only affected records are updated;
* no unbounded memory growth during repeated indexing.

Memory use must be measured before setting a strict alpha-release threshold.

Cold embedding-model startup must be reported separately.

Do not obscure model initialization inside ordinary query latency.

---

# 26. Evaluation Strategy

Retrieval quality must be evaluated before advanced architecture is justified.

## 26.1 Initial corpus

Create a versioned test corpus containing approximately 50 Markdown files.

Include:

* overlapping project names;
* duplicated passages;
* old and new decisions;
* contradictory notes;
* aliases;
* Markdown links;
* similar but irrelevant terminology;
* long documents;
* short documents;
* copied files;
* code fences;
* deeply nested headings;
* deliberately stale revisions.

## 26.2 Query judgments

For each evaluation query, label:

* required supporting segments;
* acceptable supporting segments;
* misleading segments;
* expected source diversity;
* whether relationship traversal should help;
* whether the newest revision matters.

## 26.3 Retrieval metrics

Track:

* Recall@5;
* Recall@10;
* Mean Reciprocal Rank;
* nDCG;
* duplicate-result rate;
* stale-revision rate;
* misleading-result rate;
* source diversity;
* returned token count;
* p50 latency;
* p95 latency.

## 26.4 Required comparisons

Compare:

1. lexical only;
2. vector only;
3. lexical and vector fusion;
4. fusion with deterministic structural expansion;
5. fusion with extracted relationships;
6. optional reranking.

Do not assume semantic retrieval automatically outperforms FTS.

## 26.5 Agent-level evaluation

Using a fixed answer model, measure:

* whether the answer is supported;
* whether citations identify the correct source;
* whether key evidence was omitted;
* whether contradictions were acknowledged;
* number of retrieval calls;
* context tokens consumed.

Omni-Sem retrieval must be evaluated independently from answer-model quality.

## 26.5a User-facing evaluation

The corpus, judgment, and metrics machinery described above must be reachable through `omnisem eval` (Section 22.8), not only through an internal `evals/` harness run by contributors. A user should be able to point Omni-Sem at their own small labeled corpus and get the same metric set contributors use to validate changes. This turns an internal QA process into evidence a user or downstream integrator can inspect directly, rather than an unverifiable claim in documentation, and gives the project a concrete, reproducible number to publish when comparing retrieval modes or reporting on changes over time.

## 26.6 Evaluation implementation

Core benchmark execution should preferably be implemented in Rust so it can exercise production retrieval code directly.

Optional Python tools may:

* analyze emitted JSONL;
* generate plots;
* compare experiments;
* inspect relevance judgments.

---

# 27. Testing Requirements

## 27.1 Unit tests

Cover:

* path canonicalization;
* root containment;
* include and exclude patterns;
* content hashing;
* Markdown segmentation;
* stable anchors;
* revision identity;
* unchanged-file detection;
* token budgeting;
* duplicate suppression;
* rank fusion;
* response serialization;
* domain validation.

## 27.2 Integration tests

Cover:

* initialize a temporary installation;
* add a root;
* index a fixture corpus;
* modify a file;
* create a new revision;
* delete a file;
* prevent stale results;
* rebuild the index;
* recover after parser failure;
* run an MCP query against a temporary database.

## 27.3 Security tests

Cover:

* `../` traversal;
* symlink root escape;
* device and special files;
* excluded secret filenames;
* oversized files;
* malformed MCP inputs;
* arbitrary path requests;
* source text containing prompt-injection instructions.

## 27.4 Migration tests

Every database schema migration must be tested against:

* a new database;
* the previous supported schema;
* failure during migration;
* incompatible future schema detection.

## 27.5 Property-based tests

Use property testing where valuable, particularly for:

* path containment;
* include and exclude matching;
* token-budget guarantees;
* serialization round trips;
* revision identity;
* ranking invariants.

## 27.6 Snapshot tests

Use snapshot tests only where stable structured output benefits from review.

Appropriate examples:

* parsed Markdown fixtures;
* CLI human-readable output;
* normalized JSON response shape.

Snapshots must not replace behavioral assertions.

## 27.7 Concurrency tests

When daemon work begins, test:

* simultaneous queries;
* indexing during reads;
* duplicate watcher events;
* bounded job queues;
* shutdown during active work;
* restart recovery;
* daemon and bridge protocol compatibility.

---

# 28. Error Handling

Errors should use stable categories.

Suggested error codes:

```text
CONFIG_INVALID
ROOT_NOT_FOUND
ROOT_NOT_DIRECTORY
ROOT_ESCAPE
FILE_EXCLUDED
FILE_TOO_LARGE
FILE_UNSUPPORTED
FILE_READ_FAILED
FILE_CHANGED_DURING_READ
PARSE_FAILED
DATABASE_ERROR
DATABASE_BUSY
SCHEMA_MISMATCH
MIGRATION_FAILED
FTS_UNAVAILABLE
EMBEDDING_UNAVAILABLE
EMBEDDING_FAILED
QUERY_INVALID
RESOURCE_NOT_FOUND
TOKEN_BUDGET_INVALID
MCP_PROTOCOL_ERROR
IPC_AUTH_FAILED
INTERNAL_ERROR
```

Use typed domain errors through `thiserror`.

Use `anyhow` only at application boundaries where additional context is valuable and the caller does not need structured matching.

User-facing errors must:

* explain what failed;
* explain whether processing continued;
* identify the affected path only when safe;
* suggest corrective action;
* avoid stack traces unless debug mode is enabled.

Internal errors should retain source chains.

---

# 29. Dependency Policy

Prefer:

* well-maintained crates;
* explicit version constraints;
* limited runtime dependencies;
* broadly supported native builds;
* replaceable adapters for infrastructure choices;
* audited unsafe code boundaries;
* optional features for heavy dependencies.

Before adding a major dependency, document:

* purpose;
* alternatives considered;
* binary-size impact;
* compile-time impact;
* platform support;
* license;
* transitive dependency risk;
* whether it introduces native libraries.

Do not add:

* an ORM;
* an HTTP framework;
* a graph database;
* an embedded model runtime;
* a parser framework;

until the relevant milestone requires it.

Run dependency security and license checks in CI or release workflows once the repository foundation exists.

---

# 30. Repository Structure

Recommended initial structure:

```text
omni-sem/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── LICENSE
├── SECURITY.md
├── CONTRIBUTING.md
├── CHANGELOG.md
├── rustfmt.toml
├── clippy.toml
├── crates/
│   ├── omnisem-core/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── config/
│   │       ├── domain/
│   │       ├── discovery/
│   │       ├── parsing/
│   │       ├── storage/
│   │       ├── retrieval/
│   │       ├── mcp/
│   │       └── observability/
│   └── omnisem-cli/
│       ├── Cargo.toml
│       └── src/
│           └── main.rs
├── docs/
│   ├── vision.md
│   ├── architecture.md
│   ├── threat-model.md
│   ├── retrieval-evaluation.md
│   ├── development.md
│   └── adr/
│       ├── 0001-rust-first.md
│       ├── 0002-sqlite-system-of-record.md
│       ├── 0003-source-revisions-are-immutable.md
│       ├── 0004-graph-is-derived.md
│       ├── 0005-read-only-mcp.md
│       └── 0006-daemon-and-mcp-lifecycles.md
├── tests/
│   ├── fixtures/
│   ├── integration/
│   ├── security/
│   └── mcp/
├── evals/
│   ├── corpus/
│   ├── queries.jsonl
│   ├── judgments.jsonl
│   └── reports/
├── tools/
│   └── evaluation-python/
└── scripts/
    ├── check.sh
    ├── benchmark.sh
    └── build-eval-index.sh
```

Avoid creating empty modules for future architecture.

The repository should grow toward this structure as responsibilities become real.

---

# 31. Architecture Decision Records

Create the following initial ADRs.

## ADR-0001: Rust-first runtime

Decision:

Use Rust for all production runtime components.

Python may be used only for optional offline research and analysis tooling.

Consequences:

* native executable distribution;
* predictable resource use;
* strong daemon and concurrency foundation;
* more deliberate implementation;
* some ML experiments may require external tools or providers.

## ADR-0002: SQLite system of record

Decision:

Use SQLite for canonical metadata, revisions, segments, FTS, and job records.

Consequences:

* simple installation;
* inspectable state;
* transactional updates;
* no external database service;
* vector search remains behind an interface.

## ADR-0003: Immutable source revisions

Decision:

Represent each observed content state as an immutable revision.

Consequences:

* reproducibility;
* stale-result prevention;
* transactional promotion;
* easier parser migration reasoning;
* future storage-retention policies may be necessary.

## ADR-0004: Graph is derived

Decision:

Do not use a graph as the primary source of truth.

Consequences:

* original evidence remains canonical;
* graph experiments are reversible;
* inferred relationships cannot silently rewrite source meaning.

## ADR-0005: Read-only MCP

Decision:

Expose search and retrieval but no source-file mutation.

Consequences:

* smaller security boundary;
* easier client trust;
* autonomous editing remains another system’s responsibility.

## ADR-0006: Separate daemon and MCP lifecycles

Decision:

Once continuous indexing exists, use a persistent daemon and lightweight MCP bridge.

Consequences:

* one shared model and database owner;
* MCP-client shutdown does not stop indexing;
* local IPC and authentication become necessary.

## ADR-0007: Git-aware incremental indexing precedes the daemon

Decision:

Ship `omnisem index --since` (git-diff-based) as an interim freshness mechanism before building the persistent daemon and filesystem watcher.

Consequences:

* most target users already work inside git-managed workspaces, so this covers a large share of the freshness gap at a fraction of the engineering cost of a watcher;
* the daemon and watcher (Milestone 6) remain necessary for non-git roots and true real-time freshness, and this decision must not be used to justify indefinitely deferring them;
* freshness status (18.8a) makes the remaining gap visible instead of silently serving stale results.

## ADR-0008: Provenance data is surfaced, not just stored

Decision:

Expose match explanation (18.4a) and freshness status (18.8a) directly in retrieval output, rather than treating `RetrievalSignals` and revision timestamps as internal-only data.

Consequences:

* no new subsystem is required — both features are read-only projections of data the retrieval and ingestion pipelines already compute;
* users and downstream agents can evaluate a result's trustworthiness without a separate lookup;
* CLI and JSON output schemas grow slightly and must be documented as part of the stable retrieval response contract.

## ADR-0009: Interoperate with existing personal-knowledge structures

Decision:

Respect `.gitignore` and existing workspace conventions during discovery (Section 5.12, 17.1) rather than requiring a parallel Omni-Sem-specific configuration for exclusion.

Consequences:

* lower setup friction and lower switching cost for a new user;
* discovery logic gains a dependency on already-selected tooling (the `ignore` crate) rather than a new one;
* Omni-Sem-specific excludes remain available and take precedence where they conflict.

## ADR-0010: Sensitivity tags gate visibility, not indexing

Decision:

Introduce a `SensitivityTag` (13.1a) that controls whether a segment may be returned to an MCP client, separate from whether it is indexed at all.

Consequences:

* users get finer-grained control over what is exposed to an external agent without having to exclude sensitive files from local search entirely;
* this adds a filtering check in the retrieval and MCP-response path that must be tested explicitly, including for the `NeverReturnToMcp` case, as part of Milestone 5's acceptance criteria;
* it does not weaken the read-only or local-first guarantees — it only narrows what is already-approved content is allowed to surface through the MCP boundary.

---

# 32. Milestones

## Milestone 0: Project foundation and evaluation contract

Deliverables:

* Cargo workspace initialized;
* license selected;
* core library crate;
* CLI crate;
* CLI entry point;
* formatting, linting, testing, and documentation checks;
* CI workflow;
* architecture documentation;
* initial ADRs;
* SQLite schema proposal;
* evaluation-corpus structure;
* query-judgment schema;
* `scripts/check.sh`.

Acceptance criteria:

* a clean checkout can build the project;
* one command runs all checks;
* the CLI prints version and help;
* CI passes;
* architecture decisions are documented;
* no speculative graph or daemon implementation exists.

## Milestone 1: Deterministic Markdown indexing

Deliverables:

* configuration loading;
* root management;
* `omnisem root suggest` (22.1a);
* filesystem discovery, honoring existing `.gitignore` by default (17.1);
* include, exclude, and sensitivity-tag rules (13.1a);
* content hashing;
* SQLite schema and migrations;
* immutable revisions;
* Markdown parser;
* plain-text and unparsed source discovery (17.4a);
* stable segment anchors;
* FTS5 indexing;
* `omnisem index`;
* `omnisem status`;
* `omnisem changes` (22.7).

Acceptance criteria:

* a configured Markdown directory is indexed;
* unchanged files are not reparsed;
* modified files create new revisions;
* deleted files no longer appear as active;
* FTS contains only active segments;
* parser failure in one file does not corrupt the index;
* current-revision promotion is transactional;
* `omnisem changes` correctly reports additions, modifications, and deletions since a given window using existing revision records;
* all behaviors are tested.

## Milestone 2: Lexical retrieval and context packing

Deliverables:

* lexical retrieval service;
* metadata filters;
* normalized result model, including match explanation (18.4a) and freshness status (18.8a);
* duplicate suppression;
* token-budget context packer;
* named context-budget presets (18.9);
* JSON and human-readable CLI output;
* `omnisem query`, including `--budget` and `--explain`;
* `omnisem eval` as a stable, user-facing command (22.8), built on the initial evaluation runner.

Acceptance criteria:

* queries return source path, revision, anchor, text, score, match explanation, and freshness status;
* results obey the token budget, including named presets;
* stale revisions are excluded, and results whose source has since changed are flagged `pending_reindex` rather than silently served as current;
* duplicates are suppressed;
* `omnisem eval` produces the same metrics against a user-supplied corpus as the internal evaluation harness produces against the reference corpus.

## Milestone 3: Git-aware incremental indexing and snapshots

Deliverables:

* `omnisem index --since` (22.3a), including git-diff-based change detection for git-managed roots;
* `omnisem snapshot export` / `omnisem snapshot import` (22.9);
* `omnisem status --serve` (22.1b), the optional local read-only status view.

Acceptance criteria:

* `--since` correctly limits discovery to git-changed files and falls back cleanly to full discovery outside a git repository;
* a snapshot exported from one machine imports cleanly on another with a compatible schema and refuses incompatible embedding spaces rather than silently merging them;
* an imported snapshot never exposes retrieval results for content outside the importing user's own approved roots;
* `--serve` remains read-only, localhost-only, and off by default.

This milestone exists explicitly as a low-cost interim freshness mechanism — see Section 5.11 and Section 7.2 — so users are not stuck with fully manual re-indexing while the full daemon and watcher (Milestone 6) remain unbuilt. It must not be treated as a reason to delay Milestone 6 indefinitely; it is a bridge, not a replacement.

## Milestone 4: Optional semantic retrieval

Deliverables:

* embedding-provider trait;
* disabled provider;
* Ollama embedding provider;
* embedding cache;
* exact vector-search baseline;
* hybrid rank fusion;
* semantic evaluation report;
* optional in-process provider investigation.

Acceptance criteria:

* lexical mode works without embedding dependencies;
* model changes do not mix vectors;
* hybrid retrieval can be compared objectively with lexical retrieval, using `omnisem eval`;
* semantic retrieval remains optional;
* no network request occurs without explicit configuration.

## Milestone 5: MCP proof of product

Deliverables:

* STDIO MCP server;
* `search_context`;
* `get_context`;
* `index_status`;
* resource URI resolver;
* MCP protocol tests;
* client configuration documentation;
* protocol-inspector validation.

Acceptance criteria:

* a supported MCP client can launch Omni-Sem;
* the client can search indexed Markdown;
* returned content contains provenance, match explanation, and freshness status;
* segments tagged `NeverReturnToMcp` (13.1a) are never returned through this interface;
* arbitrary filesystem paths are rejected;
* source content is labeled as untrusted evidence;
* logs do not pollute protocol output.

## Milestone 6: Incremental daemon

Deliverables:

* `omnisemd`;
* watcher;
* event coalescing;
* reconciliation scans;
* durable job state;
* bounded worker scheduling;
* retry policy;
* authenticated local IPC;
* `omnisem-mcp` bridge;
* restart recovery;
* daemon health reporting.

Acceptance criteria:

* saves, renames, and deletions update the index correctly;
* rapid save events do not create uncontrolled work;
* daemon restart does not corrupt state;
* multiple MCP clients do not load duplicate model instances;
* blocking work does not starve the async runtime;
* bridge requests cannot access arbitrary files;
* freshness status (18.8a) reports `current` in normal operation rather than routinely showing `pending_reindex`, confirming the daemon supersedes the Milestone 3 interim mechanism in practice.

## Milestone 7: Structured file adapters

Deliverables:

* PDF adapter;
* Python Tree-sitter adapter;
* TypeScript Tree-sitter adapter;
* stable format-specific anchors;
* deterministic links and code relationships;
* parser capability reporting.

Acceptance criteria:

* PDF results retain page provenance;
* source-code results retain symbol provenance;
* parser failures remain isolated;
* code relationships are deterministic and inspectable;
* optional parser dependencies remain feature-gated.

## Milestone 8: Evidence graph experiment

Deliverables:

* mention storage;
* entity candidates;
* deterministic relationships;
* optional NLP extraction;
* optional model-assisted relation proposals;
* bounded `find_connections`;
* graph-retrieval ablation report.

Acceptance criteria:

* every relationship has evidence and origin;
* inferred relationships are distinguishable from deterministic ones;
* graph traversal is bounded;
* graph-enhanced retrieval provides measurable value or remains experimental.

---

# 33. Initial Release Boundary

The first open-source alpha should contain:

* Rust-native runtime;
* Markdown indexing;
* plain-text and unparsed source discovery (17.4a);
* `.gitignore`-aware discovery (17.1);
* sensitivity-tag exclusion (13.1a);
* `omnisem root suggest` (22.1a);
* SQLite storage;
* lexical retrieval;
* match explanation and freshness status on every result (18.4a, 18.8a);
* named context-budget presets (18.9);
* `omnisem changes` (22.7);
* `omnisem eval` as a stable user-facing command (22.8);
* git-aware incremental indexing via `--since` (22.3a);
* optional local embeddings;
* hybrid retrieval;
* CLI;
* read-only STDIO MCP;
* source provenance;
* documented security boundaries;
* evaluation results;
* Linux support.

The first alpha does not require:

* persistent daemon or filesystem watcher (the `--since` git-aware mechanism covers freshness in the interim);
* index snapshot export/import (useful but not required for a single-user alpha);
* the optional local status view (`status --serve`);
* PDF support;
* code parsing;
* graph traversal;
* a general web interface or dashboard;
* Windows service integration;
* macOS launch agent;
* cloud model providers;
* mandatory Python tooling.

None of the alpha-scope additions above introduce a new runtime subsystem: they read data the core pipeline (revisions, timestamps, FTS match data, root configuration) already produces. This keeps the alpha aligned with Section 5.11 (measured complexity) while closing the largest usefulness gaps identified against comparable existing tools.

---

# 34. First Development Work Order

Begin with Milestone 0 only.

Do not implement the complete ingestion pipeline during the first pass.

## Task 1: Initialize the Cargo workspace

Create:

* workspace `Cargo.toml`;
* `crates/omnisem-core`;
* `crates/omnisem-cli`;
* `tests`;
* `docs`;
* `docs/adr`;
* `evals`;
* `tools`;
* `scripts/check.sh`;
* README;
* license;
* security policy;
* contribution guide.

Configure:

* Rust edition;
* package metadata;
* CLI entry point;
* formatting;
* Clippy;
* tests;
* documentation checks;
* CI;
* optional dependency auditing.

## Task 2: Establish domain contracts

Create typed domain models for:

* root;
* discovered document;
* source file;
* revision;
* segment;
* retrieval query;
* retrieval hit;
* retrieval response.

Use newtypes for identifiers.

Do not bind domain models directly to SQLite row representations.

## Task 3: Draft the database schema

Create:

* schema design document;
* initial migration mechanism;
* schema metadata table;
* initial tables for roots, source files, revisions, and segments.

FTS may be scaffolded, but complete retrieval belongs to the next milestone.

## Task 4: Create parser interfaces

Create:

* `DocumentParser` trait;
* parsed-document model;
* parsed-segment model;
* parser registry or explicit parser selection mechanism;
* one minimal Markdown fixture;
* parser contract tests.

Do not add NLP or embedding dependencies.

## Task 5: Create the evaluation contract

Define JSONL schemas for:

* corpus metadata;
* queries;
* relevance judgments;
* benchmark results.

Add a small starter fixture set rather than attempting the full 50-file corpus immediately.

## Task 6: Add project validation

`scripts/check.sh` must run at least:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
```

Include any required package or security checks that are stable enough for the project foundation.

The script must stop and return a non-zero exit code when any required check fails.

## Task 7: Document decisions

Write:

* `docs/vision.md`;
* `docs/architecture.md`;
* `docs/threat-model.md`;
* `docs/retrieval-evaluation.md`;
* `docs/development.md`.

Write the six initial ADRs.

## Task 8: Keep the implementation narrow

At the end of Milestone 0, the repository should contain:

* a working CLI shell;
* domain contracts;
* schema proposal;
* parser contracts;
* evaluation contracts;
* documentation;
* tests;
* CI.

It should not yet contain a partially working daemon, graph, embedding system, or watcher.

---

# 35. First-Pass Non-Goals

During the initial work order, do not add:

* filesystem watcher;
* long-running daemon;
* local IPC;
* MCP transport implementation;
* embeddings;
* ONNX Runtime;
* spaCy;
* NLTK;
* Tree-sitter;
* PDF parsing;
* graph database;
* entity extraction;
* LLM calls;
* web UI;
* Docker;
* cloud services;
* mandatory Python components;
* more crates than the initial architecture requires.

Interfaces may anticipate later features, but empty abstraction layers must not dominate the codebase.

---

# 36. Required Agent Completion Report

At the end of the first implementation pass, report the following.

## Summary

Describe what was created and why.

## Repository state

Provide:

* current branch;
* commit hash;
* changed files;
* untracked files;
* working-tree cleanliness.

## Architecture decisions

List:

* ADRs added;
* dependencies selected;
* any deviations from this blueprint;
* the reasoning for each deviation.

## Validation

Provide the exact commands run and their exit codes.

Minimum required validation:

```bash
./scripts/check.sh
```

Do not report only “tests passed.” Include the actual command and result.

## Tests

Report:

* number of tests;
* passed tests;
* failed tests;
* ignored tests;
* documentation tests;
* relevant coverage results if configured.

## Build information

Report:

* Rust toolchain;
* Rust edition;
* target platform;
* release and debug build status.

## Open issues

Identify:

* unfinished work;
* risks;
* assumptions;
* deferred decisions;
* dependency concerns.

## Next recommended task

Recommend the smallest coherent next unit of work within Milestone 1.

Do not state that Milestone 0 is complete unless all acceptance criteria are satisfied.

---

# 37. Decision Rules for Future Agents

When uncertain, use these priorities in order:

1. Preserve user data and privacy.
2. Preserve source provenance.
3. Keep source files immutable.
4. Prefer deterministic behavior.
5. Maintain a lexical fallback.
6. Make derived data rebuildable.
7. Keep infrastructure dependencies replaceable.
8. Keep the runtime native and self-contained.
9. Measure retrieval quality before adding intelligence.
10. Avoid expanding product scope.
11. Prefer bounded queues and explicit backpressure.
12. Prefer the smallest testable implementation.
13. Do not optimize before measuring.
14. Do not create a crate boundary without a demonstrated need.
15. Do not introduce Python into the runtime path without an approved architectural revision.

---

# 38. Final Project Statement

Omni-Sem will not attempt to “understand an entire hard drive” through an automatically fabricated knowledge graph.

It will build a trustworthy semantic access layer over explicitly approved local workspaces.

Its central promise is:

> Fast, private, source-grounded local context for any AI agent.

The production core will be implemented in Rust and distributed as a native application.

The project begins with:

```text
Markdown files
    → immutable revisions
    → structure-aware segments
    → SQLite FTS5
    → token-bounded retrieval
    → read-only MCP
```

Semantic vectors, continuous watching, code and PDF parsing, entity resolution, and graph traversal will be added only when each layer demonstrates measurable value.
> Implementation note (Milestone 4B): semantic and hybrid retrieval use transient query embeddings, exact bounded active-vector search, and one-pass RRF. Snapshot format 1 remains lexical-only.
