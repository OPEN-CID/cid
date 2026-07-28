/*!
 * Repository Health (Phase 6): a signal-based dashboard over the repo's own test
 * suite, not instrumented line coverage. Two honest, cheaply-computed signals:
 *
 *   - Test-to-code ratio, per crate/module, from `#[test]` fn counts vs. total
 *     `fn` counts — a proxy for "is this area tested at all," not a percentage
 *     of lines executed. Wiring real coverage (tarpaulin/llvm-cov) needs a build
 *     step this repo doesn't have yet; that's named as a real gap, not faked
 *     with a plausible-looking number.
 *   - Duplicate/near-duplicate test detection: normalizes each `#[test]` fn's
 *     body (whitespace/comments stripped) and hashes it, so two tests that
 *     assert the same thing under different names — dead weight, not extra
 *     coverage — show up as a flagged pair instead of silently inflating the
 *     test count.
 */

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleTestStats {
    pub module_path: String,
    pub fn_count: usize,
    pub test_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateTestGroup {
    pub tests: Vec<String>, // "module_path::fn_name"
    pub body_preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoHealthReport {
    pub modules: Vec<ModuleTestStats>,
    pub total_fns: usize,
    pub total_tests: usize,
    pub untested_modules: Vec<String>, // fn_count > 0, test_count == 0
    pub duplicate_test_groups: Vec<DuplicateTestGroup>,
}

const FN_RE: &str = r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+\w+";
const TEST_ATTR_RE: &str = r"(?m)^\s*#\[test\]";

/// Blanks out string-literal and comment interiors (replacing with spaces, so
/// byte offsets and line numbers are unchanged) so pattern matching below never
/// mistakes a `#[test]` inside a string fixture — such as this very module's
/// own test bodies, which quote example source containing that attribute — for
/// a real test. Handles `"..."`, `'x'`, `//...`, and `/*...*/`; does not handle
/// raw strings (`r"..."`/`r#"..."#`), which is an accepted gap for this
/// signal-based tool, not a claim of a full Rust lexer.
fn mask_non_code(content: &str) -> String {
    let mut out: Vec<u8> = content.as_bytes().to_vec();
    let bytes = content.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    if bytes[i] == b'\\' && i + 1 < bytes.len() {
                        i += 1;
                    }
                    i += 1;
                }
                i = (i + 1).min(bytes.len());
                for b in out.iter_mut().take(i).skip(start) {
                    if *b != b'\n' {
                        *b = b' ';
                    }
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                let start = i;
                while i < bytes.len() && bytes[i] != b'\n' {
                    i += 1;
                }
                for b in out.iter_mut().take(i).skip(start) {
                    *b = b' ';
                }
            }
            b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                let start = i;
                i += 2;
                while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                    i += 1;
                }
                i = (i + 2).min(bytes.len());
                for b in out.iter_mut().take(i).skip(start) {
                    if *b != b'\n' {
                        *b = b' ';
                    }
                }
            }
            _ => i += 1,
        }
    }
    String::from_utf8(out).unwrap_or_else(|_| content.to_string())
}

