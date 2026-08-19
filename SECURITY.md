# Security

This document states what CID's security boundaries actually enforce, and — just as
importantly — what they do not. Anyone enabling Autonomous mode or exposing Core beyond
localhost should read both halves.

## Reporting a vulnerability

Open a GitHub security advisory on this repository, or email the maintainers. Please do
not open a public issue for an unpatched vulnerability.

---

## 1. The Autonomous-mode worktree boundary

When a Mission runs in **Autonomous** mode, terminal commands go through
`SandboxManager`, which enforces the boundary in two layers.

### Layer 1 — command path policy (all platforms)

Before a process is spawned, the command and its arguments are scanned for path-shaped
tokens. Any token that is absolute, or climbs out with `..`, is resolved and checked
against the Mission's worktree. Anything landing outside is refused before execution.
Read-only system locations (`/usr`, `/bin`, `C:\Windows`, `C:\Program Files`) are exempt
so that invoking an interpreter is not mistaken for an escape. Separately, a Mission's
`run_terminal` working directory is clamped into the Mission root, so a model-supplied
`workdir` cannot redirect execution elsewhere.

This layer is deterministic and covers the common case. It **cannot see a path that a
program computes at runtime.**

### Layer 2 — kernel isolation, where the OS provides it

| Platform | Mechanism | Confines the filesystem? |
|---|---|---|
| macOS | `sandbox-exec` with `(deny default)` and worktree-scoped write allows | **Yes** |
| Linux, with `bwrap` installed | bubblewrap bind mounts | **Yes** |
| Linux, without `bwrap` | `unshare` fallback | **No** — policy only |
| Windows | Job Object | **No** — policy only |
| Any other OS (FreeBSD, illumos, …) | None — execution is refused | n/a — nothing runs |

On a platform CID carries no Layer 2 implementation for, `execute_sandboxed` returns
`Blocked` rather than running the command unconfined. Layer 1's path policy has already
been applied at that point, but policy alone was judged too weak to present as a sandbox,
so the command does not run at all.

**Windows is the important caveat.** A Windows Job Object constrains process lifetime,
CPU, and memory. It does **not** restrict file access. A process in a restricted job
object can write anywhere its user token allows. CID applies the Job Object for process
containment, but does not count it as a filesystem boundary — `sandbox.status` reports
`available: false` on Windows for exactly this reason.

An earlier version of this document claimed Windows Job Objects prevented writes outside
the worktree. That was wrong, and the test that was supposed to catch it asserted a
tautology (`passed || !passed`). Both have been corrected. See
[ADR 0011](docs/adr/0011-windows-sandbox-boundary.md) for the full analysis and the
AppContainer work that would close the gap.

The same class of bug recurred once while making CI green across three runners:
`writes_inside_the_worktree_are_allowed` was changed to accept *either* `Allowed` or
`Blocked` on Linux/macOS, to tolerate CI runners without usable sandbox tooling — which
left it passing whether the sandbox worked or was entirely broken. It now probes the
platform's sandbox tool directly and asserts the outcome that probe demands, in both
directions. When adapting a security test to a CI environment, assert the *correct*
result for that environment; never widen it to accept every result.

### What is not covered on any platform

- **Resource exhaustion.** No `ulimit`/cgroup limits; a command can consume RAM or CPU.
- **Runtime-computed paths on Windows and bwrap-less Linux.** See above.

### Network access — an allow-list, not a block

Sandboxed commands used to be able to reach any host with no restriction at all.
`SandboxManager::ensure_network_guard` now starts a local HTTP/HTTPS forward proxy
(`cid-core/src/net_guard/mod.rs`) and sets `HTTP_PROXY`/`HTTPS_PROXY` (plus lowercase
variants) in the sandboxed command's environment; the proxy only permits connections to
an allow-list (`github.com`, `registry.npmjs.org`, `pypi.org`, `crates.io`, and their
common subdomains/mirrors by default — editable via `sandbox.network_allowlist.get`/`.set`).

