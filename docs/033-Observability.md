# 033 — Observability

## Vision

See what Core is doing without instrumenting each request by hand — structured logging
today, with named gaps toward the founding brief's original metrics/crash-reporting
vision.

## Goals

- **Structured logging**: `tracing`/`tracing-subscriber` throughout Core, `info!`/`warn!`/
  `debug!` at meaningful decision points (worktree creation, indexing completion, sandbox
  boundary probes, model provider dispatch). Configurable via `RUST_LOG`/`EnvFilter`
  (`main.rs`).
- **`/health` endpoint**: reachability, version, connected client count, whether auth is
  required — unauthenticated by design so a client can always check Core's basic state
  (`api/router.rs::health_handler`).
- **Action History** (`013-Repository-Health.md`): every tool call, terminal command, and
  approval decision is queryable per-Mission — the closest thing CID has to an audit/
  observability trail today.

## Non-Goals

Prometheus metrics export or a dedicated crash-reporting service (Sentry) — both named in
the original founding-brief vision (`cid_project_blueprint.md`'s Performance Targets &
Monitoring section) but not built in Phases 0–5. Named here explicitly as unbuilt.

## Architecture

`tracing` calls are scattered per-module (no centralized metrics aggregator); `/health`
is the one deliberately-designed observability endpoint; History is domain data
repurposed as an audit trail rather than a separate observability subsystem.

## Data Structures

`ToolCall` (`api/types.rs`) is the closest thing to a structured observability event
CID persists.

## Traits / Interfaces

`GET /health` (unauthenticated); `message.list` (History data).

## Storage Layout

Log output goes to stdout/stderr (not persisted); `/health` state is computed live, not
stored; History is persisted in the `messages` table.

## Performance Targets

N/A.

## Tradeoffs

No metrics aggregation means diagnosing a production performance issue currently requires
reading logs and running the benchmark suite manually — acceptable at CID's current
single-user/small-team deployment scale, a real limitation at larger scale.

## Failure Modes

A crash in Core is not automatically reported anywhere — a user would need to notice the
process died and check logs themselves. No automatic restart or crash telemetry exists.

## Security

`/health` is deliberately unauthenticated but minimal — it reveals reachability, version,
and whether auth is required, nothing that isn't already implied by the port being open
(ADR 0012's explicit design reasoning).

## Testing

`health_endpoint_reports_ok`, `health_stays_reachable_on_a_protected_core` verify the
one deliberately-designed observability surface.

## Implementation Order

Structured logging (Phase 0, ongoing) → `/health` with real fields (Phase 2/3, alongside
access control). Metrics export and crash reporting remain unbuilt.

## Acceptance Criteria

`/health` accurately reflects Core's real state (auth requirement, client count) — not
hardcoded placeholder values.

## AI Coding Rules

If you build Prometheus metrics export or crash reporting, update this document's Goals/
Non-Goals to reflect it — currently named as a real, honest gap against the original
vision.
