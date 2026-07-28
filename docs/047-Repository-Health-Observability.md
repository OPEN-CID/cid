# 047 — Repository Health & Observability (Phase 6)

## Vision

Two Phase 6 deliverables, both self-hosted (Part 15's local-first architecture — no
external SaaS account is required for CID to be observable or to see its own test
health): a **Repository Health** signal over the repo's own test suite, and
**observability** (Prometheus-style metrics, a local Sentry-style crash log).

## Goals

**Repository Health** (`cid-core/src/repo_health/mod.rs`, RPC `repo_health.scan`,
frontend `src/components/health/RepoHealthPanel.tsx`):
- Test-to-code ratio per module (`#[test]` fn count vs. total `fn` count) — a presence
  signal, explicitly not a coverage percentage.
- Duplicate/near-duplicate test detection: normalizes each test body (whitespace and
  comments stripped) and hashes it, flagging tests that assert the same thing under
  different names.
- Source is masked (string literals and comments blanked, preserving byte offsets)
  before pattern-matching, so a `#[test]` sequence inside a string literal — e.g. a test
  fixture that quotes example source, exactly as this module's own tests do — is never
  miscounted as real code. Found via dogfooding the tool against this repo itself during
  Phase 6 (see Failure Modes).

**Observability** (`cid-core/src/observability/mod.rs`, `/metrics` HTTP route, RPC
`observability.crashes.list`):
- `Metrics`: in-process counters/gauges (`cid_rpc_requests_total`,
  `cid_rpc_requests_by_method_total{method=...}`, `cid_rpc_errors_total`,
  `cid_ws_connections_current`), rendered in Prometheus text exposition format at
  `GET /metrics`, unauthenticated like `/health` (it exposes call counts, not RPC
  content).
- `CrashLog`: a panic hook (installed once, in `main.rs`, not in every `Core::new()` —
  tests construct many `Core`s and must not repeatedly stomp the global hook) that
  captures `{timestamp, message, location, thread_name}` per panic, appended to a local
  JSONL file (`<data dir>/cid/crashes.jsonl`) and kept in an in-memory ring buffer
  (last 200), readable via `observability.crashes.list`.

## Non-Goals

- Instrumented line coverage (tarpaulin/llvm-cov) — needs a build step this repo doesn't
  have wired up; named as a real, tracked gap rather than approximated with a plausible
  number (see `RepoHealthPanel.tsx`'s own inline disclosure of this).
- An external crash-reporting SaaS (Sentry, Bugsnag) — "Sentry-style" here means
  structured, queryable crash capture, not a dependency on a specific vendor; nothing
  stops a deployment from also forwarding `crashes.jsonl` to one.
- A full Rust lexer for the string/comment masking pass — it handles `"..."`, `'x'`,
  `//`, and `/* */`, not raw strings (`r"..."`). Named explicitly in the function's own
  doc comment rather than silently claimed as complete.

## Architecture

Both features are read-only signal surfaces over existing state — `repo_health.scan`
walks the filesystem on demand (no persisted index, no background job); `Metrics` is an
in-process `RwLock<HashMap<...>>` of atomics, reset on restart (no cross-restart
persistence — a real Prometheus scrape target handles that via its own storage, which is
the point of the format). `CrashLog` is the one piece with real persistence, since a
crash is exactly the event a purely in-memory store would lose.

## Tradeoffs

Hand-rolled Prometheus text formatting instead of the `metrics`/
`metrics-exporter-prometheus` crates: avoids a new dependency for a genuinely small
surface (four metric names as of Phase 6), consistent with the Phase 5 dependency
audit's "don't churn/add for novelty" instruction. Revisit if the metric surface grows
enough that hand-rolled formatting becomes its own maintenance burden.

## Failure Modes

**Found during this phase's own dogfooding, fixed before shipping**: the first version
of `repo_health.scan`, run against this actual repository (not just its own unit-test
fixtures) in a live browser session, reported a phantom duplicate test — a test fixture
string in `cid-core/tests/api_integration.rs` that *quotes* `"#[test]\nfn ... {...}"` as
example input was being parsed as a second real test. Fixed by masking string/comment
interiors before pattern matching (`mask_non_code`), with a regression test
(`does_not_mistake_test_attributes_inside_string_literals_for_real_tests`) that
reproduces the exact shape of the false positive. This is the clearest evidence in this
phase that "run it for real, not just against hand-built fixtures" catches bugs unit
tests alone don't — the same pattern that caught the Confidence Engine, TestImpactGraph,
and DocGraph bugs in Phase 4.

## Security

`/metrics` and `observability.crashes.list` are unauthenticated/role-unchecked, matching
`/health`'s existing posture — call counts and redacted crash messages, not RPC content
or secrets. `CrashLog` applies `redact::redact_secrets` to every captured panic message
before it is stored or written to disk, verified by
`captured_panic_messages_are_secret_redacted`.

## Testing

- `cid-core/src/repo_health/mod.rs`: 8 unit tests, including the string-literal
  false-positive regression above.
- `cid-core/src/observability/mod.rs`: 4 unit tests (Prometheus rendering, ring-buffer
  eviction, the crash-report field-set structural guarantee, secret redaction on a real
  captured panic).
- `cid-core/tests/api_integration.rs`: 3 integration tests
  (`repo_health_scan_reports_untested_and_duplicate_tests`,
  `observability_crashes_list_starts_empty`,
  `metrics_endpoint_reports_prometheus_text_and_counts_rpc_calls`).
- Both frontend panels were verified in a real browser against a real running Core and
  this actual repository (not mocked), via a Playwright script run manually during
  development — screenshots confirmed real numbers (1191 functions, 269 tests, 16
  untested modules as of this writing) and a real, then-fixed false positive.

## Implementation Order

Repository Health and observability were built together in this phase since both are
small, independent, read-only signal surfaces with no interdependency.

## Acceptance Criteria

- `repo_health.scan` returns real numbers for a real repo, with no false-positive
  duplicates from string literals.
- `/metrics` returns valid Prometheus text exposition format and reflects real RPC call
  counts.
- A panic anywhere in Core produces a `CrashReport` with the secret-redacted message and
  location, retrievable via `observability.crashes.list`, and never contains raw file
  content.

## AI Coding Rules

When adding a new Prometheus metric, add it via `Metrics::inc_counter`/`inc_labeled`/
`set_gauge` with a `cid_`-prefixed name — do not introduce a second metrics
representation. When adding a new panic-adjacent capture path, route it through
`CrashLog::record` so the secret-redaction and ring-buffer-eviction guarantees apply
uniformly rather than being re-implemented per call site.
