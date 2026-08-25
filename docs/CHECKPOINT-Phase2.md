# CID — Phase 2 Checkpoint Report

**Scope:** Appendix A Part 22, Phase 2 — "A real multi-surface platform."

This report supersedes an earlier Phase 2 checkpoint. That report marked the phase GO,
and much of the work behind it was real — but a verification pass found four deliverables
that did not do what the report claimed, including the phase's stated security-critical
one. Those are documented below with the same weight as the working parts, because a
checkpoint that overstates what holds is worse than no checkpoint.

---

## 1. What was built

### Working as originally reported

| Deliverable | Module | Tests |
|---|---|---|
| Subagent orchestration, sharing the parent Session's worktree | `subagent/` | 7 |
| Background/ambient model routing to detected local runtimes | `background_model/` | 5 |
| Slack bridge (`/cid` slash command, channel allow-list, status posting) | `slack_bridge/` | 8 |
| Teams bridge (incoming webhook, Adaptive Cards, team allow-list) | `teams_bridge/` | 8 |
| MCP Tasks extension (pollable handles, timeouts, progress) | `mcp_tasks/` | 6 |
| Dependency graph, git-blame overlays | `semantic_engine/` | — |
| Linux added to the CI build/test matrix | `.github/workflows/ci.yml` | — |
| Mobile technology bake-off ADR (Tauri v2 Mobile selected) | `docs/adr/0010-*` | — |

### Corrected in this pass

**1. The sandbox did not confine the filesystem on Windows, and the test could not fail.**

The previous report called the sandbox boundary "the security-critical deliverable of this
phase" and reported it passing. The test was:

```rust
assert!(passed || !passed, "verify_sandbox_boundary returned a valid result");
```

That is a tautology. Underneath it, the Windows implementation used a Job Object, which
constrains process lifetime, CPU, and memory but **does not restrict file access at all**.
It checked only that the *working directory* was inside the worktree — which
`echo x > C:\elsewhere\file.txt` sidesteps entirely. `verify_sandbox_boundary` also
classified results by exit code, so on macOS a write the kernel *did* block was recorded
as an escape.

Now:
- **Layer 1, all platforms:** command path policy. Command and argument tokens that are
  absolute or use `..` are resolved and checked against the worktree before anything is
  spawned. Read-only system paths are exempt so invoking an interpreter is not misread as
  an escape. A Session's `run_terminal` working directory is clamped into the Session root,
  so a model-supplied `workdir` cannot redirect execution.
- **Layer 2, where the OS provides it:** `sandbox-exec` on macOS, `bubblewrap` on Linux.
  Windows Job Objects are still applied for process containment but are **not** counted as
  a filesystem boundary — `sandbox.status` reports `available: false` there.
- **Verification checks the filesystem, not exit codes**: it writes a uniquely-named probe
  outside the worktree and asserts the file does not exist afterwards.
- `SECURITY.md` and [ADR 0011](adr/0011-windows-sandbox-boundary.md) state the Windows
  limitation plainly instead of claiming a guarantee that does not exist.

**2. The sandbox was never applied to actual command execution.**

`execute_sandboxed` was reachable only from the `sandbox.test` RPC. The Implementer's
`run_terminal` tool spawned commands directly, unsandboxed, with `workdir` defaulting to
`"."`. Separately, `execute_tool_with_approval` required human approval unconditionally,
so **Autonomous mode never actually ran autonomously** and the allow-list was never
consulted on the execution path.

Both are now wired: tool execution carries an `ExecutionContext` (Session root, autonomy
level, repo path). Autonomous Sessions check the Repo Channel allow-list; pre-approved
commands run sandboxed without prompting, anything else falls back to the approval
request, and explicitly denied commands are refused.

**3. Access control was UI state that enforced nothing.**

`AccessControlPanel` kept "allow remote" and origin lists in React state and wrote them
nowhere. CORS was `allow_origin(Any)` — any web page the user visited could drive an RPC
surface that reads files, runs commands, and reaches model credentials.

Now: `AccessPolicy` in Core. Binding to a non-loopback address **fails at startup** unless
a token is supplied (`--auth-token`, `CID_AUTH_TOKEN`, or `--generate-token`). Both
`/api/rpc` and the `/ws` upgrade require `Authorization: Bearer <token>` when one is
configured, compared in constant time. CORS is an explicit origin allow-list. `/health`
stays open and reports whether auth is required, so the panel can warn when Core is
exposed. See [ADR 0012](adr/0012-core-access-control.md).