**This is application-layer, not kernel-enforced, and that matters:**

- It relies on the spawned process actually honoring `HTTP_PROXY`/`HTTPS_PROXY` — `git`,
  `npm`, `pip`, `cargo`, and `curl` do by default. A process using raw sockets, a
  hardcoded proxy bypass, or a runtime that ignores these env vars is not confined by
  this at all.
- A full network block (`unshare -n` with no allow-list) was deliberately rejected: it
  would break `git push`, `npm install`, and `cargo build` — the common case this project
  needs Autonomous mode to actually support. An allow-list is the version of "confined
  but still usable"; it is not equivalent to kernel-level network isolation.
- Verify it yourself: `cargo test -p cid-core --lib net_guard` (the proxy's allow/deny
  logic against a real local server) and
  `cargo test -p cid-core --lib -- execute_sandboxed_sets_the_proxy_env_vars` (proof the
  URL actually reaches a real spawned process's environment, not just the config struct).

### Verifying the boundary yourself

```powershell
# Unit tests, including the boundary probe
cargo test -p cid-core --lib sandbox

# Against a running Core
curl -X POST http://127.0.0.1:5919/api/rpc -H "Content-Type: application/json" `
  -d '{"jsonrpc":"2.0","id":"1","method":"sandbox.test","params":{"worktree_path":"C:\\path\\to\\worktree"}}'
```

`sandbox.test` writes a uniquely-named probe file outside the worktree and then checks
the filesystem to see whether it exists. It reports the result *and* which layer
enforced it, so a pass backed only by path policy is not mistaken for kernel isolation.

The test `autonomous_command_cannot_write_outside_the_worktree` asserts the boundary
actually held, and fails the build if a command escapes.

### Command allow-lists

Sandboxing is not the only guard. In Autonomous mode, `run_terminal` is additionally
checked against the Repo Channel's command allow-list. **A scope with no configured
allow-list denies everything** — the default is closed, not open. Commands not matching an
allow-listed pattern fall back to a human approval request rather than executing.

Subagents inherit the parent Mission's worktree and permissions; they get no additional
reach.

---

## 2. Core network access control

By default Core binds to `127.0.0.1` and requires no authentication — the OS is the
boundary.

**Binding anywhere else requires a token, and Core refuses to start without one:**

```powershell
cid-core --host 0.0.0.0 --auth-token "$(cid-core --generate-token)"
```

When a token is configured, both `/api/rpc` and the `/ws` upgrade require the token.
Comparison is constant-time. `/health` stays open and reports only reachability, version,
uptime, connected client count, and whether auth is required.

Two ways to present it, because a browser can only use the second:

| Client | Channel |
|---|---|
| Native (TUI, curl, desktop shell, any HTTP client) | `Authorization: Bearer <token>` |
| Browser WebSocket | `Sec-WebSocket-Protocol: cid.bearer.<base64url(token)>` |

`new WebSocket(...)` cannot set request headers — the subprotocol list is the only part
of the handshake a browser controls. Until this existed the web client could not
authenticate at all, so *any* Core it could reach was one that required no token: every
hosted deployment failed with an opaque closed socket. The header is still accepted and
still wins when both are present, so a malformed subprotocol cannot downgrade a valid
header.

The token is base64url-encoded there only because a subprotocol must be a valid HTTP
token; this is encoding, not encryption. It is the same secret over the same handshake as
the header, and it is deliberately **not** a `?token=` query parameter — those land in
access logs, proxy logs, and `Referer` headers.

In the browser the token is stored in `localStorage` and pasted per device. It is
deliberately not a `VITE_*` build variable: those are inlined into a bundle that anyone
who can load the page can download, which would publish the secret that grants full
control of Core.

CORS is an explicit origin allow-list (the local desktop and web shell origins by
default, extended with `--allow-origin`), not `Any`. Without this, any web page the user
visited could drive an RPC surface that reads files and runs commands.

### Limits of this model

- The token authenticates the **connection, not a person**. Everyone who has it has full
  access. Per-user identity arrives with Phase 3's Workspace membership.
- No rotation without a restart.
- Traffic is plain HTTP. Put a TLS-terminating reverse proxy in front if the network path
  is not trusted — the token is only as private as the connection carrying it.

See [ADR 0012](docs/adr/0012-core-access-control.md).

---

## 3. Secrets

- Provider API keys are stored in OS-native credential storage (Windows Credential
  Manager, macOS Keychain, Secret Service) via the `keyring` crate. `settings.get` returns
  a redacted form (`sk-…abcd`) — the full value never reaches the frontend.
- Terminal output and stored Mission history are passed through `redact::redact_secrets`
  before being persisted, streamed to a shell, or returned to a model as tool output. It
  covers key/value credential forms and the recognisable provider key formats (Anthropic,
  OpenAI, GitHub, Google, Slack, AWS) plus bearer headers.
- This is pattern matching, not a guarantee. A secret in an unusual format can pass
  through.

---

## 4. Human-in-the-loop guarantees

- Outside Manual autonomy, the Implementer **cannot run without an approved plan**. The
  gate is enforced in Core, not in the UI.
- Editing an approved plan returns it to draft — an approval applies to the text that was
  approved, not to whatever replaced it.
- In Co-Pilot mode every tool call requires explicit approval.
- In Autonomous mode only allow-listed commands run unattended; anything else pauses for
  approval.
- CID never force-pushes or rewrites a shared branch without an explicit confirmation.

---

## 5. Prompt injection on untrusted repository content

CID's core loop reads a repository's own content — file contents, `AGENTS.md`,
`SKILL.md`, git diffs, MCP tool results — and feeds it to a model holding file-write and
shell tools. That content comes from a repository CID does not control, so it is
untrusted input, no different in kind from a webpage a browser renders. **This class of
risk is mitigated, never eliminated** — the same caveat every other agentic coding tool
carries today.

### What's in place

- **Delimiting plus token sanitization.** Everything that enters a prompt from outside
  CID itself — `AGENTS.md`, `SKILL.md`, and every tool result (`read_file`, `list_files`,
  `git_diff`, `git_status`, `mcp_call`, `run_terminal`, and the rest) — is wrapped in an
  explicit `<untrusted_repo_instruction source="...">` delimiter, preceded by a
  system-prompt rule that content inside it is data to analyze, never an instruction to
  follow. Before wrapping, known model-control-token sequences that could impersonate a
  turn boundary or system message (`<|im_start|>`, `<|im_end|>`, `<|endoftext|>`,
  `<|eot_id|>`, `<|start_header_id|>`, `[INST]`/`[/INST]`, and a close-tag escape for the
  delimiter itself) are neutralized. See `sanitize_untrusted_repo_content` and
  `wrap_untrusted_repo_content` in `cid-core/src/skills/mod.rs` — the single
  implementation both the live system prompt (`SkillsManager::build_system_context`,
  `wrap_untrusted_tool_result` in `model/mod.rs`) and the `skills.resolve` preview RPC
  build on, so what a Repo Channel's Skills panel shows and what the model actually
  receives are the same sanitized text, not two implementations that can drift apart.
- **A human review gate on `AGENTS.md`.** `AGENTS.md` is repo-authored instructions that
  would otherwise load straight into every Mission's system prompt on first connect. It
  is now detected and shown to the user, but excluded from the system prompt until a
  human explicitly approves it (`repo.agents_md.approve`; the frontend's
  `AgentsMdReviewCard`). **Known limitation:** approval does not currently re-arm if
  `AGENTS.md` changes later — a repo approved once stays approved even if a subsequent
  `git pull` changes its `AGENTS.md`. Re-review on content change is not yet implemented.
- **Provenance marking in History.** Once untrusted content has entered a Mission's
  context (an approved `AGENTS.md`/`SKILL.md`, or any prior `read_file`/`list_files`/
  `git_diff`/`git_status`/`mcp_call` result), every subsequent tool call in that Mission
  carries a `provenance` marker, shown in the message History panel. **This is
  deliberately coarse** — a Mission-wide flag, not per-argument taint tracking. It tells
  you untrusted content was *somewhere* in context when a call was made, not that the
  call was actually influenced by it. Treat it as a hint to review, not a verdict.

### What this does not do

- **No semantic detection.** Sanitization is a fixed list of known control-token
  sequences, not a classifier — plain-English injection attempts ("ignore your previous
  instructions and...") are not stripped, only delimited. Delimiting relies on the model
  honoring the "data, not instructions" rule, which a sufficiently capable attack can
  still attempt to override (this is an open research problem industry-wide, not
  specific to CID).
- **No re-approval on `AGENTS.md` change**, as above.
- **Tool results other than `AGENTS.md`/`SKILL.md` are not gated by human review** — a
  `read_file` on a malicious file, or an MCP server's response, is delimited but still
  flows into context automatically, the same as it always has. Co-Pilot's per-tool-call
  approval remains the primary control on what a model can *do* with what it read; this
  section is about what it's told, not what it's allowed to act on.
- **Delimiting is uniform, not tool-specific.** Every tool's result is wrapped the same
  way regardless of which tool produced it (including `run_terminal`, whose output also
  passes through `redact::redact_secrets`, §3, first) — there is no attempt to judge
  which tools are "more" untrusted than others. A confirmation like `write_file`'s
  `{"ok": true}` gets the same delimiter as a file read; this trades a little noise for
  not having to maintain a special-case list.

---

## 6. The `file.*` and `fs.list_dirs` filesystem RPCs

These are network-facing RPCs the Editor and the repo-connect picker call directly over
`/api/rpc` — a different trust boundary from the model's own tool calls (§1), since a
caller here has no Mission worktree to confine to.

- **`file.read`/`file.write`/`file.list` are confined to connected repos.** Every path is
  resolved via `path_confine::resolve_confined_path_in_any` against the list of every
  currently-connected repo channel's own path — the same primitive (not a second,
  possibly-drifting implementation of it) the model's own file tools use for their
  worktree confinement. A path outside every connected repo (absolute escape, `..`
  traversal, a symlink that resolves outside) is refused. See
  `cid-core/src/path_confine.rs` and the `file_rpc_confinement` test module in
  `cid-core/tests/api_integration.rs`.
- **`fs.list_dirs` is deliberately *not* confined the same way** — its entire purpose is
  letting the repo-connect picker browse the filesystem *before* any repo is connected, so
  there is no connected-repo allow-list to confine it to. Its boundary is narrower in a
  different dimension instead: it returns **directory names only** — never a file name,
  never file contents, never anything from inside a directory other than the names and
  git-repo-ness of its immediate subdirectories. It enumerates whatever directories the
  Core process's OS user can read, anywhere on the filesystem (all local drives on
  Windows, the whole tree from `/` on Unix) — the same reach the Core process already has
  for every other purpose, just exposed as a browse affordance instead of implicitly
  assumed.
- **Both are protected the same way every other state-reading RPC on this surface is: by
  §2's loopback bind, not by a per-call session check.** Neither calls `require_session`,
  matching the existing pattern on `repo.list` and the other `file.*` handlers — Core's
  default posture is "anyone who can reach `127.0.0.1:<port>` already has full API access"
  (§2), and `fs.list_dirs` does not raise that bar or lower it. If Core is exposed beyond
  loopback (`--host 0.0.0.0`), the `--auth-token` requirement in §2 covers this RPC exactly
  as it covers every other one — there is no separate opt-out.
- **What this does not do:** `fs.list_dirs` does not check the requested path against any
  allow-list, so it will happily walk into a directory the user did not intend to expose
  (a mounted network share, another user's home directory the OS permits reading) — the
  only filter is directory-vs-file and dot-hidden-vs-visible. Treat it as exactly as
  sensitive as shell access to `ls` on the same machine, because that is what it is.
