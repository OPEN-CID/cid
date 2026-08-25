import { useCid } from "./useCid";

/**
 * The working tree the currently selected Session actually operates in.
 *
 * A worktree Session (the default) materializes its own checkout under
 * `.cid/worktrees/<session-id>/`. The Terminal already runs there — `pty.create`
 * in `router.rs` resolves `worktree_path.unwrap_or(repo.path)` — and so does the
 * Diff panel. The Editor did not: it always opened the *main* repo checkout.
 *
 * That mismatch is why a file edited and saved in the Editor never showed up in
 * the Diff list. The edit was real, it just landed in a different working tree
 * from the one being diffed — and therefore outside the Session entirely, so it
 * would never be captured by its auto-checkpoint, reviewed, or merged with the
 * agent's work.
 *
 * Every panel that reads or writes *the Session's files* must resolve the path
 * through here rather than reaching for `repos.find(...).path` itself. Panels
 * that configure the repo as a whole (Skills, Repo health, Automation) correctly
 * use the main repo path and should not use this hook.
 */
export function useSessionRepoPath(): string | null {
  const { selectedSessionId, sessions, repos, selectedRepoId } = useCid();
  // Defaulted rather than assumed present: the store is populated
  // asynchronously after `api.connect()`, so a render before the first
  // `session.list`/`repo.list` reply legitimately sees neither array.
  const allSessions = sessions ?? [];
  const allRepos = repos ?? [];

  const session = allSessions.find((s) => s.id === selectedSessionId);
  if (session) {
    return session.worktree_path || allRepos.find((r) => r.id === session.repo_channel_id)?.path || null;
  }
  // No Session selected: the Editor is still usable as a plain file browser
  // over the connected repo.
  return allRepos.find((r) => r.id === selectedRepoId)?.path ?? null;
}
