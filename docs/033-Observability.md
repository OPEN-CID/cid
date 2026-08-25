# 033 — Observability

## Vision

See what Core is doing without instrumenting each request by hand — structured logging,
a self-hosted Prometheus metrics endpoint, and a self-hosted Sentry-style crash log, all
local-first (Part 15): no external SaaS account required to observe a self-hosted Core.

**Corrected 2026-08 (production-readiness review)**: this doc previously listed
Prometheus export and crash reporting under Non-Goals as "not built in Phases 0–5." That
was true when originally written but went stale — both were built in a later pass
(`cid-core/src/observability/mod.rs`) and never reconciled back into this file, exactly
the class of doc/code drift this project's own culture (`CLAUDE.md`) exists to catch.
Corrected here from the real current code, not from the prior draft of this doc.

## Goals

- **Structured logging**: `tracing`/`tracing-subscriber` throughout Core, `info!`/`warn!`/
  `debug!` at meaningful decision points (worktree creation, indexing completion, sandbox
  boundary probes, model provider dispatch). Configurable via `RUST_LOG`/`EnvFilter`
  (`main.rs`).
- **`/health` endpoint**: reachability, version, connected client count, whether auth is
  required — unauthenticated by design so a client can always check Core's basic state
  (`api/router.rs::health_handler`).
- **`/metrics` endpoint**: real Prometheus text-exposition-format (0.0.4) output —
  `Metrics::render_prometheus` (`observability/mod.rs`) tracks counters (total RPC calls,
  per-method breakdown via `cid_rpc_requests_by_method_total{method="..."}`) and gauges,
  incremented on every real RPC dispatch. No external Prometheus server is bundled —
  CID exposes the endpoint; scraping it is the operator's responsibility (see
  `docs/052-Production-Deployment.md` for a scrape-config example).
- **Local crash log**: `install_panic_hook` (called once from `main.rs`) replaces Rust's
  default panic hook with one that also captures a redacted `CrashReport` (panic message
  after `redact::redact_secrets`, source `file:line:col`, thread name — structurally no
  field that could hold file contents, see the type's own doc comment) into an in-memory
  ring buffer (last 200) and, if a log path was configured, appends it as a JSON line to
  disk so reports survive a restart. Queryable via `observability.crashes.list`.
- **Action History** (`013-Repository-Health.md`): every tool call, terminal command, and
  approval decision is queryable per-Session — a complementary, domain-level audit trail
  alongside the crash log and metrics.

## Non-Goals

No *external* telemetry integration (no bundled Prometheus server, no Sentry/Datadog
SDK, no dashboards) — CID exposes the data locally; wiring it to an external observability
stack is the operator's choice, not CID's. No automatic process restart on crash.

## Architecture

`tracing` calls are scattered per-module (no centralized log aggregator — logs go to
stdout/stderr, collection is the operator's job, see `docs/052-Production-Deployment.md`).
`Metrics` and `CrashLog` (`observability/mod.rs`) are each a single shared struct on
`AppState`, updated synchronously from the request-handling path (`router.rs` increments
metrics per RPC call; `install_panic_hook` runs process-wide via `std::panic::set_hook`).
`/health` and `/metrics` are both deliberately-designed HTTP endpoints; History remains
domain data repurposed as an audit trail rather than a separate observability subsystem.

## Data Structures

`CrashReport`, `CrashLog`, `Metrics` (`observability/mod.rs`); `ToolCall` (`api/types.rs`)
is the closest structured observability event on the History side.

## Traits / Interfaces

`GET /health` (unauthenticated); `GET /metrics` (Prometheus text format);
`observability.crashes.list` (RPC); `message.list` (History data).

## Storage Layout

Log output goes to stdout/stderr (not persisted by Core itself — see the production doc
for log-rotation guidance). `/metrics` state lives in-process only (an `AppState`-held
`Metrics`, reset on restart — there is no time-series storage; a real Prometheus server
scraping the endpoint is what would retain history). Crash reports live in an in-memory
ring buffer (last 200) plus, if a log path is configured, an append-only JSON-lines file
on disk that does persist across restarts. `/health` state is computed live, not stored.
History is persisted in the `messages` table.

## Performance Targets

N/A.

## Tradeoffs

Metrics reset on every Core restart (no persistence, no historical query) — acceptable
because a real Prometheus server is expected to be the thing retaining history via
scraping, not Core itself. Crash reports persist to disk but with no size cap on the log
file itself (only the in-memory list is capped at 200) — a very crash-heavy process could
grow that file unboundedly; not yet a rotation policy, just a gap worth knowing about.

## Failure Modes

A crash is captured (redacted message + location + thread, both in-memory and — if
configured — on disk) but **nothing restarts the process automatically** — an operator
or process supervisor (systemd, a Windows service, a container orchestrator) still needs
to notice the exit and either restart it or alert a human. This doc previously claimed no
crash telemetry existed at all; that was stale — the real gap is auto-restart, not
telemetry.

## Security

`/health` is deliberately unauthenticated but minimal — it reveals reachability, version,
and whether auth is required, nothing that isn't already implied by the port being open
(ADR 0012's explicit design reasoning). `/metrics` is currently unauthenticated on the
same basis; it reveals call-volume-by-method, not payload content — still worth putting
behind the same reverse proxy as everything else on a non-loopback deployment. Crash
reports are redacted before storage (`redact::redact_secrets`) and structurally cannot
contain file contents.

## Testing

`health_endpoint_reports_ok`, `health_stays_reachable_on_a_protected_core`,
`metrics_endpoint_reports_prometheus_text_and_counts_rpc_calls`,
`observability_crashes_list_starts_empty` (`cid-core/tests/api_integration.rs`);
`crash_log_keeps_only_the_most_recent_reports`,
`crash_report_has_no_field_that_could_hold_file_contents` (`observability/mod.rs` unit
tests) verify the real behavior, not just that the endpoints return 200.

## Implementation Order

Structured logging (Phase 0, ongoing) → `/health` with real fields (Phase 2/3, alongside
access control) → `/metrics` + local crash log (Phase 6). External telemetry integration
(Sentry/Datadog/a bundled Prometheus server) remains unbuilt and is not currently planned
— see Non-Goals.

## Acceptance Criteria

`/health` and `/metrics` accurately reflect Core's real state — not hardcoded placeholder
values. A real `panic!` anywhere in Core produces a queryable `CrashReport`.

## AI Coding Rules

This doc was stale for a real, shipped feature (Prometheus export, crash reporting) for
one full pass — a direct instance of the doc/code drift `CLAUDE.md` warns about. Before
editing this file again: grep `observability/mod.rs` and `router.rs`'s `/metrics` route
for the current real behavior rather than trusting this doc's prior wording, and if you
build external telemetry integration or auto-restart, update Goals/Non-Goals/Failure
Modes together in the same change that adds the code.
