# 021 — Storage

## Vision

One storage engine for structured state (SQLite), one for full-text search (Tantivy) —
cut from the founding brief's original SQLite+RocksDB+Tantivy+petgraph stack because no
Phase 0–4 workload actually needed the extra engines (Part 18).

## Goals

- **SQLite** (`cid-core/src/persistence/mod.rs`): every structured domain entity —
  workspaces, repo channels, missions, messages, plans, reviews, skills, MCP servers,
  settings, GitHub/forge/tracker configs, users, sessions, role profiles, confidence
  scores, deployment records. One file, ACID, `rusqlite` with the `bundled` feature (no
  system SQLite dependency).
- **Tantivy** (`cid-core/src/semantic_engine/index.rs`): per-repo full-text index at
  `<repo>/.cid/index`, persisted on disk, survives Core restarts.
- **OS credential storage** (`keyring` crate): API keys and forge/tracker tokens — never
  in SQLite plaintext.

## Non-Goals

RocksDB — no workload in Phases 0–4 is write-heavy enough at the scale that would justify
a second storage engine; cut explicitly from the original stack table, revisit only with
evidence (Part 18). A dedicated vector database — the current embedding set doesn't need
one (see `007-Context-Engine.md`'s Non-Goals).

## Architecture

`Persistence` (`persistence/mod.rs`) is the single point of SQLite access — every manager
that needs structured storage takes an `Arc<Persistence>`, never opens its own connection.
`init_schema` runs `CREATE TABLE IF NOT EXISTS` for every table plus additive `ALTER
TABLE` migrations guarded to ignore already-applied changes (no migration framework — see
Tradeoffs).

## Data Structures

See `035-Database.md` for the full schema.

## Storage Layout

Single SQLite file (default: OS data dir/`cid/cid.db`, or `--db` override); Tantivy index
per connected repo under that repo's own `.cid/index/` (not centralized — a repo's search
index lives with the repo, not with Core's own state).

## Performance Targets

Not independently benchmarked as a storage-layer concern; folded into the
`Core::new_in_memory` construction benchmark (`004-System-Architecture.md`), which
exercises full schema initialization.

## Tradeoffs

**No migration framework** (Diesel or similar) — `init_schema`'s additive `ALTER TABLE`
statements, each wrapped to ignore "column already exists" errors, serve as a de facto
migration mechanism. Works for the additive-only schema changes this project has made so
far; would not handle a column rename or type change gracefully. A real, accepted
limitation for a project at this stage, not a considered-and-rejected alternative.

**One SQLite connection behind a `Mutex`** (`Persistence.conn: Mutex<Connection>`) —
serializes all writes. Acceptable at current concurrency levels (a single desktop user, a
handful of concurrent Missions); would need connection pooling or WAL-mode tuning at
higher concurrency, not yet needed.

## Failure Modes

A corrupted or locked SQLite file surfaces as a `rusqlite` error propagated through
`anyhow::Result` to the RPC caller — no automatic recovery or backup/restore mechanism
exists **in Core itself**. The operational answer (an external, WAL-safe backup script
using `sqlite3 .backup`, plus a restore procedure) lives in
`docs/052-Production-Deployment.md` §5 — this remains a real gap in Core's own
capabilities, just not an undocumented one.

## Security

API keys and forge/tracker tokens go through `keyring`, never SQLite — verified by
`settings_never_return_a_full_api_key` and the `passwords_are_never_stored_in_plain_text`
test for user accounts (which uses Argon2id hashing, a different mechanism for a
different secret class — see `031-Security.md`).

## Testing

Every manager's tests exercise `Persistence::new_in_memory()` — an in-memory SQLite
instance with full schema, used across all 302 `cid-core` unit tests and 56 integration
tests.

## Implementation Order

SQLite persistence (Phase 0) → Tantivy (Phase 2, replacing an in-memory-only word index)
→ no structural change through Phase 4, only additive tables per phase's new domain
entities.

## Acceptance Criteria

Every domain entity described in `005-Domain-Driven-Design.md` has a corresponding table
and is round-trip persisted (write, restart-equivalent in-memory reconstruction, read)
by at least one test.

## AI Coding Rules

New tables go in `persistence::init_schema`'s `CREATE TABLE IF NOT EXISTS` block; new
columns on existing tables go in the `ALTER TABLE` migration list below it, in the
existing ignore-if-exists pattern — don't add a separate migration mechanism without an
ADR.
