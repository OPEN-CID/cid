# 053 — Production-Readiness Review (2026-08-10)

Status of the review pass that asked a direct question: **is CID ready to hand to a
real user?** The honest answer at the start of this pass was *no*, and the reasons are
worth recording precisely, because none of them were visible from the test suite.

This document is the spec/report for that pass. It supersedes nothing; it sits
alongside `050-Gold-Standard-Review.md` and `051-Editor-Excellence-Roadmap.md` and
follows the same rule they do — **every claim here was checked against running code,
not against a previous document's claims.**

---

## 0. Scope

Driven by a product review of the actual UX, not a code audit:

1. A Mission required a task description; it should be optional.
2. There was no way to pick a repo folder — you had to type an absolute path.
3. There was no model selection anywhere in the Mission flow.
4. The model catalog was stale (Claude 3.5-era ids that no longer resolve).
5. Dummy/simulated implementations and placeholder data should be gone.

Items 1–4 were implemented by delegated agents. **This pass is the verification and
remediation on top of that work**, and it is where most of the findings below came
from — including several defects in the newly-added code itself.

---

## 1. The headline: seven defects, none caught by 707 passing tests

Every item in this table was found by running the real binary and the real UI and
looking at what actually happened. All were fixed in this pass.

| # | Defect | Why the suite missed it |
|---|---|---|
| 1 | **Every existing install kept the retired model id.** The catalog refresh changed the schema `DEFAULT`, the seed `INSERT`, and an in-memory fallback — none of which touch an already-existing row. `settings.get` on the real machine still returned `claude-3-5-sonnet-20241022`, which 404s. | Every test builds a fresh database. The *upgrade* path had zero coverage. |
| 2 | **`fs.list_dirs` + `repo.connect` created duplicate repo rows.** The new folder picker returns `canonicalize`'s `\\?\C:\Projects\cid`; the text box returns `C:\Projects\cid`. `connect_repo` stored both verbatim on a `UNIQUE` column. | Both components were individually correct and individually tested. The bug lived only in the seam — and only on Windows. |
| 3 | **The model picker rendered every option disabled.** The RPC returns `available`; the frontend type and both new pickers read `enabled`. `!undefined` is `true`, so *no model could be selected* — the exact feature being added. | The test fixtures encoded the invented field name, so they agreed with the component instead of with the wire. TypeScript could not help: `call()` returns an asserted type, not a validated one. |
| 4 | **`repo.disconnect` was broken for any repo that had ever been used.** A bare `DELETE FROM repo_channels` against a live `missions.repo_channel_id` foreign key — it failed with `FOREIGN KEY constraint failed` 100% of the time once a Mission existed. | Nothing tested disconnect with dependent rows. Found by trying to use it. |
| 5 | **E2E runs wrote into the developer's real database.** `playwright.config.ts` started Core with no `--db`, so it fell back to `%APPDATA%/cid/cid.db`. A real install had accumulated **15 dead `test-repo`/`e2e` channels** that — because of #4 — the UI could not remove. | The tests passed. They were simply pointed at the wrong database, and nothing asserted otherwise. |
| 6 | **An unconfigured provider fabricated a completed turn.** With no API key, Core wrote an *Assistant* message reading *"here's a simulated response… I would have: 1. Analyzed the repo, 2. Checked AGENTS.md…"*, dumped the whole settings config into the chat, and set the Mission to **`Review`** — as if work had been done and was awaiting inspection. This was the first turn a new user without a key would ever take. | No test asserted on the no-key path's content or resulting status. |
| 7 | **A worktree Mission on CID's own repo hard-reloaded the dev UI.** The worktree materializes a full repo copy under `.cid/worktrees/<id>/`, including a `tsconfig.json`; Vite detected a "changed tsconfig", forced a full reload, and wiped the in-memory store mid-Mission. | Dev-server behavior. Invisible to both unit and component tests. |

### What changed

- **#1** — Four `UPDATE` migrations rewrite retired `claude-3-*`/`claude-2*`/`claude-instant*`
  ids across `anthropic_model` and the three per-role override columns, mapping each to
  its own tier (opus→opus-5, haiku→haiku-4-5, else sonnet-5). 4.x ids are deliberately
  **not** touched — they may still resolve, and guessing would be the same
  unverifiable behavior this project warns about. Verified on the real `cid.db`:
  `claude-3-5-sonnet-20241022` → `claude-sonnet-5`.
