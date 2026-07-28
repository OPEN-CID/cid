/*!
 * Test-impact and documentation graphs (Phase 4, Part A).
 *
 * Both extend the existing Phase 2 Semantic Context Engine rather than
 * standing up a separate subsystem — per the Phase 4 brief's own resolution
 * of the "Repository Digital Twin" proposal, these are the two genuinely new
 * pieces, not a reason to rebuild what Part 7 already covers.
 *
 * Both are built from the same source: `CodeAnalyzer::analyze_directory`'s
 * Tree-sitter symbol extraction, plus a lightweight scan of test and doc
 * files for which symbols they mention. Off by default per Repo Channel,
 * same as the rest of the Semantic Context Engine (Part 17).
 */

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::api::types::FileIndex;

const TEST_FILE_MARKERS: &[&str] = &["test", "tests", "__tests__", "spec"];
const TEST_FILE_SUFFIXES: &[&str] = &[
    ".spec.ts",
    ".spec.tsx",
    ".spec.js",
    ".test.ts",
    ".test.tsx",
    ".test.js",
    "_test.py",
    "_test.go",
    "_test.rs",
];
const DOC_EXTENSIONS: &[&str] = &["md", "mdx"];

pub fn is_test_file(path: &str) -> bool {
    let lower = path.to_lowercase();
    TEST_FILE_SUFFIXES.iter().any(|s| lower.ends_with(s))
        || TEST_FILE_MARKERS.iter().any(|m| {
            lower
                .split(['/', '\\'])
                .any(|segment| segment == *m || segment.starts_with(&format!("{m}_")))
        })
}