/// Walk `root` for `*.rs` files (skipping `target/`) and compute the health report.
pub fn scan_repo_health(root: &Path) -> RepoHealthReport {
    let fn_re = regex::Regex::new(FN_RE).unwrap();
    let test_re = regex::Regex::new(TEST_ATTR_RE).unwrap();

    let mut modules = Vec::new();
    let mut total_fns = 0usize;
    let mut total_tests = 0usize;
    let mut body_hashes: HashMap<String, Vec<String>> = HashMap::new();
    let mut bodies: HashMap<String, String> = HashMap::new();

    for entry in WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            // Never prune the walk root itself — `tempfile`'s temp dirs (used by
            // this module's own tests) are named like `.tmpXXXXXX` on Windows,
            // which would otherwise match the dotfile exclusion below and
            // silently walk nothing.
            if e.depth() == 0 {
                return true;
            }
            let name = e.file_name().to_string_lossy();
            name != "target" && name != "node_modules" && !name.starts_with('.')
        })
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
            let Ok(content) = std::fs::read_to_string(entry.path()) else {
                continue;
            };
            let masked = mask_non_code(&content);
            let module_path = entry
                .path()
                .strip_prefix(root)
                .unwrap_or(entry.path())
                .to_string_lossy()
                .replace('\\', "/");

            let fn_count = fn_re.find_iter(&masked).count();
            let test_count = test_re.find_iter(&masked).count();
            total_fns += fn_count;
            total_tests += test_count;

            if fn_count > 0 {
                modules.push(ModuleTestStats {
                    module_path: module_path.clone(),
                    fn_count,
                    test_count,
                });
            }

            for (name, body) in extract_test_bodies(&content, &masked) {
                let key = format!("{module_path}::{name}");
                let hash = hash_body(&body);
                body_hashes.entry(hash).or_default().push(key.clone());
                bodies.insert(key, body);
            }
        }
    }

    let untested_modules: Vec<String> = modules
        .iter()
        .filter(|m| m.fn_count > 0 && m.test_count == 0)
        .map(|m| m.module_path.clone())
        .collect();

    let duplicate_test_groups: Vec<DuplicateTestGroup> = body_hashes
        .into_values()
        .filter(|group| group.len() > 1)
        .map(|tests| {
            let preview = tests
                .first()
                .and_then(|k| bodies.get(k))
                .map(|b| b.chars().take(120).collect::<String>())
                .unwrap_or_default();
            DuplicateTestGroup {
                tests,
                body_preview: preview,
            }
        })
        .collect();

    modules.sort_by(|a, b| a.module_path.cmp(&b.module_path));

    RepoHealthReport {
        modules,
        total_fns,
        total_tests,
        untested_modules,
        duplicate_test_groups,
    }
}

/// Pulls out `(fn_name, normalized_body)` for every `#[test] fn ...  { ... }` in
/// `content`, using brace counting rather than a regex (test bodies nest braces).
/// All position-finding (attribute, name, braces) runs against `masked` — the
/// same text with string/comment interiors blanked — so a string literal that
/// happens to contain `{`, `}`, or `#[test]` can't be mistaken for real code;
/// the returned body text is then sliced from the real `content` at those same
/// byte offsets (masking preserves length, so offsets carry over directly).
fn extract_test_bodies(content: &str, masked: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let bytes = masked.as_bytes();
    let mut search_from = 0usize;

    while let Some(attr_pos) = masked[search_from..].find("#[test]") {
        let abs_attr = search_from + attr_pos;
        let after_attr = &masked[abs_attr..];
        let Some(fn_kw) = after_attr.find("fn ") else {
            break;
        };
        let name_start = abs_attr + fn_kw + 3;
        let name_end = masked[name_start..]
            .find(|c: char| c == '(' || c.is_whitespace())
            .map(|i| name_start + i)
            .unwrap_or(name_start);
        let name = content[name_start..name_end].to_string();

        let Some(brace_start) = masked[name_end..].find('{') else {
            break;
        };
        let brace_start = name_end + brace_start;
        let mut depth = 0i32;
        let mut i = brace_start;
        let mut brace_end = None;
        while i < bytes.len() {
            match bytes[i] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        brace_end = Some(i);
                        break;
                    }
                }
                _ => {}
            }
            i += 1;
        }
        let Some(brace_end) = brace_end else { break };
        let body = &content[brace_start + 1..brace_end];
        out.push((name, normalize_body(body)));
        search_from = brace_end + 1;
        if search_from >= masked.len() {
            break;
        }
    }
    out
}

