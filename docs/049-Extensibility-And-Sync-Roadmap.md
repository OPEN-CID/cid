# 049 — Extensibility Ecosystem & Cross-Device Sync (Phase 7+ design sketch)

## Vision

Two related "what's next" questions, both explicitly Phase 7+ (evidence-gated, not
committed work): how CID could support a VS Code-style third-party extension ecosystem,
and how state could follow a user across devices. Written now, as design thinking with a
concrete evidence gate, per Part 0's own rule: ambiguity gets a documented default, not
silence — and per the Release prompt's instruction that "not yet" is a complete answer
when it names what would change it.

## Goals

### Part 1 — Extensibility: what CID already has vs. what VS Code's model adds

VS Code's extension architecture, for reference (stable since ~2018, not something that
shifts year to year): a manifest (`package.json` with an `engines.vscode` version range
and a `contributes` section), an out-of-process **Extension Host** so a misbehaving
extension can't crash the main UI process, lazy **activation events**
(`onLanguage:rust`, `onCommand:...`, `workspaceContains:...`) so 90% of installed
extensions cost nothing until actually needed, a typed `vscode` API namespace
(`window`, `workspace`, `commands`, `languages`), a **Webview API** (sandboxed, iframe-
based custom UI), a separate **color/icon/product-icon theme** format (JSON token→color
maps, no code execution required), and a Marketplace with publisher accounts and
`.vsix` packaging.

**CID already has three of the same underlying needs solved, via real open standards
rather than a proprietary format** — this is not a gap, it's a deliberate advantage
stated back in Part 12: a team adopting CID doesn't migrate anything.

| VS Code concept | CID's existing equivalent | Assessment |
|---|---|---|
| Extensions that add *tools/capabilities* | **MCP servers** (Part 8) — any process speaking the 2026-07-28 MCP spec, added per-Workspace, enabled per-Repo-Channel | Already a real, working extension point, and a better one than VS Code's own: MCP is a genuine multi-vendor standard (25+ agents/editors), not CID-specific. Nothing to build here — document and promote it as the answer to "how do I extend CID's capabilities," rather than inventing a second, incompatible plugin format. |
| Extensions that add *custom UI inside a tool call* | **MCP Apps** (the 2026-07-28 MCP extension, Part 2/Part 8) — servers render real interactive HTML inline | This is architecturally the same problem VS Code's Webview API solves, already solved at the protocol level CID targets. A third party wanting a custom dashboard/form inside a Mission thread ships it as an MCP Apps-capable server, not a CID-specific plugin. |
| Extensions that add *reusable knowledge/procedures* | **`SKILL.md`** bundles (Part 12) | Already real, already UI-editable, already resolves Workspace→Repo→Mission. |
| Process isolation (one bad extension can't crash the host) | MCP servers already run out-of-process (stdio/HTTP transports) | Inherited for free from the MCP architecture — CID never had to build its own extension-host sandboxing for this category. |

**What's genuinely missing** — the part VS Code's model covers that none of the above
does:

1. **UI-structure extensions**: adding a new left-rail section, a new right-panel tab
   (beyond what a Mission-scoped MCP Apps surface gives you), or a custom command in the
   composer that isn't a tool call. There is no manifest format or contribution-point
   system for this today.
2. **Themes**: CID currently ships one dark theme (Tailwind + shadcn/ui, CSS custom
   properties) with no user-facing theming system, no light-theme toggle exposed, and no
   way for a third party to ship a color scheme.
3. **A packaging/distribution story** for whichever of the above gets built — a
   Marketplace-equivalent, or at minimum a documented manifest format and a way to load
   one locally for development, mirroring VS Code's Extension Development Host (F5) loop.

**Recommended design direction, if/when this is built** (Phase 7+, not started):

- **Themes first, extensions second** — themes are the lower-risk, higher-value-per-
  effort piece: a JSON token→CSS-variable map (`contributes.colors`-equivalent), no code
  execution, trivially sandboxed, and immediately gives users the "make it mine"
  customization VS Code's ecosystem is famous for. A light theme and a
  community-theme-loading mechanism are a reasonable Phase 7 candidate on their own,
  decoupled from the larger extension-host question.
- **UI-structure extensions should reuse MCP Apps' rendering model** (sandboxed HTML,
  not arbitrary React component injection) rather than inventing a second in-process
  extension host — CID's "many thin surfaces over one Core" architecture (Part 15)
  doesn't have a single privileged UI process the way Electron-based VS Code does, so a
  VS Code-style in-process Extension Host isn't the natural fit here anyway. A
  sandboxed-HTML contribution point that can register a named right-panel tab is a more
  architecturally honest answer than copying VS Code's model wholesale.
- **Do not build a second tool-plugin format.** The temptation with any "extension
  system" ask is to build a bespoke plugin API; CID's actual answer is "you're already
  looking at it — MCP." Reiterating this in the eventual Phase 7 build prompt is worth
  more than new code.