**4. Repository indexing was a no-op, and there was no Tantivy.**

`index_repository` walked the file tree and logged `"Would index: {path}"`. It never
populated anything. Enabling the semantic engine appeared to work and indexed zero files;
only explicit `index_file` calls did anything, into an in-memory `HashMap` that vanished
on restart.

Now: a real Tantivy index under `<repo>/.cid/index`, with BM25 ranking, batched commits,
per-file replace-on-change so edits don't leave stale chunks, overlapping line-window
chunking, and skip rules for `.git`/`target`/`node_modules`/etc. Search queries Tantivy
first and blends BM25 with embedding cosine similarity for hybrid retrieval; the previous
in-memory scan remains as a fallback when the on-disk index cannot be opened. A test
asserts the index survives being closed and reopened.

**5. The Web Shell and MCP Apps renderer were dead code.**

`WebShell.tsx` (420 lines) and `McpAppCard.tsx` (375 lines) were never imported by any
component. Both were reported as delivered. Now `ConnectionBanner` renders app-wide, a
`server` tab carries `HealthDashboard` and the rewritten `AccessControlPanel`, and tool
results in the Session thread render as MCP Apps when a server sends renderable content
(`extractMcpAppContent`), falling back to the plain result card otherwise.

---

## 2. What was deferred or stubbed

| Item | Why | Phase |
|---|---|---|
| HNSW vector index | The embedding set is small enough that exact cosine similarity is not the bottleneck; adding an ANN index before there is a corpus to justify it is premature | Phase 3+, gated on real corpus size |
| Windows AppContainer sandbox | Real kernel confinement on Windows; deferred for cost, not because the approach is unclear (ADR 0011) | Phase 3+ |
| Network isolation, resource limits in sandbox | Filesystem confinement only | Phase 3+ |
| Slack/Teams real-time (Socket Mode, Graph subscriptions) | Webhook-only is adequate to demonstrate the bridge | Phase 3 |
| Per-user identity | The access token authenticates a connection, not a person | Phase 3 |
| Mobile companion app | Only the bake-off ADR was in Phase 2 scope | Phase 3 |

---

## 3. Known issues

- **Autonomous mode on Windows is guarded by the allow-list and path policy, not kernel
  isolation.** A program that computes an out-of-worktree path at runtime is not stopped.
  This is the single most important caveat in this report.
- **Linux without `bwrap` has the same limitation.** The `unshare` fallback bind-mounts the
  worktree onto itself, which does not remove access to the rest of the filesystem.
- **The access token has no rotation without a restart**, and traffic is plain HTTP unless
  a TLS-terminating proxy is placed in front.
- **Embeddings are a deterministic hash-based projection, not a learned model.** They add
  a weak signal to ranking. Real embeddings need the background model wired to an
  embedding endpoint.
- **Tantivy indexes write into the repo's `.cid/index`.** `.cid/` is gitignored on connect,
  but a read-only checkout falls back to the in-memory index (logged, not silent).

---

## 4. Test status

```
cargo test -p cid-core
  unit:        175 passed
  integration:  34 passed
npm run test:    2 passed
npx tsc --noEmit: clean
npm run build:   clean
```

New in this pass: the sandbox boundary suite (a real escape probe, absolute-path blocking,
`..` traversal blocking, allowing legitimate in-worktree work, and a check that status
does not overstate the Windows guarantee); the access-control suite (401 without a token,
401 with a wrong token, 200 with the right one, health reachable either way); the Tantivy
index suite including restart persistence; and repository-scan tests proving indexing
actually happens.

Not covered: cross-shell E2E driving desktop and web against one Core simultaneously, and
MCP Apps rendering against a live 2026-07-28-spec server. Both are listed in Part 21's
Phase 2 bar and are honestly absent — the MCP Apps path is unit-covered via
`extractMcpAppContent` but has not been run against a real server.

---

## 5. Proposed go/no-go for Phase 3

**GO**, with the Windows sandbox limitation understood and documented rather than hidden.

The mobile bake-off ADR ([0010](adr/0010-mobile-technology-bakeoff.md)) selects **Tauri v2
Mobile**, so Phase 3's companion app builds on the existing React bundle and the same
JSON-RPC contract.