fn normalize_body(body: &str) -> String {
    body.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn hash_body(body: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(body.as_bytes());
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_file(dir: &Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut f = std::fs::File::create(path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
    }

    #[test]
    fn counts_fns_and_tests_per_module() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "src/lib.rs",
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n\
             fn helper() {}\n\
             #[test]\nfn test_add() { assert_eq!(add(1, 2), 3); }\n",
        );
        let report = scan_repo_health(dir.path());
        assert_eq!(report.total_fns, 3);
        assert_eq!(report.total_tests, 1);
        assert_eq!(report.modules.len(), 1);
        assert_eq!(report.modules[0].fn_count, 3);
        assert_eq!(report.modules[0].test_count, 1);
    }

    #[test]
    fn flags_a_module_with_functions_but_no_tests() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "src/untested.rs",
            "pub fn risky() { todo!() }\n",
        );
        let report = scan_repo_health(dir.path());
        assert!(report
            .untested_modules
            .iter()
            .any(|m| m.contains("untested.rs")));
    }

    #[test]
    fn does_not_mistake_test_attributes_inside_string_literals_for_real_tests() {
        // Found by dogfooding this exact module against the real repo: a test
        // fixture that embeds example source containing "#[test]\nfn ..." as a
        // *string* was being parsed as a second real test.
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "src/fixture_writer.rs",
            "#[test]\nfn writes_a_fixture() {\n    \
             let src = \"#[test]\\nfn also_checks_one() { assert_eq!(1 + 1, 2); }\\n\";\n    \
             assert!(src.contains(\"test\"));\n}\n",
        );
        let report = scan_repo_health(dir.path());
        assert_eq!(
            report.total_tests, 1,
            "the string literal must not count as a second test"
        );
        assert!(report.duplicate_test_groups.is_empty());
    }

    #[test]
    fn does_not_flag_a_module_with_no_functions_at_all() {
        let dir = tempfile::tempdir().unwrap();
        write_file(dir.path(), "src/consts.rs", "pub const MAX: u32 = 10;\n");
        let report = scan_repo_health(dir.path());
        assert!(report.modules.is_empty());
        assert!(report.untested_modules.is_empty());
    }

    #[test]
    fn finds_two_tests_with_identical_bodies_as_a_duplicate_group() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "src/a.rs",
            "#[test]\nfn checks_addition() {\n    assert_eq!(1 + 1, 2);\n}\n\
             #[test]\nfn also_checks_addition() {\n    assert_eq!(1 + 1, 2);\n}\n\
             #[test]\nfn checks_something_else() {\n    assert_eq!(2 + 2, 4);\n}\n",
        );
        let report = scan_repo_health(dir.path());
        assert_eq!(report.duplicate_test_groups.len(), 1);
        let group = &report.duplicate_test_groups[0];
        assert_eq!(group.tests.len(), 2);
        assert!(group.tests.iter().any(|t| t.contains("checks_addition")));
        assert!(group
            .tests
            .iter()
            .any(|t| t.contains("also_checks_addition")));
    }

    #[test]
    fn does_not_flag_tests_that_merely_look_similar() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "src/b.rs",
            "#[test]\nfn checks_two_plus_two() {\n    assert_eq!(2 + 2, 4);\n}\n\
             #[test]\nfn checks_three_plus_three() {\n    assert_eq!(3 + 3, 6);\n}\n",
        );
        let report = scan_repo_health(dir.path());
        assert!(report.duplicate_test_groups.is_empty());
    }

    #[test]
    fn ignores_whitespace_and_comment_differences_when_hashing() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "src/c.rs",
            "#[test]\nfn tight() { assert!(true); }\n\
             #[test]\nfn loose() {\n    // a comment\n    assert!(true);\n\n}\n",
        );
        let report = scan_repo_health(dir.path());
        assert_eq!(report.duplicate_test_groups.len(), 1);
    }

    #[test]
    fn skips_the_target_directory() {
        let dir = tempfile::tempdir().unwrap();
        write_file(
            dir.path(),
            "target/debug/build/generated.rs",
            "pub fn generated() {}\n",
        );
        let report = scan_repo_health(dir.path());
        assert!(report.modules.is_empty());
    }
}