- **#2** — New `path_confine::normalize_stored_path` (backed by `dunce`, already in the
  lock tree) applied at `Persistence::connect_repo` — the single storage boundary, so
  the invariant holds regardless of entry route. `fs.list_dirs` also returns the
  simplified spelling. **The regression test was verified to fail without the fix**
  (two distinct UUIDs for one directory), and it asserts its own Windows precondition
  so it cannot pass vacuously on Linux.
- **#3** — `ModelInfo.enabled` → `available` in the type and both pickers; fixtures
  rewritten to the real wire shape and a missing "the available one is *not* disabled"
  assertion added. Verified the updated test fails when the field name is wrong.
- **#4** — `disconnect_repo` is now transactional, clearing all seven mission-scoped
  tables, then missions, then the channel. Nothing on disk is touched: disconnecting
  forgets a repo, it does not delete the user's code.
- **#5** — New `dev:core:e2e` script pointing at a disposable `.cid-e2e/` database, used
  by `playwright.config.ts` and the CI workflow; `.gitignore` updated.
- **#6** — Replaced with a `System` notice (CID speaking, not a model pretending to)
  that states only what is true and what to configure, and sets `Failed`, because the
  turn genuinely did not run.
- **#7** — `vite.config.ts` now excludes `**/.cid/**` from the watcher.

---

## 2. Verified live, not just tested

Against a real `cid-core` on `:5919` holding the real `cid.db`, driven through the
actual UI in a real browser (Playwright, 11/11 checks, console-clean):

