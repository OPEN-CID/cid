# ADR 0016 — Dev Container: built, scoped to the browser+Core loop only

**Status:** Accepted
**Context:** Phase 5, contributor experience

## Context

The Phase 5 build prompt asks explicitly: build a Dev Container "if the maintenance cost
is judged worth it — note that tradeoff as an ADR either way, don't just build it
silently." This is that ADR.

## Decision

**Built, but deliberately scoped to the browser+standalone-Core dev loop only — not the
Tauri desktop shell.**

`.devcontainer/devcontainer.json` uses the standard `rust:1-bookworm` devcontainer image
plus the Node 20 feature, forwards the two ports the dev loop needs (1420 for Vite, 5919
for Core), and runs `npm install` on creation. A contributor opening this repo in VS Code
(locally with Docker, or in GitHub Codespaces) gets a working `cargo run -p cid-core` +
`npm run dev` loop with zero manual toolchain setup.

**What it deliberately does not attempt**: building or running the Tauri desktop shell.
Tauri's Linux build needs `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, `librsvg2-dev`, and `libssl-dev` (the same list CI's
`build-tauri-linux` job installs) — addable to the container, but Tauri's own dev/build
loop inside a container has real friction beyond dependencies: no native window system to
render into without additional X11/Wayland forwarding, and `tauri dev`'s hot-reload story
assumes a host display. A contributor who wants the real desktop shell should still run
natively per `CONTRIBUTING.md`'s Prerequisites section (MSVC on Windows, etc.) — the
container is an accelerant for the fastest loop, not a replacement for every dev path.

## Alternatives considered

- **No devcontainer at all.** Rejected: the marginal cost of the scoped version above is
  small (one JSON file, using an off-the-shelf base image, no custom Dockerfile to
  maintain), and the benefit — a genuinely zero-setup path for Codespaces users and
  non-Windows contributors — is real.
- **A devcontainer covering the full Tauri build too.** Rejected for this pass: the
  container-display friction above makes it a bigger, higher-maintenance surface for a
  benefit (containerized Tauri dev) most contributors won't actually need, since the
  fastest loop (browser + standalone Core) doesn't require Tauri at all. Revisit if real
  contributor demand emerges for a fully-containerized desktop-shell loop.

## Consequences

- The devcontainer is **not** validated by CI — it's a local/Codespaces convenience, and
  CI's own `test-rust-linux` job already covers the equivalent dependency set without a
  container. A devcontainer definition going stale (e.g., the Rust or Node version drifting
  from what CI actually uses) is a real, currently-unmitigated risk; worth a periodic
  manual check, not currently automated.
- `CONTRIBUTING.md`'s Prerequisites and Development Setup sections remain the source of
  truth for the non-container path — the devcontainer is presented as one option among
  several, not the only supported one.
