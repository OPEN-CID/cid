# 016 — Git Engine

## Vision

Isolated worktrees per Session by default, so AI-authored edits never touch the main
branch until a human explicitly merges — the mechanic every agent-multiplexer tool in
`001-Competitive-Analysis.md` validates, built on `git2-rs`.

## Goals

- **Worktree lifecycle**: `git.worktree.{create,list,remove}` — `cid-core/src/git/mod.rs`.
  Created on Session start (`cid/<session-slug>` branch), removable on close.
- **Shared clone mode**: the alternative Isolation for solo, sequential work — no
  worktree overhead for repos where it's impractical.
- **Diff**: `git.diff`, structured per-file/per-hunk, backing per-hunk accept/reject
  (`012-Semantic-Editing.md`).
- **Commit discipline**: atomic per-logical-change commits (`git.commit`), Aider's
  pattern, not one giant end-of-Session commit.
- **Status polling**: file-watcher-triggered `git.diff.update` notifications, not
  continuous polling from the client.

## Non-Goals

Full merge/rebase conflict resolution UI — CID surfaces that a branch is behind base and
offers a rebase-and-recheck action (Part 10), not a full three-way merge editor.

## Architecture

`GitManager` wraps `git2-rs` for all operations, including writes (push, merge, worktree
management) — `gix` (gitoxide) considered opportunistically for hot read paths only if
profiling showed `git2` was a bottleneck (ADR 0002); no such profiling evidence exists
yet, so `git2-rs` remains the sole backend.

## Data Structures

`GitStatusFile`, `GitDiff`, worktree listing structures (`api/types.rs`).

## Traits / Interfaces

RPC: `git.{status,diff,commit,log,hunk.apply}`, `git.worktree.{create,list,remove}`.

## Storage Layout

Worktrees live under `<repo>/.cid/worktrees/<session-id>` by default (configurable via
`worktree_root` setting), auto-gitignored on `repo.connect`.

## Performance Targets

`git status` on a 50-file repo: 2.99ms in this environment
(`git_status_is_fast_on_a_small_repo`) — comfortably inside Part 17's "feels instant"
budget.

## Tradeoffs

`git2-rs` over `gitoxide` — ADR 0002's decision, re-verified in Phase 5's dependency audit
(`045-Dependency-Audit.md`) and still correct as of that audit: gitoxide's own
crate-status docs still list push/merge/rebase as under development.

## Failure Modes

Worktree creation is all-or-nothing — verified by the property test
`worktree_creation_is_all_or_nothing`: either a real worktree directory results, or none
does, never a partial state. Removing an already-removed worktree is idempotent
(`removing_a_worktree_is_idempotent`). A duplicate create attempt cannot corrupt an
existing worktree's contents (`creating_the_same_worktree_twice_does_not_corrupt_the_first`).

## Security

Worktree paths are always resolved relative to the managed root; the sandbox boundary
(`cid-core/src/sandbox/mod.rs`, `031-Security.md`) additionally confines Autonomous-mode
command execution to the Session's own worktree.

## Testing

11 property tests in `cid-core/tests/worktree_property.rs` (Part 21's Phase 3+ floor,
using `proptest` for 3 of them), covering creation, removal, duplicate handling, sibling
isolation, and parent-repo survival under churn.

## Implementation Order

Worktree/shared-clone modes (Phase 0) → no structural change through Phase 4; the
property-test suite (Phase 3) was added to an already-stable implementation, which is
itself evidence the design held up.

## Acceptance Criteria

A Session's worktree is created inside the managed root
(`worktrees_are_created_inside_the_managed_root`), and worktree churn never damages the
parent repository (`the_parent_repo_survives_worktree_churn`).

## AI Coding Rules

Any change to `create_worktree`/`remove_worktree` must be re-verified against
`cargo test -p cid-core --test worktree_property` — these are the tests that would catch
a regression to a half-created or corrupted worktree state.
