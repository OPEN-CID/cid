# CID unified command driver. Every recipe here wraps an existing cargo/npm
# command already used directly elsewhere (CI, CONTRIBUTING.md) — this file
# adds a single memorable entry point (`just <recipe>`), it does not replace
# or duplicate what those commands do. See CLAUDE.md's Background Task
# Delegation Rule for when to reach for `opencode run` instead of doing work
# in-session.

default: check-all

# Start Rust Core + Vite frontend together (existing dev:all wrapper).
dev:
    npm run dev:all

# Start Core only, for when you want the frontend running separately.
core:
    npm run dev:core

# Native desktop app in dev mode (Tauri v2).
desktop-dev:
    npm run tauri:dev

# Rust workspace tests (unit + integration + fuzz + property + perf).
test-rust:
    cargo test --workspace --exclude cid --all-features

# Frontend unit tests (Vitest).
test-frontend:
    npm run test

# Both Rust and frontend unit/integration suites.
test: test-rust test-frontend

# Playwright E2E — starts vite itself, but cid-core must already be running (see `just core`).
test-e2e:
    npx playwright test

# Rust format check (fails on drift, does not rewrite).
fmt-check:
    cargo fmt --all -- --check

# Rust format, applied.
fmt:
    cargo fmt --all

# Clippy across the whole workspace, warnings are errors.
clippy:
    cargo clippy --workspace --all-targets -- -D warnings

# TypeScript typecheck, no emit.
typecheck:
    npx tsc --noEmit

# ESLint.
lint:
    npm run lint

# Regenerate src/index.css's CSS-variable block from src/theme/tokens.json.
theme-generate:
    npm run theme:generate

# Fails if src/index.css has drifted from src/theme/tokens.json — same check CI runs.
theme-check:
    npm run theme:check

# Everything CI checks, in one place — what `just` (no argument) runs.
check-all: fmt-check clippy typecheck lint theme-check test-rust test-frontend

# Production desktop app bundle (Windows/macOS/Linux, per host OS).
build-desktop:
    npm run tauri build

# Standalone headless server binary, for Docker/cloud deployment.
build-server:
    cargo build --release -p cid-core

# CLI/TUI terminal client binary.
build-tui:
    cargo build --release -p cid-tui

# Production frontend build (tsc + vite build).
build-web:
    npm run build

# Remove build artifacts. Does NOT remove node_modules or Cargo's registry
# cache — only what a fresh clone would need to regenerate.
clean:
    cargo clean
    rm -rf dist
