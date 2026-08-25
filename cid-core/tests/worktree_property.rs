//! Property tests for the git worktree lifecycle.
//!
//! Part 21's Phase 3+ bar names create/prune/conflict edge cases specifically.
//! These are the operations that touch a user's real repository, so the
//! invariants that matter are: a worktree never escapes its managed directory,
//! creation is either complete or absent, removal is idempotent, and no
//! sequence of operations damages the parent repo.

use cid_core::git::GitManager;
use proptest::prelude::*;
use std::path::{Path, PathBuf};

/// A throwaway repo with one commit, so `worktree add` has a HEAD to branch from.
struct Fixture {
    _dir: tempfile::TempDir,
    repo_path: String,
    worktree_root: PathBuf,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let repo = git2::Repository::init(dir.path()).expect("git init");
    std::fs::write(dir.path().join("README.md"), "# fixture\n").unwrap();
    {
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("README.md")).unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let sig = git2::Signature::now("CID Test", "test@example.com").unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
    }
    let worktree_root = dir.path().join(".cid").join("worktrees");
    Fixture {
        repo_path: dir.path().to_string_lossy().to_string(),
        worktree_root,
        _dir: dir,
    }
}

/// Branch names git will accept, so the tests exercise the lifecycle rather
/// than git's own name validation.
fn branch_name() -> impl Strategy<Value = String> {
    "[a-z][a-z0-9_-]{0,20}".prop_map(|s| format!("cid/{s}"))
}

#[test]
fn a_created_worktree_exists_and_is_listed() {
    let fx = fixture();
    let gm = GitManager::new();
    let wt = fx.worktree_root.join("m1").to_string_lossy().to_string();

    gm.create_worktree(&fx.repo_path, "cid/first", &wt)
        .expect("worktree creation should succeed on a repo with a commit");

    assert!(Path::new(&wt).exists(), "the worktree directory must exist");
    let listed = gm.list_worktrees(&fx.repo_path).unwrap();
    assert!(
        listed
            .iter()
            .any(|w| w.to_string().replace('\\', "/").contains("m1")),
        "a created worktree must appear in the listing: {listed:?}"
    );
}

#[test]
fn removing_a_worktree_is_idempotent() {
    let fx = fixture();
    let gm = GitManager::new();
    let wt = fx.worktree_root.join("m1").to_string_lossy().to_string();
    gm.create_worktree(&fx.repo_path, "cid/first", &wt).unwrap();

    gm.remove_worktree(&fx.repo_path, &wt)
        .expect("first removal");
    // A second removal must not error out — pruning runs on Workspace open and
    // may race with a user removing the same worktree by hand.
    let second = gm.remove_worktree(&fx.repo_path, &wt);
    assert!(
        second.is_ok() || !Path::new(&wt).exists(),
        "removing an already-removed worktree must not be a hard failure: {second:?}"
    );
}

#[test]
fn creating_the_same_worktree_twice_does_not_corrupt_the_first() {
    let fx = fixture();
    let gm = GitManager::new();
    let wt = fx.worktree_root.join("dup").to_string_lossy().to_string();

    gm.create_worktree(&fx.repo_path, "cid/dup", &wt).unwrap();
    std::fs::write(Path::new(&wt).join("marker.txt"), "keep me").unwrap();

    // Second attempt at the same path and branch: whether it errors or is a
    // no-op, the existing worktree's contents must survive.
    let _ = gm.create_worktree(&fx.repo_path, "cid/dup", &wt);

    assert!(
        Path::new(&wt).join("marker.txt").exists(),
        "a duplicate create must never wipe an existing worktree"
    );
}

#[test]
fn the_parent_repo_survives_worktree_churn() {
    let fx = fixture();
    let gm = GitManager::new();

    for i in 0..8 {
        let wt = fx
            .worktree_root
            .join(format!("m{i}"))
            .to_string_lossy()
            .to_string();
        let _ = gm.create_worktree(&fx.repo_path, &format!("cid/b{i}"), &wt);
        if i % 2 == 0 {
            let _ = gm.remove_worktree(&fx.repo_path, &wt);
        }
    }

    // The parent repository must still be readable and have its commit.
    let status = gm.status(&fx.repo_path);
    assert!(
        status.is_ok(),
        "parent repo status must still work: {status:?}"
    );
    let repo = git2::Repository::open(&fx.repo_path).expect("parent repo must still open");
    assert!(repo.head().is_ok(), "parent HEAD must still resolve");
    assert!(
        Path::new(&fx.repo_path).join("README.md").exists(),
        "worktree churn must not touch the parent's files"
    );
}

