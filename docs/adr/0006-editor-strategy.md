# ADR 0006: Editor strategy — CodeMirror 6 inline + Monaco full pane, no native GPU engine Phase0-3

- **Date**: 2026-07-26
- **Status**: Accepted, **partially superseded 2026-07-27** —
  `050-Gold-Standard-Review.md` F5 found the CodeMirror inline editor described in the
  Decision below was never actually built (no `codemirror` dependency exists in this
  repository); Monaco alone shipped and is the sole editor going forward, per
  `012-Semantic-Editing.md`'s corrected Goals section. Similarly, the "Monaco... with LSP
  integration for supported languages" line below never happened — no LSP client exists
  anywhere in `cid-core`. That part of the decision remains the *intent*; it's tracked as
  real, not-yet-started work in `051-Editor-Excellence-Roadmap.md` Wave 3, gated
  specifically on feeding diagnostics into agent context (Wave 3.3), not editor parity for
  its own sake. The GPU-engine non-goal below is unaffected and still stands.
- **Context**: Build Prompt v1 asked for native GPU-rendered editor built from scratch, no Monaco. v3.0 explains why that doesn't hold up: Zed, built by Atom/Tree-sitter creators with $32M funding, took ~5 years to reach 1.0 (April 29, 2026) building exactly this — realistic cost, not few-days line item. Market converged on embedding proven editors plus ACP host for pop-out to full IDE. Non-goal for foreseeable roadmap: try to beat Monaco/CodeMirror/Zed GPUI on raw rendering Phase0-3.
- **Decision**: 
  - Inline editor (thread-embedded, quick edits/approvals without leaving conversation, e.g., hand-tweak diff hunk before accepting): **CodeMirror 6** — lightweight, fast to load inline in chat message
  - Full file/project editor pane: **Monaco** (same component VS Code, Kiro, early Cursor build on) in Session's right panel or dedicated tab, with LSP integration for supported languages — fulfills "open a file like VS Code" ask
  - File tree annotated by Context Engine once enabled — not plain tree (per Part 7)
  - Phase1+: CID becomes **ACP host** (Agent Client Protocol, created by Zed Aug 2025, co-developed JetBrains Oct 2025, Apache-licensed, JSON-RPC over stdio, 25+ agents, 10+ editor surfaces) so session can pop out into Zed or JetBrains IDE and hand back — interops with best editors instead of out-building them
- **Alternatives**:
  - Build native GPU engine from scratch: multi-year, multi-team build, exactly the failure mode Part 0 rule 1 warns against
  - Monaco only everywhere: heavier for inline quick edits, CodeMirror 6 better for embedding in chat
  - Custom CodeMirror-only: loses VS Code-level language support Monaco provides via well-tested LSP integration
- **Consequences**:
  - Phase0 EditorPane uses Monaco via `@monaco-editor/react` 4.6 — file tree in left of pane, editor on right, Save writes via `file.write` RPC which goes to repo or worktree path
  - No custom GPU rendering, keeps Phase0 small and testable per Part 0 rule 3 (working Phase0 beats scaffolded everything)
  - Pop-out via ACP is Phase1+, not Phase0 — document as deferred
  - Performance: Monaco is batteries-included but heavier than building minimal editor; acceptable trade for Phase0 budgets (<150MB idle with optional features off, <2s cold start) — budgets to validate after profiling, not guarantees per Part 17
- **References**: Build Prompt Parts 2 (Zed proof), 11 (editor strategy), 17 (performance budgets), 18 tech stack, 22 phases.
