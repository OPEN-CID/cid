# Build Brief — opencid.dev (the live app) + doc.opencid.dev (documentation)

You are an AI coding agent with file, shell, and git access. This brief covers **two**
separate deliverables under one domain family. Read §0 first — it corrects a naming
assumption from an earlier draft of this brief that would otherwise send you the wrong
direction on where things live.

---

## 0. Domain split — read this before doing anything else

| Domain | What it is | Where it's built |
|---|---|---|
| **opencid.dev** | The **live CID product**, running in a browser — the same React/Vite web client already in this platform repo (`src/`), the one Tauri wraps for desktop and `npm run dev` serves locally. Visiting it is *using CID*, not reading about it. | **Inside the existing CID platform repo** (this repo, or wherever it lives) — not a new repo. See Part A. |
| **doc.opencid.dev** | Documentation, architecture, concepts, roadmap, blog, community — everything about the project that isn't the running app itself. | A **new, standalone repo** (`cid-docs` or similar). See Part B. |

**Do not build a third "marketing landing page" repo.** An earlier draft of this brief
had `opencid.dev` as a static marketing site and `docs.opencid.dev` as documentation, with
no home for the actual product. That's superseded: the product's own web client already
exists and is what belongs at the primary domain, so there is no separate landing site to
build — the "landing" experience is the CID app's own first-run screen (§Part A.2).

If a lightweight explainer is still wanted for people who land on `opencid.dev` with zero
context before connecting anything, that lives as the **first-run screen inside the app**
(Part A.2), not as a separate site — one domain, one visitor flow: arrive → understand in
five seconds what this is → connect to a Core → use it.

---

# Part A — opencid.dev: hosting the actual CID web client

## A.1 What already exists (verified in this repo, do not rebuild)

- `src/` is the real React/TypeScript web client. `npm run build` (`tsc && vite build`)
  already produces a static, deployable bundle — this is the *same* bundle Tauri
  desktop uses and `npm run dev` serves locally at `localhost:1420`.
- `cid-core` (the Rust/Tokio daemon) is a **separate process the browser talks to** over
  JSON-RPC 2.0 (HTTP `/api/rpc` + WebSocket `/ws`). The web client is a thin client; there
  is no server-side rendering and no product logic to reimplement.
