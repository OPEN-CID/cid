# Contributing to CID

Every command below was run against this repository while writing this document —
not copied from a template and assumed correct.

## Prerequisites

- Rust (stable toolchain). On Windows, `tauri dev`/`tauri build` additionally need MSVC
  Build Tools (`winget install Microsoft.VisualStudio.2022.BuildTools` with the `VCTools`
  workload) — not required for the browser+standalone-Core loop below.
- Node 20+ and npm.
- Git.

No other setup is required — `rusqlite` bundles SQLite, `git2` bundles libgit2, and
Tantivy needs nothing external.

## Development Setup

**Dev Container option**: opening this repo in VS Code with Docker (or GitHub
Codespaces) picks up `.devcontainer/devcontainer.json` automatically — Rust, Node, and
`npm install` are handled for you, giving the browser+standalone-Core loop below with no
manual toolchain setup. It intentionally does not set up the Tauri desktop shell (see
ADR 0016) — for that, follow the native setup below.

Fastest loop, no Tauri/MSVC required:

```powershell
git clone <this-repo>
cd cid
npm install

# Terminal 1
cargo run -p cid-core -- --port 5919

# Terminal 2
npm run dev
# open http://localhost:1420
```

Desktop shell (needs MSVC on Windows, see Prerequisites):

```powershell
npm run tauri:dev
```

CLI/TUI shell:

```powershell
cargo run -p cid-core -- --port 5919      # in one terminal
cargo run -p cid-tui -- --port 5919       # in another
```