**Evidence gate**: real demand from users who've hit the ceiling of MCP servers + Skills
+ a light/dark toggle and specifically want structural UI customization or third-party
themes — not speculative "VS Code has this so we should too."

### Part 2 — Cross-device sync ("do work on laptop, see it on mobile")

**What already works today, undocumented until now**: CID's mobile shell is the *same*
React bundle as desktop/web (Part 15), reading `VITE_CID_CORE_HOST`/`VITE_CID_CORE_PORT`
at runtime (`src/lib/api.ts`). If a laptop's Core is bound to its LAN address (not just
`127.0.0.1`) with an auth token — which `AccessPolicy::new` already requires by
construction for any non-loopback bind (ADR 0012) — a phone on the **same network** can
point at `http://<laptop-lan-ip>:5919` and see the identical Missions, messages, and
state, because it isn't sync at all: it's the same Core, the same SQLite file, the same
process. **No new architecture needed for the same-network case.** What's missing is
purely UX: a QR-code or pairing flow to get the LAN address + token onto the phone
without typing it manually, and a settings toggle to bind Core non-loopback safely. That
is a small, contained Phase 7 candidate on its own — much smaller than "build sync."

**What genuinely doesn't exist**: the cross-*network* case (phone on cellular, laptop
asleep or on a different network entirely) — the scenario the user is actually asking
about. This needs one of:

1. **A relay/tunnel** (Tailscale-style or a CID-operated equivalent) that makes the
   laptop's Core reachable from anywhere without a public IP or port-forwarding — the
   laptop stays the source of truth, nothing is duplicated or migrated, sync in the
   colloquial sense is really "remote access," not state replication.
2. **A real hosted "CID Cloud" Core** (already named as a Phase 7+, evidence-gated
   possibility in Part 15) that mirrors/relays state so the phone can see recent history
   even when the laptop is offline — this is a materially bigger undertaking (a second
   deployment target, data retention/deletion policy, a real multi-tenant story) and is
   the natural home for a **paid tier**, exactly as suggested: self-hosting stays free
   and fully capable: a relay/hosted-sync flag is an opt-in convenience layer on top, not
   a capability gate.

**Recommended design, for whenever real demand justifies starting this**:

- An explicit `sync.enabled` flag, **off by default** — matching Part 17's "heavy
  features off by default" philosophy exactly. Self-hosted CID must remain fully
  functional with this permanently off; sync is additive, never load-bearing.
- Same-network pairing (item 1 in "what already works") ships first, independent of any
  relay/cloud decision — it's real, useful, and needs no new backend.
- The relay/hosted layer, if built, should sync *pointers and recent state* (which
  Missions exist, their status, recent messages) for the mobile approval/monitoring use
  case Part 15 already scopes mobile to — not full worktree/file content, which stays on
  the machine that has the actual git checkout. This keeps the sync surface small and
  matches mobile's existing "approval/monitoring, not full editing" non-goal (Part 1).
- Whether the hosted layer is a paid feature, a self-hostable relay anyone can run, or
  both, is a product/business decision outside this document's scope — the technical
  design above holds either way.

**Evidence gate**: real users who've outgrown same-network LAN access and are asking for
cross-network mobile visibility specifically — not a speculative "competitors have sync
so we should."

## Non-Goals

Building either of the above now. This document is design thinking with named evidence
gates, exactly like the rest of the Phase 7+ bucket (native editor, enterprise hardening,
hosted Cloud) already in `docs/041-Roadmap.md` — not a commitment.

## Architecture

Diagrams intentionally omitted — both designs above are sketches pending real demand,
not committed architecture worth diagramming yet.

## Tradeoffs

The core tradeoff running through both halves of this document: CID's actual advantage
over building a VS Code-style extension host or a bespoke sync protocol from scratch is
that its architecture (MCP client + ACP host + "One Core, Many Surfaces" over JSON-RPC)
already solves adjacent problems using real, adopted standards. The temptation in any
"make it extensible/syncable" ask is to build something CID-specific; the recommendation
throughout this document is to extend what's already standards-based instead.

## Failure Modes

N/A — no code shipped in this document.

## Security

Any future non-loopback Core bind already requires a bearer token by construction (ADR
0012) — the same-network mobile-access path described above inherits that guarantee for
free. A future relay/cloud layer would need its own threat model (in transit and at rest
for whatever state it relays) before it ships, not after.

## Testing

N/A — design document.

## Implementation Order

Not sequenced — both halves are Phase 7+, evidence-gated, and independent of each other
(themes/UI-extensions and sync are unrelated features that happen to share this
document because both were raised in the same product-design conversation).

## Acceptance Criteria

N/A — no acceptance criteria for undeferred, unscoped future work.

## AI Coding Rules

If a future Phase 7+ prompt picks up either half of this document, read it first rather
than re-deriving the analysis — in particular, do not propose a second tool-plugin
format without re-reading why MCP already covers that need.
