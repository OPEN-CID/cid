# 035 — Database

## Vision

The real, current SQLite schema — every table that exists in
`cid-core/src/persistence/mod.rs::init_schema` at time of writing, not an aspirational
one.

## Goals

17 tables, grouped by domain:

| Table | Domain | Added |
|---|---|---|
| `workspaces` | Workspace | Phase 0 |
| `repo_channels` | Repo Channel | Phase 0 |
| `missions` | Mission | Phase 0 |
| `messages` | Chat history | Phase 0 |
| `skills` | Skills library | Phase 0 |
| `mcp_servers` | MCP connector config | Phase 0 |
| `settings` | Provider keys, per-role model config | Phase 0, extended each phase |
| `github_configs` | GitHub bridge | Phase 1 |
| `mission_plans` | Planner output + approval state | Phase 1 (this pass) |
| `mission_reviews` | Reviewer output | Phase 1 (this pass) |
| `forge_configs` | GitLab/Bitbucket bridge | Phase 3 |
| `tracker_links` | Jira/Linear Mission↔ticket links | Phase 3 |
| `users` | Local accounts | Phase 3 |
| `sessions` | Auth session tokens | Phase 3 |
| `role_profiles` | Configurable role profiles | Phase 4 |
| `confidence_scores` | Confidence Engine history | Phase 4 |
| `deployment_records` | Deployment log | Phase 4 |

## Non-Goals

A separate analytics/reporting schema — SQLite here is transactional state, not a data
warehouse.

## Architecture

See `021-Storage.md` for the storage-engine-level decisions this schema sits inside.

## Data Structures

Each table's Rust-side type is documented in its owning subsystem doc (see
`028-Backend.md`'s index) — this document is the schema inventory, not a field-by-field
redefinition.

## Storage Layout

Single SQLite file. Enum-typed columns (`status`, `role`, `scope`, `source`, etc.) are
stored as their `snake_case` serde string representation via `enum_str`/`parse_enum`
helpers in `persistence/mod.rs`, not as SQLite `INTEGER` codes — chosen for
debuggability (a raw `SELECT` shows `"approved"`, not `2`) at a negligible storage cost.

## Performance Targets

Indexes exist on the query patterns actually used:
`idx_missions_repo`, `idx_messages_mission`, `idx_messages_created`,
`idx_reviews_mission`, `idx_sessions_user`, `idx_role_profiles_scope`,
`idx_deployments_mission`, `idx_tracker_links_mission`, `idx_confidence_mission`.

## Tradeoffs

Additive-only migrations (`ALTER TABLE ... ADD COLUMN`, each ignoring "already exists"
errors) rather than a migration framework — see `021-Storage.md`'s Tradeoffs for the full
reasoning.

## Failure Modes

See `021-Storage.md`.

## Security

Password hashes (Argon2id) and no plaintext secrets anywhere in this schema — API keys
and forge/tracker tokens live in OS credential storage via `keyring`, never a column
here. Verified by `passwords_are_never_stored_in_plain_text` and
`settings_never_return_a_full_api_key`.

## Testing

Every table is exercised by `Persistence::new_in_memory()`-backed tests across the
codebase — 302 `cid-core` unit tests and 56 integration tests collectively touch every
table listed above.

## Implementation Order

See the "Added" column above — one column per phase, additive throughout.

## Acceptance Criteria

`init_schema` run against a fresh in-memory SQLite connection succeeds and produces every
table listed above — verified on every test run, since every test uses
`new_in_memory()`.

## AI Coding Rules

New tables go in `init_schema`'s `CREATE TABLE IF NOT EXISTS` block; update this
document's table with the new table name, domain, and phase when you add one — this is
the single source of truth for "what tables actually exist," and it drifting from reality
defeats its purpose.