**Unified command driver**: the `Justfile` at the repo root wraps the cargo/npm commands
above (and the checks CI runs) behind one memorable entry point — install
[`just`](https://github.com/casey/just), then `just --list` to see every recipe. `just`
with no argument (or `just check-all`) runs everything CI's `just-check-all` job runs:
fmt check, clippy, typecheck, lint, the theme-token drift check, and both test suites —
so a green `just check-all` locally means CI's checks pass too, not a hopeful guess.

## Project Structure

```
cid/
  cid-core/              # Rust core daemon — the single source of truth for all logic
    src/
      api/                # JSON-RPC 2.0 types + router (every RPC method lives here)
      access/             # Bearer-token access control for non-loopback binds
      auth/                # Local accounts, sessions, roles
      governance/          # Workspace policy (Autonomous mode, spend caps)
      git/                 # git2-rs wrapper, worktree lifecycle
      pty/                 # Native PTY per Mission
      mcp/, mcp_tasks/     # MCP client + Tasks extension
      model/               # Provider routing + tool-use loop
      roles/                # Planner/Reviewer + plan-approval gate
      role_profiles/       # Configurable named agent profiles
      confidence/           # Confidence Engine (9-signal patch scoring)
      semantic_engine/      # Tantivy search, dependency/test-impact/doc graphs
      context_engine/       # Structural (Tree-sitter) index
      sandbox/               # Autonomous-mode filesystem confinement
      forges/, trackers/, github/  # GitLab/Bitbucket, Jira/Linear, GitHub bridges
      decisions/              # ADR listing + deployment record
      persistence/            # SQLite (rusqlite)
    tests/                    # Integration tests — real HTTP/WS against a spawned Core
  cid-tui/                # Terminal client (ratatui) — no new Core surface
  src/                    # React frontend, shared by desktop/web/mobile shells
    components/
    mobile/                 # Mobile companion shell (approval/monitoring only)
    lib/                    # api.ts — the one JSON-RPC client every component uses
  src-tauri/              # Tauri v2 desktop shell
  docs/                   # 000–045: architecture/design docs (read before touching a subsystem)
  docs/adr/               # Architecture Decision Records
  tests/e2e/              # Playwright E2E (Flow 1 golden path)
```

Read the relevant `docs/0XX-*.md` file before implementing something that touches it —
each subsystem has one, and it names the real files, RPC methods, and tests involved
(see `docs/028-Backend.md` for the index).

## Branching & Commits

- Feature branches: `feature/<short-name>`, `fix/<short-name>`.
- Atomic, per-logical-change commits with a human-readable message.

## ADR Process

Every non-trivial decision gets a short ADR in `docs/adr/` (template:
`docs/adr/0000-template.md`): what you chose, what you gave up, why. Check the highest
existing ADR number before assigning a new one — this repo has one real history of a
number collision (see `docs/042-ADRs.md`) from not doing so.

## Code Style

- Rust: `cargo fmt --check` and `cargo clippy -p cid-core -p cid-tui --all-targets --
  -D warnings` must pass — CI-enforced for both crates.
- TypeScript: `npm run lint` must pass.
- No placeholder code presented as done — a stub is fine mid-task; reporting it as
  complete is not (`docs/003-Product-Philosophy.md`).
- Comments explain *why*, not *what*.

## Testing

Run everything locally before opening a PR:

```powershell
cargo test --workspace     # cid-core (359 unit + 74/5/9/11/3 integration) + cid-tui (3)
npm run test                # Vitest component tests
npx tsc --noEmit             # frontend typecheck
npm run build                 # production build
npm run test:e2e               # Playwright — needs Core running first, see below
```

For `test:e2e`, start Core first:

```powershell
cargo run -p cid-core -- --port 5919 --db C:\Temp\cid-e2e.db
npx playwright test
```

CI runs the same test surface: `cargo test --workspace --exclude cid --all-features`
(the Tauri package is excluded from this job since it needs system webview dependencies
already covered by the separate `build-tauri-*` CI jobs). This was a real, found-and-fixed
gap during Phase 5 — see `docs/036-CI-CD.md` for the history if you're curious what
changed and why.

## Delegating Simple Tasks (saving Claude usage)

If you're driving this repo with Claude Code, routine work — boilerplate, docstrings,
mechanical test scaffolding — doesn't need to burn Claude context or quota. Delegate it
to [OpenCode](https://opencode.ai) against OpenRouter's free router:

```powershell
opencode run --auto --model openrouter/openrouter/free "<task description>"
```

Run it as a background step; let it write/edit files directly rather than pasting file
contents into your own context, then verify the result yourself (build, test, read the
diff) before treating it as done. If the free router model produces unclear results or
fails tool calls, fall back to a specific free-tier model instead of retrying blind:

```powershell
opencode run --auto --model openrouter/meta-llama/llama-3.3-70b-instruct:free "<task>"
```

Reserve this for genuinely simple, well-scoped work. Anything requiring judgment about
architecture, security, or cross-cutting design should stay with the primary agent. See
`CLAUDE.md` for the enforced version of this rule.

## Running cid-core in production

This section and the one below it cover *shipping a signed desktop release*. If you're
instead standing up a `cid-core` instance for a team to actually use day to day (TLS,
a persistent service, backups, upgrades, monitoring), see
`docs/052-Production-Deployment.md` — that's the operational runbook; this file stays
focused on contributing to and releasing the project itself.

## Release Signing Setup (manual, one-time, maintainer-only)

`.github/workflows/release.yml` builds and, if these secrets exist, signs the desktop
app on every `v*.*.*` tag push. Without them it still runs and produces **unsigned**
installers (not a failure — just unsigned, so Windows SmartScreen/macOS Gatekeeper will
warn on install). This section is what you need to do yourself, outside any AI agent's
reach, to turn signing on.

### Windows

Microsoft requires code-signing certificates issued since mid-2023 to live on a hardware
token or a cloud HSM — a plain exportable `.pfx` is no longer issued by most CAs for new
purchases. Two real paths:

1. **Azure Trusted Signing (recommended — cheapest, no hardware, ~$10/mo)**
   - Sign up at [Azure Trusted Signing](https://azure.microsoft.com/en-us/products/trusted-signing) (needs an Azure subscription and a few days for identity validation).
   - This path needs `azure/trusted-signing-action` wired into `release.yml` instead of
     the plain `WINDOWS_CERTIFICATE` secret below — tell your AI agent (or ask me, in a
     fresh session) to add it once you have the Azure resource; it's a different signing
     mechanism (a signing *service* called during the build, not a certificate file) so
     the workflow needs a real edit, not just a secret.
2. **A traditional exportable `.pfx`** (if you already own one, e.g. from before the 2023
   policy change, or a CA that still offers OV certs on a software token):
   - Export it as a `.pfx` with a password.
   - Base64-encode it: `certutil -encode cert.pfx cert_base64.txt` (Windows), then open
     `cert_base64.txt` and strip the `-----BEGIN CERTIFICATE-----`/`-----END...` header
     and footer lines, leaving just the base64 body.
   - In the GitHub repo: **Settings → Secrets and variables → Actions → New repository
     secret**, add:
     - `WINDOWS_CERTIFICATE` — the base64 body from above.
     - `WINDOWS_CERTIFICATE_PASSWORD` — the `.pfx` password.

### macOS

Needs an active **Apple Developer Program** membership ($99/year — apple.com/developer).

1. In Xcode (or [developer.apple.com](https://developer.apple.com/account/resources/certificates)), create a **Developer ID Application** certificate.
2. Export it from Keychain Access as a `.p12` file with a password.
3. Base64-encode it: `base64 -i DeveloperIDApp.p12 -o cert_base64.txt` (macOS/Linux) or
   `certutil -encode DeveloperIDApp.p12 cert_base64.txt` (Windows, then strip headers as above).
4. Generate an **app-specific password** at [appleid.apple.com](https://appleid.apple.com) → Sign-In and Security → App-Specific Passwords (needed for notarization — this is *not* your regular Apple ID password).
5. Find your **Team ID** at [developer.apple.com/account](https://developer.apple.com/account) (top right, under your name).
6. Add these GitHub repo secrets (same Settings path as above):
   - `APPLE_CERTIFICATE` — the base64 body from step 3.
   - `APPLE_CERTIFICATE_PASSWORD` — the `.p12` password.
   - `APPLE_SIGNING_IDENTITY` — the certificate's name exactly as shown in Keychain
     Access, e.g. `Developer ID Application: Your Name (TEAMID1234)`.
   - `APPLE_ID` — your Apple ID email.
   - `APPLE_PASSWORD` — the app-specific password from step 4.
   - `APPLE_TEAM_ID` — the Team ID from step 5.

### Shipping a release

Once secrets are in place (or even before, to test the unsigned path):

```powershell
git tag v1.0.0
git push origin v1.0.0
```

This creates a **draft** GitHub Release with the built installers attached — review and
publish it manually; nothing goes live automatically.

## Security-Critical Changes

A change to `sandbox`, `access`, `auth`, or `governance` needs a real integration test
exercising the actual enforcement point (not just a unit test of the isolated function).
See `docs/031-Security.md`'s Failure Modes section for two real examples of what unit
tests alone missed in this project's own history.

## Pull Requests

- Describe what was built concretely, with exact commands to try it.
- List what was deferred/stubbed and which phase or follow-up it belongs to.
- List known issues honestly.
- State test status honestly — "I ran `cargo test --workspace` and it passed" is a claim
  that should be true when you write it, not aspirational.

Use `.github/pull_request_template.md` — it mirrors this checklist.

## Security

- Secrets never sent to a model as plain context.
- Terminal output and stored history pass through secret redaction
  (`cid-core/src/redact/mod.rs`) before being persisted or streamed.
- MCP servers are enabled per Repo Channel, not globally (least privilege).
- No hidden execution — every autonomous action is the result of an approved plan step
  or an explicitly pre-approved command pattern (`docs/031-Security.md`).

## License

MIT. By contributing, you agree your contributions will be MIT-licensed.