- Core already supports being reached by a hosted web client, safely, today —
  this was purpose-built for exactly this ("Part 15, Phase 2" in `cid-core/src/access/mod.rs`):
  - `cid-core --host 0.0.0.0 --auth-token <token> --allow-origin https://opencid.dev`
    binds Core beyond loopback, **requires** a bearer token (refuses to start
    non-loopback without one — verified in `AccessPolicy::new`,
    `cid-core/src/access/mod.rs`), and adds `https://opencid.dev` to the CORS allow-list
    (`cid-core/src/main.rs`'s `--allow-origin`, repeatable).
  - Default CORS origins (loopback-only Core, the common case) are
    `http://localhost:1420`, `http://127.0.0.1:1420`, `tauri://localhost`,
    `https://tauri.localhost` — **`https://opencid.dev` is not in that default list**, so
    a hosted page can only reach a *local* Core if the user starts Core with
    `--allow-origin https://opencid.dev` (loopback Core + browser on a different origin
    still works for CORS purposes; loopback-only refers to the bind address, not who's
    allowed to call it).
- **There is no multi-tenant cloud Core.** CID does not host anyone's code or run anyone's
  agents server-side. `opencid.dev` hosting the web client does **not** mean CID-the-company
  can see your repos — every visitor points the hosted page at *their own* Core (running on
  their machine, their LAN, or a box they control). Say this plainly in the app; don't let
  it be ambiguous.

## A.2 What to build

1. **A "Connect to Core" first-run screen**, shown when the web client has no working
   Core connection configured (new component, e.g. `src/components/onboarding/ConnectCore.tsx`,
   gating the existing app shell):
   - A short, honest explainer (2-3 sentences: what CID is, that this page talks to a
     Core *you* run, that nothing is uploaded anywhere).
   - A form: Core URL (default guess `http://localhost:5919`), optional bearer token
     field (only needed if the user's Core requires one).
   - A "how do I get a Core running" panel with the exact copyable command:
     `cid-core --port 5919` for same-machine use (no `--allow-origin` needed if the
     browser is also on `localhost:1420` — but opencid.dev is a *different* origin, so
     the real instruction for opencid.dev specifically must be:
     `cid-core --allow-origin https://opencid.dev` (loopback bind, no token needed since
     it's still bound to 127.0.0.1) — verify this against current `access/mod.rs` logic
     before shipping the copy, don't assume).
   - Persist the working Core URL (and token, if any) in `localStorage`, not cookies —
     match whatever the existing desktop client already uses for its own config storage
     (check `src/lib/api.ts` and any existing settings persistence before inventing a new
     mechanism).
   - Clear, reachable "Disconnect / change Core" control once connected, not just a
     one-way setup wizard.
   - If the configured Core is unreachable (network error, CORS rejection, wrong token),
     show the *actual* error, not a generic "something went wrong" — CORS rejections in
     particular look like a silent network failure in the browser console; detect and
     explain that specific case if you can distinguish it.
2. **A deploy pipeline for the existing web client**, added to this platform repo's CI
   (not the docs repo): on push to `main` (or a `release` tag — decide based on how this
   repo already gates releases, check `.github/workflows/ci.yml` first), run
   `npm run build` and deploy `dist/` to Cloudflare Workers static assets under the
   `opencid.dev` route, via `wrangler deploy`. Add `wrangler.jsonc` at the repo root (or
   under `src/` if that reads cleaner given the existing layout — match this repo's
   conventions, don't force a structure it doesn't already have).
3. **Do not change how the desktop/Tauri build works.** This is an *additional* deploy
   target for the same `dist/` output, not a replacement — Tauri keeps bundling the app
   itself exactly as it does today.

## A.3 Honesty rules for Part A

- Never claim or imply CID has a hosted backend. Every screen must make "bring your own
  Core" unambiguous.
- Don't build a fake demo mode with canned data unless explicitly asked — if there's no
  Core connected, show the connect screen, not a simulated product tour.
- If `--allow-origin` or the CORS/token behavior doesn't work exactly as described above
  when you test it against a real running `cid-core`, **stop and report the discrepancy**
  rather than shipping onboarding copy that tells users to run a command that doesn't do
  what the copy says.

---

# Part B — doc.opencid.dev: documentation, blog, and community

This is a **new, standalone repository** (`cid-docs` or similar) — a static site with no
product/app code, no live Core connection, nothing dynamic beyond what a CDN serves.

## B.1 Ground truth about the product (for writing accurate docs)

CID is **already built and working** — 465+ passing tests, a working Rust Core, web UI,
Tauri desktop app, and TUI. Docs should be written as real documentation, not
pre-launch teaser copy.

**One-sentence description:** a chat-native, multi-agent software engineering platform —
Slack-shaped mission control for shipping code with AI agents.

**Core mental model** (use these exact terms; they are the product's real domain
language, do not paraphrase them into something else):

- **Workspace → Repo Channel → Mission Thread** — same nesting as Slack.
- A **Mission** ("Build OAuth", "Fix #245") runs in an isolated **git worktree** by
  default, or a shared clone.
- Agents: **Planner → Implementer → Reviewer** — prompt + model + tool-permission
  configurations, *not* a fixed cast of independent agents — plus ad hoc **subagents**
  for scoped parallel work.
- **Exactly three autonomy levels per Mission**, named exactly this:
  - **Manual** — you drive.
  - **Co-Pilot** — every tool call is shown and requires approval. **Default.**
  - **Autonomous** — runs against a governed, per-repo command allow-list without
    per-step approval; a human still reviews the final diff.
  - A **vibe-coding preset** skips the Planner's ceremony for quick, low-stakes changes
    (plan auto-approves so the Implementer starts immediately); Co-Pilot's per-tool-call
    approval, the diff viewer, and History stay unchanged.
- **One Core, many surfaces**: `cid-core` (Rust/Tokio) exposes everything over
  JSON-RPC 2.0 (HTTP + WebSocket). Desktop (Tauri v2), web (§Part A — same bundle), the
  CLI/TUI (`cid-tui`), and a future mobile companion are thin clients over that one API.

**Real, shipping capabilities** (safe to document as existing — verify each against the
CID repo's actual code/tests before writing the page, per the honesty rules in B.2):

| Area | Detail |
|---|---|
| Confidence Engine | 9-signal patch scoring — symbol resolution, static analysis, type validation, architecture-rule validation, test impact, duplicate detection, dependency impact, semantic similarity, existing-reuse — shown inline before approval |
| Git / diff | `git2-rs`-backed worktrees, per-hunk accept/reject, atomic auto-commits per logical change, checkpoint + rewind per Mission |
| Terminal | Real native PTY per Mission, secret redaction on by default (live view and stored history) |
| Context | Opt-in Tree-sitter structural indexing; `AGENTS.md` (Linux Foundation/AAIF standard) and `SKILL.md` (Anthropic Agent Skills) layered Workspace → Repo Channel → Mission with nearest-scope-wins; hybrid BM25 + embedding search via Tantivy |
| Model routing | Anthropic, OpenAI, Google natively; one generic OpenAI-compatible endpoint slot (OpenRouter, Groq, self-hosted vLLM, most others); hardware-gated local-model detection (Ollama / LM Studio / `llama.cpp --server`); per-role model overrides |
| Security | Least-privilege MCP servers scoped per Repo Channel; OS-native credential storage; multi-user local auth (Argon2id; Viewer < Reviewer < Developer < Admin < Owner); workspace governance over Autonomous mode, per repo, with spend caps |
| Integrations | GitHub, GitLab, Bitbucket (issue/PR bridges); Jira and Linear (ticket linkage); Slack and Microsoft Teams bridges |
| Observability | Repository-health dashboard, Prometheus-style `/metrics`, local secret-redacted crash log |

**Deliberate scope boundaries** — document as intentional, not as gaps:

- No deployment-provider integrations. No "Deploy to X" buttons. Permanent decision.
- The repo-health dashboard is **signal-based** (test presence, duplicate-test
  detection) — **not** instrumented line coverage. State this plainly.
- On Windows, Autonomous-mode sandboxing has a command allow-list and path policy, but
  Windows Job Objects **do not confine the filesystem** at kernel level the way
  `sandbox-exec` (macOS) and `bubblewrap` (Linux) do. Document honestly, don't smooth over.

**Platform status — encode exactly, do not upgrade any row:**

| Surface | Status | Docs may say |
|---|---|---|
| Web (opencid.dev / self-hosted) | **Working** | Full instructions — link to opencid.dev, explain the "bring your own Core" model from Part A |
| Desktop (Tauri v2, Windows + macOS) | **Working, builds from source** | Build-from-source steps. No signed installers published yet — no live download buttons pointing at files that don't exist |
| CLI / TUI (`cid-tui`) | **Working** | Full instructions |
| Linux desktop | Buildable, less tested | Mark clearly as such |
| Mobile companion | **Not built yet** (planned: approval/monitoring only, never full editing) | Roadmap only — no app-store badges, no fake links |

Current version: **0.1.0**. No published release, no code signing, no package-manager
distribution (no Homebrew formula, no winget, no `cargo install` from crates.io) yet.

## B.2 Honesty rules (non-negotiable)

1. **Never fabricate a screenshot.** Build real HTML/CSS mockups if imagery is needed,
   clearly a design representation — never a fabricated "screenshot" PNG.
2. **Never publish a download link that 404s.** A disabled "not yet published — build
   from source" state is correct; a live-looking dead button is not.
3. **Never document a CLI flag, RPC method, or config key you haven't verified** against
   the CID source. If unverifiable, write a stub with `TODO(verify)` and list it in your
   final report.
4. **Do not oversell autonomy.** The product centers on human approval gates — match that
   tone. Never "AI writes your whole codebase while you sleep."
5. **Do not fabricate metrics** — no invented benchmarks, user counts, or testimonials.
   Star counts must be fetched live at build time from the GitHub API, or omitted.
6. **Do not claim CID hosts anyone's code.** Every mention of `opencid.dev` in the docs
   must be consistent with Part A.1's "bring your own Core" model.

## B.3 Repository layout

```
cid-docs/
├── src/
│   ├── content/
│   │   ├── docs/               # Starlight-managed
│   │   └── blog/
│   ├── pages/
│   │   └── community.astro     # plain Astro page, outside Starlight
│   ├── components/
│   ├── data/
│   └── styles/
├── public/assets/
├── astro.config.mjs
├── wrangler.jsonc
├── .github/workflows/ci.yml
├── README.md
└── LICENSE                     # MIT, matching the platform repo
```

One Astro project; Starlight owns `/` (the docs home) and `/docs/*`. Blog and Community
are plain Astro pages in the same project so they share the design system without
fighting Starlight's layout for pages that aren't reference docs.

Deploy to Cloudflare Workers static assets under the `doc.opencid.dev` route — same
hosting mechanism as Part A, separate `wrangler.jsonc`/deploy, separate repo.

## B.4 Tech stack (use exactly this)

| Concern | Choice |
|---|---|
| Framework | Astro (latest stable), TypeScript strict |
| Docs | Starlight (`@astrojs/starlight`) |
| Styling | Tailwind CSS (official Astro integration) |
| Hosting | Cloudflare Workers, static assets, `wrangler.jsonc` |
| Search | Pagefind (ships with Starlight) |
| Icons | Lucide |
| Content | Markdown; MDX only where a page genuinely needs an interactive component |
| Analytics | Cloudflare Web Analytics — cookieless, no consent banner |
| CI | GitHub Actions |
| Package manager | npm (lockfile committed) |

**Do not add:** a React/Vue/Svelte runtime for anything static HTML can do, CSS-in-JS, a
component library, a heavy animation library, a CMS, or cookie-setting analytics.

## B.5 Design direction

Same audience as the product itself: engineers who live in dark-mode editors.

- **Dark-mode-first**; light mode is the toggle. Respect `prefers-color-scheme` on first
  visit, persist explicit user choice.
- One restrained accent color. No gradient meshes, no 3D blobs, no glassmorphism.
- Monospace used deliberately for code/terminal/diff-flavored content; a clean sans for
  body copy (system stack or one self-hosted variable font — no runtime Google Fonts).
- Systematic type scale and spacing as Tailwind theme tokens, not ad hoc per-component values.

## B.6 Page-by-page specification

### Docs home (Starlight-managed, `doc.opencid.dev/`)
A short landing inside the docs shell: one-line thesis, primary CTA = **"Open the app"**
linking to `opencid.dev` (not a fake download button) and a secondary CTA = "Star on
GitHub." A compact "why not just use an in-editor assistant" honesty section — chat-native
and thread-shaped, worktree isolation per Mission, explicit approval gates, one Core
across surfaces. Describe what CID chose and why; do not disparage named competitors.

### `/docs/*` sidebar structure — build as titled stub pages first, fill after

- **Getting Started** — What is CID · Using the web app (opencid.dev + your own Core,
  cross-link to Part A's connect flow) · Installing the desktop app · Installing the
  CLI/TUI · Your first Mission · Connecting a model provider
- **Concepts** — Workspaces, Repo Channels & Missions · Worktrees & isolation ·
  Autonomy levels (Manual / Co-Pilot / Autonomous) · Agents & subagents · Confidence
  Engine (the 9 signals) · Context Engine (`AGENTS.md`, `SKILL.md`, indexing, search) ·
  Model routing & per-role overrides
- **Guides** — Reviewing a diff per hunk · Checkpoints & rewind · Using the terminal ·
  MCP servers per Repo Channel · Autonomous-mode allow-lists · Spend caps · Running your
  own Core for opencid.dev (the `--host`/`--auth-token`/`--allow-origin` flags, precisely)
- **Reference** — JSON-RPC API overview · Configuration · CLI/TUI · `/metrics`
- **Security** — Threat model · Credential storage · Sandboxing (including the Windows
  filesystem-confinement limitation, stated plainly) · Reporting a vulnerability
- **Project** — Roadmap · Scope boundaries · Contributing · FAQ

Where the CID repo's own `docs/000`–`049` series already covers a topic, transcribe and
format it — it is authoritative. Note the source doc per page in an HTML comment.

### `/blog` — seed with three real posts
1. **Why CID exists** — the product thesis.
2. **The autonomy model** — Manual / Co-Pilot / Autonomous, why approval gates are the
   default, what Autonomous actually governs.
3. **One Core, many surfaces** — the JSON-RPC architecture, why `opencid.dev` is a thin
   client over a Core you run yourself rather than a hosted backend.

Real design-decision writing. No filler, no listicles, no SEO padding.

### `/community`
GitHub link, live-fetched star/contributor counts (build time, or omit), Discussions,
Code of Conduct, Contributing, security policy. Only link channels that actually exist.

**Do not build:** a pricing page, a changelog (no releases yet), a newsletter signup, a
testimonials section, or any page duplicating what's now inside the app itself (Part A).

## B.7 Quality bars

**Accessibility** — WCAG 2.1 AA: semantic landmarks, one `h1` per page, correct heading
order, visible focus rings, 4.5:1 contrast in both themes, full keyboard reachability,
`prefers-reduced-motion` respected, real `alt` text. Verify with axe.

**Performance** — Lighthouse ≥ 95 (Performance, Accessibility, Best Practices, SEO) on
the docs home and one inner docs page. No render-blocking third-party requests.
Self-hosted fonts, `font-display: swap`. Images as AVIF/WebP with explicit dimensions.
Target < 100 KB JS on the docs home.

**SEO** — unique title + meta description per page, canonical URLs, Open Graph + Twitter
cards, `sitemap.xml`, `robots.txt`, blog RSS, JSON-LD on the docs home. Programmatic OG
images (`astro-og-canvas` or Satori) for every page.

## B.8 CI/CD

**On PR:** install → `npx tsc --noEmit` → lint → `npm run build` → internal link check
(fail on any broken internal link) → Lighthouse CI against the built output.

**On merge to `main`:** the above, then `wrangler deploy` to Cloudflare Workers using
`CLOUDFLARE_API_TOKEN` / `CLOUDFLARE_ACCOUNT_ID` repo secrets. Confirm `sitemap.xml` and
the blog RSS feed exist in the deployed output. Never commit secrets.

## B.9 Build order

1. Scaffold Astro + Starlight + Tailwind + `wrangler.jsonc`. **Deploy a near-empty site
   to a `*.workers.dev` URL immediately** to prove the pipeline before writing content.
2. Wire GitHub Actions; confirm a PR run and a deploy run both pass.
3. Build the design system: theme tokens, dark/light toggle, header, footer, layout.
4. Create the **entire docs sidebar as stub pages** with correct titles/nesting. Stop and
   report the structure before filling content.
5. Build the docs home (§B.6).
6. Fill docs content, transcribing from the CID repo's `docs/` series where it exists.
7. Write the three blog posts.
8. Build `/community`.
9. Accessibility + Lighthouse + link-check pass; fix everything failing §B.7.
10. Connect `doc.opencid.dev`, enable Cloudflare Web Analytics, verify routing in production.

---

## Final report (both parts)

Report: what you built, what you deliberately left as a stub and why, every
`TODO(verify)` left with file and line, anything you could not verify against the CID
source, and any deviation from this brief with the reason. **Report failures and gaps
plainly. Do not describe unfinished work as complete.**

For Part A specifically, report the *actual* observed behavior when you tested
`--allow-origin`/`--auth-token` against a real running `cid-core`, not just what this
brief predicted it would do — if they diverge, the docs and onboarding copy must match
reality, and this brief's prediction is the thing that's wrong.