fn is_doc_file(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| DOC_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Test-impact graph
// ---------------------------------------------------------------------------

/// Which test files exercise a given symbol, and vice versa — "what tests
/// cover this" and "what does this test touch" in one incrementally
/// rebuildable structure.
#[derive(Debug, Default, Clone)]
pub struct TestImpactGraph {
    /// symbol name -> test file paths that reference it
    symbol_to_tests: HashMap<String, HashSet<String>>,
    /// test file path -> symbol names it references
    test_to_symbols: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestImpactEntry {
    pub symbol: String,
    pub covering_tests: Vec<String>,
}

impl TestImpactGraph {
    pub fn new() -> Self {
        Self::default()
    }

    /// Rebuild from a full symbol scan plus each test file's real content.
    /// Called on enable and on a full repository re-index; `update_file`
    /// handles the incremental case.
    ///
    /// `test_contents` must hold each test file's actual source, not its
    /// parsed symbol list — a test file's *defined* symbols (`fn it_adds()`)
    /// are not what it covers; what it *references* in the body (a call to
    /// `add_numbers`) is. Feeding definitions in here silently produces an
    /// empty graph against every real codebase, since a test rarely redefines
    /// the function it's testing.
    pub fn build(files: &[FileIndex], test_contents: &[(String, String)]) -> Self {
        let mut graph = Self::new();
        let known_symbols: HashSet<&str> = files
            .iter()
            .filter(|f| !is_test_file(&f.path))
            .flat_map(|f| f.symbols.iter().map(|s| s.name.as_str()))
            .collect();

        for (path, content) in test_contents {
            if !is_test_file(path) {
                continue;
            }
            let referenced = super::extract_identifier_like_tokens(content);
            graph.index_test_file(path, &referenced, &known_symbols);
        }
        graph
    }

    /// Re-index one test file's content in isolation, e.g. after a file-change
    /// notification — the incremental refresh Part 7 asks for, without
    /// rescanning the whole repository.
    pub fn update_file(
        &mut self,
        test_file_path: &str,
        test_file_content: &str,
        known_symbols: &HashSet<&str>,
    ) {
        if !is_test_file(test_file_path) {
            return;
        }
        self.remove_file(test_file_path);
        let referenced = super::extract_identifier_like_tokens(test_file_content);
        self.index_test_file(test_file_path, &referenced, known_symbols);
    }

    fn index_test_file(
        &mut self,
        test_file_path: &str,
        candidate_names: &[String],
        known_symbols: &HashSet<&str>,
    ) {
        let mut touched = HashSet::new();
        for name in candidate_names {
            if known_symbols.contains(name.as_str()) {
                touched.insert(name.clone());
                self.symbol_to_tests
                    .entry(name.clone())
                    .or_default()
                    .insert(test_file_path.to_string());
            }
        }
        if !touched.is_empty() {
            self.test_to_symbols
                .insert(test_file_path.to_string(), touched);
        }
    }

    pub fn remove_file(&mut self, test_file_path: &str) {
        if let Some(symbols) = self.test_to_symbols.remove(test_file_path) {
            for symbol in symbols {
                if let Some(tests) = self.symbol_to_tests.get_mut(&symbol) {
                    tests.remove(test_file_path);
                    if tests.is_empty() {
                        self.symbol_to_tests.remove(&symbol);
                    }
                }
            }
        }
    }

    /// Tests covering a single symbol.
    pub fn tests_for_symbol(&self, symbol: &str) -> Vec<String> {
        self.symbol_to_tests
            .get(symbol)
            .map(|s| {
                let mut v: Vec<String> = s.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    }

    /// Tests covering any of a set of symbols — what a patch touching several
    /// symbols at once should re-run.
    pub fn tests_for_symbols(&self, symbols: &[String]) -> Vec<String> {
        let mut out: HashSet<String> = HashSet::new();
        for symbol in symbols {
            if let Some(tests) = self.symbol_to_tests.get(symbol) {
                out.extend(tests.iter().cloned());
            }
        }
        let mut v: Vec<String> = out.into_iter().collect();
        v.sort();
        v
    }

    /// Symbols a single test file exercises, for "what does this test cover."
    pub fn symbols_for_test(&self, test_file_path: &str) -> Vec<String> {
        self.test_to_symbols
            .get(test_file_path)
            .map(|s| {
                let mut v: Vec<String> = s.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    }

    pub fn entries(&self) -> Vec<TestImpactEntry> {
        let mut out: Vec<TestImpactEntry> = self
            .symbol_to_tests
            .iter()
            .map(|(symbol, tests)| {
                let mut covering_tests: Vec<String> = tests.iter().cloned().collect();
                covering_tests.sort();
                TestImpactEntry {
                    symbol: symbol.clone(),
                    covering_tests,
                }
            })
            .collect();
        out.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        out
    }

    pub fn symbol_count(&self) -> usize {
        self.symbol_to_tests.len()
    }

    pub fn test_count(&self) -> usize {
        self.test_to_symbols.len()
    }
}

// ---------------------------------------------------------------------------
// Documentation graph
// ---------------------------------------------------------------------------

/// Which docs reference which code symbols — and, from that, which docs are
/// stale because the symbol they describe no longer exists.
///
/// Mentions are stored **unconditionally**, not filtered against
/// `known_symbols` at write time — staleness detection depends on being able
/// to compare "what a doc mentioned" against "what currently exists," so
/// pre-filtering at write time would make every doc trivially non-stale by
/// construction. `known_symbols` only ever applies at query time, in
/// `stale_docs`.
#[derive(Debug, Default, Clone)]
pub struct DocGraph {
    /// doc path -> backticked identifiers it mentions
    doc_to_symbols: HashMap<String, HashSet<String>>,
    /// mentioned identifier -> doc paths that mention it
    symbol_to_docs: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaleDocEntry {
    pub doc_path: String,
    /// Symbols the doc references that no longer exist anywhere in the repo.
    pub missing_symbols: Vec<String>,
}

impl DocGraph {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn build(doc_contents: &[(String, String)]) -> Self {
        let mut graph = Self::new();
        for (path, content) in doc_contents {
            if is_doc_file(path) {
                graph.update_doc(path, content);
            }
        }
        graph
    }

    /// Re-index one doc after it changes. Stores every backticked mention
    /// found, whether or not it currently matches a real symbol — see the
    /// struct-level note on why filtering happens only at query time.
    pub fn update_doc(&mut self, doc_path: &str, content: &str) {
        self.remove_doc(doc_path);
        let mentioned = extract_doc_symbol_mentions(content);
        if mentioned.is_empty() {
            return;
        }
        for symbol in &mentioned {
            self.symbol_to_docs
                .entry(symbol.clone())
                .or_default()
                .insert(doc_path.to_string());
        }
        self.doc_to_symbols.insert(doc_path.to_string(), mentioned);
    }

    pub fn remove_doc(&mut self, doc_path: &str) {
        if let Some(symbols) = self.doc_to_symbols.remove(doc_path) {
            for symbol in symbols {
                if let Some(docs) = self.symbol_to_docs.get_mut(&symbol) {
                    docs.remove(doc_path);
                    if docs.is_empty() {
                        self.symbol_to_docs.remove(&symbol);
                    }
                }
            }
        }
    }

    pub fn docs_for_symbol(&self, symbol: &str) -> Vec<String> {
        self.symbol_to_docs
            .get(symbol)
            .map(|d| {
                let mut v: Vec<String> = d.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    }

    pub fn symbols_for_doc(&self, doc_path: &str) -> Vec<String> {
        self.doc_to_symbols
            .get(doc_path)
            .map(|s| {
                let mut v: Vec<String> = s.iter().cloned().collect();
                v.sort();
                v
            })
            .unwrap_or_default()
    }

    /// Docs that reference at least one symbol not present in `known_symbols`
    /// — the concrete "stale docs are detectable" deliverable from Part 7.
    pub fn stale_docs(&self, known_symbols: &HashSet<&str>) -> Vec<StaleDocEntry> {
        let mut out = Vec::new();
        for (doc, symbols) in &self.doc_to_symbols {
            let missing: Vec<String> = symbols
                .iter()
                .filter(|s| !known_symbols.contains(s.as_str()))
                .cloned()
                .collect();
            if !missing.is_empty() {
                let mut missing = missing;
                missing.sort();
                out.push(StaleDocEntry {
                    doc_path: doc.clone(),
                    missing_symbols: missing,
                });
            }
        }
        out.sort_by(|a, b| a.doc_path.cmp(&b.doc_path));
        out
    }

    pub fn doc_count(&self) -> usize {
        self.doc_to_symbols.len()
    }
}

/// Pull candidate symbol names out of markdown: backticked code spans
/// (`` `functionName` ``) are the highest-confidence signal that a doc names
/// a real symbol, so those are what get checked — free prose isn't parsed
/// for identifier-shaped words, which would produce far too many false
/// positives (English words that happen to look like identifiers).
fn extract_doc_symbol_mentions(content: &str) -> HashSet<String> {
    let mut out = HashSet::new();
    let mut rest = content;
    while let Some(start) = rest.find('`') {
        let after = &rest[start + 1..];
        match after.find('`') {
            Some(end) => {
                let span = &after[..end];
                // A code span may be a whole call (`doSomething()`), a path
                // (`src/foo.rs`), or a bare identifier — only the identifier
                // shape is useful here.
                let candidate = span.trim_end_matches("()").trim();
                if !candidate.is_empty()
                    && !candidate.contains(['/', '\\', ' ', '.'])
                    && candidate
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphabetic() || c == '_')
                    && candidate.chars().all(|c| c.is_alphanumeric() || c == '_')
                {
                    out.insert(candidate.to_string());
                }
                rest = &after[end + 1..];
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::types::{CodeSymbol, SymbolKind};
    use chrono::Utc;

    fn file(path: &str, symbol_names: &[&str]) -> FileIndex {
        FileIndex {
            path: path.to_string(),
            language: "rust".to_string(),
            symbols: symbol_names
                .iter()
                .map(|n| CodeSymbol {
                    id: format!("{path}::{n}"),
                    name: n.to_string(),
                    kind: SymbolKind::Function,
                    file_path: path.to_string(),
                    line: 1,
                    column: 0,
                    end_line: 1,
                    end_column: 0,
                    parent: None,
                    imports: vec![],
                })
                .collect(),
            imports: vec![],
            last_modified: Utc::now(),
            size: 0,
        }
    }

    // ---- is_test_file ----

    #[test]
    fn recognizes_common_test_file_shapes() {
        assert!(is_test_file("src/auth.test.ts"));
        assert!(is_test_file("src/auth.spec.ts"));
        assert!(is_test_file("tests/worktree_property.rs"));
        assert!(is_test_file("src/__tests__/auth.js"));
        assert!(is_test_file("auth_test.py"));
    }

    #[test]
    fn does_not_misclassify_ordinary_source_files() {
        assert!(!is_test_file("src/auth.rs"));
        assert!(
            !is_test_file("src/testimonial.rs"),
            "must not match on a substring inside a word"
        );
        assert!(!is_test_file("src/latest.ts"));
    }

    // ---- TestImpactGraph ----

    #[test]
    fn builds_symbol_to_test_mapping_from_real_test_content() {
        let files = vec![file("src/auth.rs", &["validate_token"])];
        let test_contents = vec![(
            "tests/auth_test.rs".to_string(),
            "fn setup_fixture() {}\nfn it_validates() { validate_token(\"x\"); }".to_string(),
        )];
        let graph = TestImpactGraph::build(&files, &test_contents);
        assert_eq!(
            graph.tests_for_symbol("validate_token"),
            vec!["tests/auth_test.rs".to_string()]
        );
    }

    #[test]
    fn does_not_treat_a_tests_own_helper_as_a_covered_source_symbol() {
        let files = vec![file("src/auth.rs", &["validate_token"])];
        let test_contents = vec![(
            "tests/auth_test.rs".to_string(),
            "fn setup_fixture() {}\nfn it_validates() { setup_fixture(); validate_token(\"x\"); }"
                .to_string(),
        )];
        let graph = TestImpactGraph::build(&files, &test_contents);
        // setup_fixture is defined only in the test file itself, never in
        // real source, so it must not appear as a "covered" symbol even
        // though the test content mentions it.
        assert!(graph.tests_for_symbol("setup_fixture").is_empty());
    }

    #[test]
    fn a_symbol_with_no_covering_test_returns_empty() {
        let files = vec![file("src/orphan.rs", &["untested_function"])];
        let graph = TestImpactGraph::build(&files, &[]);
        assert!(graph.tests_for_symbol("untested_function").is_empty());
    }

    #[test]
    fn tests_for_symbols_unions_across_multiple_symbols() {
        let files = vec![file("src/a.rs", &["fn_a"]), file("src/b.rs", &["fn_b"])];
        let test_contents = vec![
            (
                "tests/a_test.rs".to_string(),
                "fn t() { fn_a(); }".to_string(),
            ),
            (
                "tests/b_test.rs".to_string(),
                "fn t() { fn_b(); }".to_string(),
            ),
        ];
        let graph = TestImpactGraph::build(&files, &test_contents);
        let mut covering = graph.tests_for_symbols(&["fn_a".to_string(), "fn_b".to_string()]);
        covering.sort();
        assert_eq!(
            covering,
            vec!["tests/a_test.rs".to_string(), "tests/b_test.rs".to_string()]
        );
    }

    #[test]
    fn a_test_file_passed_without_the_is_test_file_shape_is_ignored() {
        // build() is defensive: content for a path that doesn't look like a
        // test file must not be indexed, even if the caller's list is wrong.
        let files = vec![file("src/a.rs", &["fn_a"])];
        let test_contents = vec![(
            "src/regular_file.rs".to_string(),
            "fn t() { fn_a(); }".to_string(),
        )];
        let graph = TestImpactGraph::build(&files, &test_contents);
        assert!(graph.tests_for_symbol("fn_a").is_empty());
    }

    #[test]
    fn incremental_update_replaces_a_test_files_previous_edges() {
        let known: HashSet<&str> = ["fn_a", "fn_b"].into_iter().collect();
        let mut graph = TestImpactGraph::new();
        graph.update_file("tests/x_test.rs", "fn_a();", &known);
        assert_eq!(
            graph.tests_for_symbol("fn_a"),
            vec!["tests/x_test.rs".to_string()]
        );

        graph.update_file("tests/x_test.rs", "fn_b();", &known);
        assert!(
            graph.tests_for_symbol("fn_a").is_empty(),
            "stale edge must be gone"
        );
        assert_eq!(
            graph.tests_for_symbol("fn_b"),
            vec!["tests/x_test.rs".to_string()]
        );
    }

    #[test]
    fn removing_a_test_file_removes_its_edges_both_directions() {
        let files = vec![file("src/a.rs", &["fn_a"])];
        let test_contents = vec![(
            "tests/a_test.rs".to_string(),
            "fn t() { fn_a(); }".to_string(),
        )];
        let mut graph = TestImpactGraph::build(&files, &test_contents);
        assert_eq!(graph.test_count(), 1);
        graph.remove_file("tests/a_test.rs");
        assert!(graph.tests_for_symbol("fn_a").is_empty());
        assert_eq!(graph.test_count(), 0);
    }

    // ---- DocGraph ----

    #[test]
    fn a_doc_referencing_a_real_symbol_is_indexed() {
        let mut graph = DocGraph::new();
        graph.update_doc("docs/api.md", "Call `compute_total` to get the sum.");
        assert_eq!(
            graph.docs_for_symbol("compute_total"),
            vec!["docs/api.md".to_string()]
        );
    }

    #[test]
    fn a_doc_is_stale_when_its_symbol_no_longer_exists() {
        let known: HashSet<&str> = ["compute_total"].into_iter().collect();
        let mut graph = DocGraph::new();
        graph.update_doc("docs/api.md", "See `compute_total` and `removed_function`.");

        let stale = graph.stale_docs(&known);
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].doc_path, "docs/api.md");
        assert_eq!(
            stale[0].missing_symbols,
            vec!["removed_function".to_string()]
        );
    }

    #[test]
    fn a_doc_with_only_valid_symbols_is_not_stale() {
        let known: HashSet<&str> = ["compute_total"].into_iter().collect();
        let mut graph = DocGraph::new();
        graph.update_doc("docs/api.md", "See `compute_total`.");
        assert!(graph.stale_docs(&known).is_empty());
    }

    #[test]
    fn prose_words_that_look_like_identifiers_are_not_extracted_without_backticks() {
        let mut graph = DocGraph::new();
        graph.update_doc(
            "docs/guide.md",
            "This function will render the page when called.",
        );
        // "render" appears in prose but not in backticks, so it must not be
        // picked up — free-text scanning produces far too many false matches.
        assert!(graph.symbols_for_doc("docs/guide.md").is_empty());
    }

    #[test]
    fn a_backticked_call_shape_still_matches_the_bare_symbol_name() {
        let mut graph = DocGraph::new();
        graph.update_doc("docs/api.md", "Call `compute_total()` for the sum.");
        assert_eq!(
            graph.symbols_for_doc("docs/api.md"),
            vec!["compute_total".to_string()]
        );
    }

    #[test]
    fn a_backticked_file_path_is_not_mistaken_for_a_symbol() {
        let mut graph = DocGraph::new();
        graph.update_doc("docs/api.md", "See `src/math.rs` for details.");
        assert!(graph.symbols_for_doc("docs/api.md").is_empty());
    }

    #[test]
    fn updating_a_doc_replaces_its_previous_edges() {
        let mut graph = DocGraph::new();
        graph.update_doc("docs/x.md", "`fn_a`");
        graph.update_doc("docs/x.md", "`fn_b`");
        assert!(graph.docs_for_symbol("fn_a").is_empty());
        assert_eq!(graph.docs_for_symbol("fn_b"), vec!["docs/x.md".to_string()]);
    }

    #[test]
    fn removing_a_doc_clears_both_directions() {
        let mut graph = DocGraph::new();
        graph.update_doc("docs/x.md", "`fn_a`");
        assert_eq!(graph.doc_count(), 1);
        graph.remove_doc("docs/x.md");
        assert!(graph.docs_for_symbol("fn_a").is_empty());
        assert_eq!(graph.doc_count(), 0);
    }
}
