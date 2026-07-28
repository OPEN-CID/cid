# ADR 0002: git2-rs as default git backend

- **Date**: 2026-07-26
- **Status**: Accepted
- **Context**: Build Prompt v1 proposed "gitoxide preferred, git2 fallback" without criteria. v2 flagged gitoxide can't push as settled fact. v3.0 re-verifies against live sources mid-2026: gitoxide's own crate-status docs and maintainer discussion still list push, full merge workflows, rebase, hooks as "under development". Maintainers themselves point people to git2 for anything beyond read-heavy workflows. Some SEO content claims full push support — primary source doesn't back it up.
- **Decision**: Use `git2` (libgit2 bindings) 0.19 with vendored libgit2+openssl as default engine for all operations including writes (status, diff, commit, worktree create/remove, log). `gix` (gitoxide) may be used opportunistically for hot read paths (status polling, diff computation) only if profiling shows git2 is bottleneck — not a hard dependency Phase0.
- **Alternatives**:
  - gitoxide as primary: would require fallback to CLI for push/merge/rebase anyway, increases complexity, risk of incomplete workflows
  - CLI git spawning: simple but loses libgit2 performance and error handling via Result, also requires git binary present
- **Consequences**:
  - Diff parsing implemented via `diff.foreach` with file_cb, hunk_cb, line_cb to build per-hunk structures for per-hunk accept/reject UI (Cline/Aider pattern)
  - Worktree operations via `repo.worktree` + `worktree.prune`
  - Vendored features increase binary size but improve Windows compatibility (no OpenSSL DLL hell)
  - Need to handle libgit2 threading: git2 is not fully Send/Sync, use spawn_blocking for blocking ops
- **References**: Build Prompt Part 2 competitive table, Part 4 repo model, Part 10 diff workflow, Part 18 tech stack.