- Core connects; the footer status dot reflects real connection state.
- The folder picker browses the real filesystem via `fs.list_dirs` and shows no
  `\\?\` paths.
- Connecting the same directory as `\\?\C:\Projects\cid` and as `C:\Projects\cid`
  returns **the same repo channel id** — one row.
- The center header shows the real repo name and mission title.
- Task Description is labelled optional; **Create Mission is enabled with it empty**,
  and the created Mission's `task_description` falls back to the title.
- The model dropdown lists 20 models with **9 genuinely selectable**; the one chosen in
  the UI (`google/gemini-1.5-pro`) is what actually persisted on the Mission.
- An empty title is refused server-side.
- `mission.create` honors a per-mission model override, and it reaches the wire (proved
  by a mock server that captures the `model` field, not merely by the code compiling).

The user's install was also cleaned: 15 dead test channels removed, leaving one real
repo. A backup was taken first (`%APPDATA%/cid/backup-<timestamp>/`).

---

## 3. Gates

All run for real on this machine after the final change:

| Gate | Result |
|---|---|
| `cargo fmt --all -- --check` | clean |
| `cargo clippy --workspace --all-targets -- -D warnings` | clean |
| `cargo test --workspace --exclude cid --all-features` | **550 passed**, 1 ignored (the network-dependent real-embeddings test) |
| `npx tsc --noEmit` | clean |
| `npm run lint` (`--max-warnings 0`) | clean |
| `npx vitest run` | **181 passed**, 29 files |

Rust tests went 526 → 550; the additions are regression coverage for every defect above.

Two operational notes for anyone re-running these on Windows, both already in `CLAUDE.md`:
the `performance_budget` test binary hit the transient WDAC block (`os error 4551`) and
needed deleting from `target/debug/deps/` before it would run — it is not a test failure —
and Core must be stopped before a rebuild, since a running `cid-core.exe` locks its binary.

---

## 3a. Follow-up: the model catalog is now live, not hardcoded

§4 originally listed "`OPENAI_MODELS` and `GOOGLE_MODELS` are stale — verify before
release" as open. Verifying them against the [models.dev](https://models.dev) registry
(the open catalog `opencode` uses) and OpenRouter's `/api/v1/models` found the hand-written
arrays were wrong in three ways at once:

| Finding | Impact |
|---|---|
| Google offered `gemini-1.5-pro` / `-flash` / `-flash-8b` / `-pro-002` — **none of which exist any more**, and `gemini-1.5-pro` was flagged `default` | The picker's headline Google option was guaranteed to 404 on first use. Confirmed live: it was being served with `available: true`. |
| OpenAI's list stopped at `gpt-4o`/`o1` | Nothing from the `gpt-5.x` generation was offered at all. |
| `claude-sonnet-5` priced at the sonnet-tier **$3/$15**; it is actually **$2/$10** | Every spend estimate and every governance cap decision on what the §1 migration had just made *the default model* was 50% high. |
| `context_window_tokens` returned a flat `200_000` for every Anthropic model | `opus-5`/`sonnet-5` have a 1M window, so compaction fired at 140k — summarizing away context the model could still hold. |

Fixed by removing the hand-maintained lists entirely:

- **`cid-core/src/model/catalog.rs`** — three layers, so any failure degrades rather than
  breaks: a live registry fetch on startup → a disk cache (`model-catalog.json`, 24h TTL)
  → a snapshot compiled into the binary. The fetch is fire-and-forget and never blocks a
  turn; offline, the cached or bundled catalog stands and a warning is logged.
- **`scripts/generate-model-catalog.mjs`** — generates the bundled snapshot, following
  this repo's existing `generate-theme-css.mjs` convention (`npm run models:generate` /
  `models:check`). Selection is principled rather than name-matched: the registry's own
  `tool_call` flag and text-only output, since a model that cannot call a tool is useless
  as a CID agent and image/TTS/embedding entries are noise in a picker.
- `estimate_cost_usd` and `context_window_tokens` now read real per-model figures, falling
  back to family heuristics (never to `$0`, which would silently defeat spend caps) for a
  custom or brand-new id.

Verified live against a running Core: `model.list` serves 34 models, 10 per first-party
provider, no retired ids, and the previously-absent `gpt-5.x` and `gemini-3.x` families
present with correct context windows.

**One bug in this work was caught only by re-running the real binary**, exactly the pattern
this document is about: the per-provider cap was applied when parsing the registry but not
when loading the disk cache, so a Core that had already cached the uncapped list kept
serving 37 OpenAI models after the "fix". Now the cache carries a `schema_version` and is
discarded when it does not match, with the cap re-applied on load.

`models:check` is deliberately **not** a required CI gate. The runtime prefers the live
registry, so snapshot drift is not a correctness failure — only the offline fallback ages.
A blocking check that fails whenever a vendor ships a model would be noise, not signal.

## 4. Known limitations — stated, not buried
- **`normalize_stored_path` is best-effort.** A path that doesn't exist can't be
  canonicalized, so it is stored as given. The same directory can therefore still take
  two rows if first connected while missing and again once it exists.
- **E2E database isolation is defeated by `reuseExistingServer: true`.** If your real
  Core is already running on 5919, the suite reuses it and writes there. Stop it first
  for a genuinely isolated run. Kept deliberately, because CLAUDE.md documents
  manually-starting Core as the workaround for a separate Windows issue.
- **The `Dockerfile` is still not build-verified, and this machine cannot verify it.**
  Docker Desktop 29.6.2 is installed, but the daemon will not start: `HypervisorPresent`
  is `False` and WSL is not installed, so there is no backend for it to run on. Fixing
  that needs CPU virtualization enabled in **BIOS/UEFI** plus an elevated
  `wsl --install` and a reboot — none of which an agent can do. Stated plainly rather
  than reported as "verified".

  What *was* verified without a daemon, and what it turned up:
  - Both base image tags resolve in the registry (`rust:1-bookworm`,
    `debian:bookworm-slim` → HTTP 200; a deliberately fake tag returns 404, so the check
    is real rather than a false pass).
  - Every `COPY` source exists, and the three copied directories cover all three
    workspace members (`cid-core`, `cid-tui`, `src-tauri`), so `cargo build -p cid-core`
    can resolve the workspace. `.dockerignore` excludes nothing the build needs.
  - **A real defect, found by inspection:** `VOLUME ["/home/cid/data"]` declared a
    directory that the image never created. Docker creates an absent volume mount point
    as `root:root`, but the container runs as the unprivileged `cid` user — so the
    default `CMD`'s `--db /home/cid/data/cid.db` would have failed with a permission
    error on the very first start. The image now creates and `chown`s that directory
    before dropping privileges. This is a static-analysis fix; it removes a certain
    failure but is **not** a substitute for actually building and booting the image.
- **`fs.list_dirs` is not path-confined**, by design — it exists to find the repo to
  connect. It returns directory names only, never file contents. See `SECURITY.md` §6.
- **Disconnecting a repo does not remove its git worktrees from disk.** They remain
  under `.cid/worktrees/`; `git worktree prune` is still a manual step.

---

## 5. Is it ready?

For a single technical user running Core locally, connecting their own repos: the
flows in §2 now genuinely work end to end, and that was not true at the start of this
pass.

The gap this pass keeps illustrating is the one `CLAUDE.md` opens with. Seven defects,
all in code that was written, tested, and reported as complete — and every one of them
surfaced within minutes of running the actual product. **The test suite is necessary
and is not sufficient.** Before any release, run the binary and click the thing.

Specifically still open before calling it production-ready: verify the OpenAI and
Google catalogs (§4), and build-verify the container image.