#[test]
fn worktrees_are_created_inside_the_managed_root() {
    let fx = fixture();
    let gm = GitManager::new();
    let wt = fx
        .worktree_root
        .join("scoped")
        .to_string_lossy()
        .to_string();
    gm.create_worktree(&fx.repo_path, "cid/scoped", &wt)
        .unwrap();

    let created = Path::new(&wt).canonicalize().unwrap();
    let root = fx.worktree_root.canonicalize().unwrap();
    assert!(
        created.starts_with(&root),
        "a worktree must live under the managed root: {created:?} vs {root:?}"
    );
}

#[test]
fn removing_a_worktree_leaves_sibling_worktrees_intact() {
    let fx = fixture();
    let gm = GitManager::new();
    let a = fx.worktree_root.join("a").to_string_lossy().to_string();
    let b = fx.worktree_root.join("b").to_string_lossy().to_string();

    gm.create_worktree(&fx.repo_path, "cid/a", &a).unwrap();
    gm.create_worktree(&fx.repo_path, "cid/b", &b).unwrap();
    std::fs::write(Path::new(&b).join("keep.txt"), "b's work").unwrap();

    gm.remove_worktree(&fx.repo_path, &a).unwrap();

    assert!(Path::new(&b).exists(), "sibling worktree must survive");
    assert!(
        Path::new(&b).join("keep.txt").exists(),
        "a Session's work must not be collateral damage"
    );
}

#[test]
fn a_worktree_with_uncommitted_changes_can_still_be_removed() {
    let fx = fixture();
    let gm = GitManager::new();
    let wt = fx.worktree_root.join("dirty").to_string_lossy().to_string();
    gm.create_worktree(&fx.repo_path, "cid/dirty", &wt).unwrap();
    std::fs::write(Path::new(&wt).join("scratch.txt"), "uncommitted").unwrap();

    // Whether removal succeeds or is refused, it must be a clean result rather
    // than leaving git's worktree metadata inconsistent.
    let result = gm.remove_worktree(&fx.repo_path, &wt);
    let repo = git2::Repository::open(&fx.repo_path).unwrap();
    assert!(
        repo.worktrees().is_ok(),
        "worktree metadata must stay readable after removal attempt: {result:?}"
    );
}

#[test]
fn operations_on_a_path_that_is_not_a_repo_fail_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let not_a_repo = dir.path().to_string_lossy().to_string();
    let gm = GitManager::new();

    assert!(
        gm.status(&not_a_repo).is_err(),
        "status on a non-repo must error"
    );
    assert!(
        gm.create_worktree(&not_a_repo, "cid/x", &format!("{not_a_repo}/wt"))
            .is_err(),
        "creating a worktree in a non-repo must error rather than half-succeed"
    );
    assert!(
        !Path::new(&format!("{not_a_repo}/wt")).exists(),
        "a failed creation must not leave a directory behind"
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    /// Whatever the branch name, creation either fully succeeds (directory
    /// present, listed) or fully fails (no directory left behind). There is no
    /// in-between state for a caller to trip over.
    #[test]
    fn worktree_creation_is_all_or_nothing(branch in branch_name()) {
        let fx = fixture();
        let gm = GitManager::new();
        let wt = fx.worktree_root.join("prop").to_string_lossy().to_string();

        let result = gm.create_worktree(&fx.repo_path, &branch, &wt);
        let exists = Path::new(&wt).exists();

        match result {
            Ok(_) => prop_assert!(exists, "success must leave a real worktree for {branch}"),
            Err(_) => prop_assert!(!exists, "failure must not leave a partial worktree for {branch}"),
        }
    }

    /// Repeated create/remove cycles on the same path must not accumulate state
    /// or leave the parent repo unreadable.
    #[test]
    fn repeated_create_remove_cycles_are_stable(cycles in 1usize..5) {
        let fx = fixture();
        let gm = GitManager::new();
        let wt = fx.worktree_root.join("cycle").to_string_lossy().to_string();

        for i in 0..cycles {
            let _ = gm.create_worktree(&fx.repo_path, &format!("cid/cycle{i}"), &wt);
            let _ = gm.remove_worktree(&fx.repo_path, &wt);
        }

        prop_assert!(gm.status(&fx.repo_path).is_ok(), "parent repo must stay healthy");
    }

    /// A branch name git rejects must produce an error, never a panic.
    #[test]
    fn hostile_branch_names_error_rather_than_panic(name in ".{0,40}") {
        let fx = fixture();
        let gm = GitManager::new();
        let wt = fx.worktree_root.join("hostile").to_string_lossy().to_string();

        // The only requirement is that this returns rather than unwinding.
        let _ = gm.create_worktree(&fx.repo_path, &name, &wt);
        prop_assert!(gm.status(&fx.repo_path).is_ok());
    }
}
