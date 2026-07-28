# 038 — Developer Guide

## Vision

Get a developer from clone to a running, testable CID in as few steps as possible — the
practical companion to `037-Testing.md` and `004-System-Architecture.md`.

## Goals

**Prerequisites**: Rust (stable, MSVC toolchain on Windows), Node 20+, `git`. Windows
additionally needs MSVC Build Tools for `tauri dev`/`tauri build` specifically
(`025-Desktop-Architecture.md`).

**Fastest loop — browser + standalone Core** (recommended for iteration):
```powershell
cargo run -p cid-core -- --port 5919 --db C:\Temp\cid.db
npm install
npm run dev
# open http://localhost:1420
```

**CLI/TUI**:
```powershell
cargo run -p cid-core -- --port 5919
cargo run -p cid-tui -- --port 5919
```

**Desktop (Tauri)**:
```powershell
npm run tauri:dev
```

**Run everything**:
```powershell
cargo test --workspace
npm test
npx tsc --noEmit
npm run build
```

**Exercise Autonomous mode / governance**: requires enabling Workspace policy first
(`governance.policy.set` with an Admin session) — Autonomous mode is refused by default,
which is a feature, not a bug to work around (`017-Workspace-Manager.md`).

**Exercise the sandbox boundary**:
```powershell
curl -X POST http://127.0.0.1:5919/api/rpc -H "Content-Type: application/json" `
  -d '{"jsonrpc":"2.0","id":"1","method":"sandbox.test","params":{"worktree_path":"C:\\path\\to\\worktree"}}'
```

## Non-Goals

A from-scratch onboarding wizard or setup script — Phase 5's contributor-experience work
(`CONTRIBUTING.md`) is the place for a genuinely-tested clean-environment setup path; this
document assumes a developer already has the prerequisites.

## Architecture

See `004-System-Architecture.md`.

## Tradeoffs

N/A.

## Failure Modes

WDAC (Windows Defender Application Control) can block a freshly-built test binary from
executing in some locked-down environments — a real, observed environment issue (not a
CID bug), resolved by rebuilding (`cargo clean` the specific binary, rebuild) in the one
case this was hit during Phase 4 development.

## Security

N/A — see `031-Security.md`.

## Testing

See `037-Testing.md`.

## Implementation Order

N/A.

## Acceptance Criteria

Every command in this document has been run in this repository during its own
development and produced the output implied — not copy-pasted from a template without
verification.

## AI Coding Rules

Keep this document's commands in sync with any changes to `main.rs`'s CLI flags,
`package.json`'s scripts, or `cid-tui`'s flags — a stale command here is worse than no
command, since it wastes a new contributor's time diagnosing a doc bug instead of a real
one.
