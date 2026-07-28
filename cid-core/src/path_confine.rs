/*!
 * Shared path-confinement primitive (review_prompt.md §1.1, and its extension
 * to the network-facing `file.*` RPCs in `050-Gold-Standard-Review.md` F1).
 *
 * Both the model's file tools (`model::ExecutionContext::resolve_confined_path`)
 * and the `file.read`/`file.write`/`file.list` RPC handlers (`api::router`) need
 * the identical guarantee: a caller-supplied path, however it's spelled — `..`
 * traversal, an absolute path elsewhere, a symlink whose target leaves the
 * root — must not resolve outside an allowed root. Two independent
 * implementations of that check is exactly the duplication class
 * `CLAUDE.md` warns about (three divergent system-prompt builders, only one
 * of them wired) — so this is the single implementation both call.
 */

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

/// Resolve `requested` against `root`, refusing (a hard error, never a silent
/// clamp) anything that escapes it.
///
/// Works for paths that don't exist yet (`write_file` creating a new file,
/// possibly under new directories `create_dir_all` will make) — `canonicalize`
/// errors on a nonexistent path, so this walks up to the deepest ancestor that
/// *does* exist, canonicalizes that (resolving every symlink in it), then
/// lexically re-normalizes the full reconstructed path, which catches a
/// `..`-laden path that would only escape once its missing directories are
/// created, before any of them are actually created.
pub fn resolve_confined_path(root: &Path, requested: &str) -> Result<PathBuf> {
    let canonical_root = root
        .canonicalize()
        .with_context(|| format!("confinement root does not exist: {}", root.display()))?;

    let requested_path = Path::new(requested);
    let candidate = if requested_path.is_absolute() {
        requested_path.to_path_buf()
    } else {
        root.join(requested_path)
    };

    let mut missing_components: Vec<OsString> = Vec::new();
    let mut cursor = candidate.clone();
    while !cursor.exists() {
        let name = cursor
            .file_name()
            .ok_or_else(|| anyhow!("path escapes the confined root: {}", requested))?
            .to_os_string();
        missing_components.push(name);
        cursor = cursor
            .parent()
            .ok_or_else(|| anyhow!("path escapes the confined root: {}", requested))?
            .to_path_buf();
    }

    let canonical_existing = cursor
        .canonicalize()
        .with_context(|| format!("cannot resolve path: {}", cursor.display()))?;
    if !canonical_existing.starts_with(&canonical_root) {
        bail!(
            "path escapes the confined root: {} is outside {}",
            requested,
            root.display()
        );
    }

    let mut resolved = canonical_existing;
    for component in missing_components.into_iter().rev() {
        resolved.push(component);
    }

    let normalized = lexically_normalize(&resolved);
    if !normalized.starts_with(&canonical_root) {
        bail!(
            "path escapes the confined root: {} is outside {}",
            requested,
            root.display()
        );
    }

    Ok(resolved)
}

/// Try each candidate root in turn, returning the first that confines
/// `requested` successfully. Used where the caller doesn't (and, per the
/// RPC's existing wire contract, can't) say which connected repo a path
/// belongs to — `file.list` is called with a bare repo path on first load,
/// before the client has any per-repo context to attach.
pub fn resolve_confined_path_in_any(roots: &[PathBuf], requested: &str) -> Result<PathBuf> {
    if roots.is_empty() {
        bail!("no connected repo to confine this path against");
    }
    for root in roots {
        if let Ok(resolved) = resolve_confined_path(root, requested) {
            return Ok(resolved);
        }
    }
    bail!(
        "path is outside every connected repo: {} (checked {} root(s))",
        requested,
        roots.len()
    );
}

/// Resolve `.`/`..` components against the raw path text, without touching
/// the filesystem — used to confinement-check a path whose deepest segments
/// don't exist yet (`Path::canonicalize` requires the target to exist).
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                out.pop();
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn a_relative_in_root_path_resolves() {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("a.txt"), "hi").unwrap();
        let resolved = resolve_confined_path(root.path(), "a.txt").unwrap();
        assert_eq!(resolved, root.path().canonicalize().unwrap().join("a.txt"));
    }

    #[test]
    fn parent_traversal_is_denied() {
        let root = TempDir::new().unwrap();
        let err = resolve_confined_path(root.path(), "../../etc/passwd").unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }

    #[test]
    fn an_absolute_path_outside_root_is_denied() {
        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("secret.txt");
        std::fs::write(&target, "s").unwrap();
        let err = resolve_confined_path(root.path(), target.to_str().unwrap()).unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }

    #[test]
    fn a_new_file_under_a_missing_nested_directory_still_resolves_inside_root() {
        let root = TempDir::new().unwrap();
        let resolved = resolve_confined_path(root.path(), "new/nested/dir/file.txt").unwrap();
        assert!(resolved.starts_with(root.path().canonicalize().unwrap()));
    }

    #[test]
    fn traversal_via_a_not_yet_created_path_is_still_denied() {
        let root = TempDir::new().unwrap();
        let err = resolve_confined_path(root.path(), "new/../../outside.txt").unwrap_err();
        assert!(err.to_string().contains("escapes"));
    }

    #[test]
    fn resolve_in_any_falls_through_to_the_matching_root() {
        // A relative filename alone resolves under the first root regardless
        // (new-file paths are allowed anywhere confined), so this must use an
        // absolute path that only root_b actually contains to prove the
        // fallthrough is real and not a false pass.
        let root_a = TempDir::new().unwrap();
        let root_b = TempDir::new().unwrap();
        let target = root_b.path().join("b.txt");
        std::fs::write(&target, "b").unwrap();
        let roots = vec![root_a.path().to_path_buf(), root_b.path().to_path_buf()];
        let resolved = resolve_confined_path_in_any(&roots, target.to_str().unwrap()).unwrap();
        assert!(resolved.starts_with(root_b.path().canonicalize().unwrap()));
    }

    #[test]
    fn resolve_in_any_denies_a_path_outside_every_root() {
        let root_a = TempDir::new().unwrap();
        let root_b = TempDir::new().unwrap();
        let roots = vec![root_a.path().to_path_buf(), root_b.path().to_path_buf()];
        let err = resolve_confined_path_in_any(&roots, "/etc/passwd").unwrap_err();
        assert!(err.to_string().contains("outside every connected repo"));
    }

    #[test]
    fn resolve_in_any_with_no_roots_is_denied() {
        let err = resolve_confined_path_in_any(&[], "anything.txt").unwrap_err();
        assert!(err.to_string().contains("no connected repo"));
    }
}
