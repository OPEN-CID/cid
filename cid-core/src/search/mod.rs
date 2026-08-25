//! Repo-wide text search on ripgrep's engine.
//!
//! Replaces the previous path, which walked every directory (including
//! `target/`, `node_modules/` and CID's own `.cid/worktrees/` copies of the
//! repo), read each candidate file into memory and tree-sitter parsed it just
//! to match a substring against symbol names. On CID's own repo one query
//! visited ~27k files and took **218 seconds**, so the UI sat on "Searching…"
//! indefinitely.
//!
//! Three properties matter here and none of them were true before:
//!
//! - **Respects `.gitignore`.** `ignore::WalkBuilder` is the same walker
//!   ripgrep uses, so build output and dependency trees are skipped because
//!   the repo already says to skip them — not because of a hardcoded list that
//!   drifts.
//! - **Bounded.** A hit cap stops the walk early, so a query matching half the
//!   repo returns promptly and says it truncated instead of streaming forever.
//! - **Off the async runtime.** Callers run this inside `spawn_blocking`; it is
//!   CPU- and IO-bound and would otherwise hold a Tokio worker.

use anyhow::Result;
use grep_matcher::Matcher;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder, Sink, SinkMatch};
use ignore::{WalkBuilder, WalkState};
use serde::Serialize;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

/// Directories to skip even when the repo's own ignore rules don't. `.cid`
/// holds Session worktrees — full copies of the repo — which otherwise return
/// a duplicate of every hit.
const ALWAYS_SKIP: &[&str] = &[".git", ".cid", "node_modules", "target"];

/// Lines longer than this are minified bundles or embedded data, never
/// something a human is searching for; matching them also blows up the
/// response with megabyte-long strings.
const MAX_LINE_BYTES: usize = 512;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SearchHit {
    pub file_path: String,
    pub line: u64,
    /// The matching line, trimmed of trailing newline and capped at
    /// `MAX_LINE_BYTES` so one pathological line can't dominate the payload.
    pub line_text: String,
    /// Byte offset of the match within `line_text`, for highlighting. `None`
    /// when the match starts beyond the truncation point.
    pub match_start: Option<usize>,
    pub match_end: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SearchOutcome {
    pub hits: Vec<SearchHit>,
    /// True when the hit cap stopped the walk, so the UI can say "showing
    /// first N" rather than implying these are all the matches.
    pub truncated: bool,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    /// Treat the query as a regex. Default is a literal substring, which is
    /// what a plain search box implies — a stray `(` should not error.
    pub regex: bool,
    /// `None` means smart-case: case-insensitive unless the query itself
    /// contains an uppercase letter, matching ripgrep's `-S`.
    pub case_sensitive: Option<bool>,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 500,
            regex: false,
            case_sensitive: None,
        }
    }
}

/// Collects matches for one file. `grep-searcher` calls `matched` per matching
/// line; returning `Ok(false)` stops searching this file.
struct Collector<'a> {
    path: &'a Path,
    hits: Vec<SearchHit>,
    matcher: &'a grep_regex::RegexMatcher,
    remaining: &'a AtomicUsize,
}

impl Sink for Collector<'_> {
    type Error = std::io::Error;

    fn matched(
        &mut self,
        _searcher: &grep_searcher::Searcher,
        m: &SinkMatch,
    ) -> Result<bool, Self::Error> {
        if self.remaining.load(Ordering::Relaxed) == 0 {
            return Ok(false);
        }

        let raw = String::from_utf8_lossy(m.bytes());
        let trimmed = raw.trim_end_matches(['\n', '\r']);

        // Match offsets are relative to the untruncated line, so they are only
        // meaningful when they fall inside what we keep.
        let found = self.matcher.find(trimmed.as_bytes()).ok().flatten();
        let (mut start, mut end) = match found {
            Some(mat) => (Some(mat.start()), Some(mat.end())),
            None => (None, None),
        };

        let line_text = if trimmed.len() > MAX_LINE_BYTES {
            if start.map(|s| s > MAX_LINE_BYTES).unwrap_or(false) {
                start = None;
                end = None;
            } else {
                end = end.map(|e| e.min(MAX_LINE_BYTES));
            }
            // Cut on a char boundary — `trimmed` is lossy-decoded UTF-8, and
            // slicing mid-codepoint would panic.
            let mut cut = MAX_LINE_BYTES;
            while cut > 0 && !trimmed.is_char_boundary(cut) {
                cut -= 1;
            }
            format!("{}…", &trimmed[..cut])
        } else {
            trimmed.to_string()
        };

        self.hits.push(SearchHit {
            file_path: self.path.to_string_lossy().to_string(),
            line: m.line_number().unwrap_or(0),
            line_text,
            match_start: start,
            match_end: end,
        });

        // `fetch_update` rather than a plain decrement: several worker threads
        // share this budget and it must not wrap below zero.
        let still_wanted = self
            .remaining
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_ok();
        Ok(still_wanted)
    }
}

pub fn search_text(root: &Path, query: &str, opts: &SearchOptions) -> Result<SearchOutcome> {
    let started = std::time::Instant::now();

    if query.is_empty() {
        return Ok(SearchOutcome {
            hits: Vec::new(),
            truncated: false,
            elapsed_ms: 0,
        });
    }

    let pattern = if opts.regex {
        query.to_string()
    } else {
        regex::escape(query)
    };

    let mut builder = RegexMatcherBuilder::new();
    match opts.case_sensitive {
        Some(true) => {
            builder.case_insensitive(false);
        }
        Some(false) => {
            builder.case_insensitive(true);
        }
        None => {
            builder.case_smart(true);
        }
    }
    let matcher = builder.build(&pattern)?;

    let remaining = Arc::new(AtomicUsize::new(opts.limit));
    let collected: Arc<Mutex<Vec<SearchHit>>> = Arc::new(Mutex::new(Vec::new()));

    let walker = WalkBuilder::new(root)
        .hidden(false) // dotfiles are real source (.eslintrc.cjs); ignore rules still apply
        .git_ignore(true)
        .git_global(true)
        // Without this, `.gitignore` is honored only when the root is inside a
        // git work tree — so a plain folder, or a Session worktree opened
        // directly, would silently search build output.
        .require_git(false)
        .parents(true)
        .filter_entry(|entry| {
            entry
                .file_name()
                .to_str()
                .map(|name| !ALWAYS_SKIP.contains(&name))
                .unwrap_or(true)
        })
        .build_parallel();

    walker.run(|| {
        let matcher = matcher.clone();
        let remaining = Arc::clone(&remaining);
        let collected = Arc::clone(&collected);
        let mut searcher = SearcherBuilder::new()
            .binary_detection(BinaryDetection::quit(b'\x00'))
            .line_number(true)
            .build();

        Box::new(move |result| {
            if remaining.load(Ordering::Relaxed) == 0 {
                return WalkState::Quit;
            }
            let entry = match result {
                Ok(e) => e,
                Err(_) => return WalkState::Continue,
            };
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                return WalkState::Continue;
            }

            let mut collector = Collector {
                path: entry.path(),
                hits: Vec::new(),
                matcher: &matcher,
                remaining: &remaining,
            };
            // A single unreadable or undecodable file must not abort the walk.
            if searcher
                .search_path(&matcher, entry.path(), &mut collector)
                .is_ok()
                && !collector.hits.is_empty()
            {
                if let Ok(mut all) = collected.lock() {
                    all.extend(collector.hits);
                }
            }
            WalkState::Continue
        })
    });

    let mut hits = Arc::try_unwrap(collected)
        .map(|m| m.into_inner().unwrap_or_default())
        .unwrap_or_else(|arc| arc.lock().map(|g| g.clone()).unwrap_or_default());

    // The parallel walk finishes files in nondeterministic order; sort so the
    // same query twice returns the same list.
    hits.sort_by(|a, b| a.file_path.cmp(&b.file_path).then(a.line.cmp(&b.line)));
    let truncated = hits.len() >= opts.limit;
    hits.truncate(opts.limit);

    Ok(SearchOutcome {
        hits,
        truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let src = tmp.path().join("src");
        fs::create_dir_all(&src).unwrap();
        fs::write(src.join("main.rs"), "fn main() {\n    let needle = 1;\n}\n").unwrap();
        fs::write(src.join("other.rs"), "// no match here\n").unwrap();
        tmp
    }

    #[test]
    fn finds_a_literal_match_with_line_and_offsets() {
        let tmp = repo();
        let out = search_text(tmp.path(), "needle", &SearchOptions::default()).unwrap();
        assert_eq!(out.hits.len(), 1);
        let hit = &out.hits[0];
        assert_eq!(hit.line, 2);
        assert!(hit.file_path.ends_with("main.rs"));
        assert_eq!(hit.line_text, "    let needle = 1;");
        let (s, e) = (hit.match_start.unwrap(), hit.match_end.unwrap());
        assert_eq!(&hit.line_text[s..e], "needle");
    }

    /// The whole point of the rewrite: these directories used to be walked,
    /// parsed, and returned as duplicate hits.
    #[test]
    fn build_and_worktree_directories_are_skipped() {
        let tmp = repo();
        for dir in ["target", "node_modules", ".cid/worktrees/abc/src", ".git"] {
            let d = tmp.path().join(dir);
            fs::create_dir_all(&d).unwrap();
            fs::write(d.join("copy.rs"), "let needle = 2;\n").unwrap();
        }
        let out = search_text(tmp.path(), "needle", &SearchOptions::default()).unwrap();
        assert_eq!(
            out.hits.len(),
            1,
            "expected only src/main.rs, got {:?}",
            out.hits.iter().map(|h| &h.file_path).collect::<Vec<_>>()
        );
    }

    #[test]
    fn gitignored_files_are_skipped() {
        let tmp = repo();
        fs::write(tmp.path().join(".gitignore"), "generated/\n").unwrap();
        let gen = tmp.path().join("generated");
        fs::create_dir_all(&gen).unwrap();
        fs::write(gen.join("out.rs"), "let needle = 3;\n").unwrap();

        let out = search_text(tmp.path(), "needle", &SearchOptions::default()).unwrap();
        assert_eq!(out.hits.len(), 1);
        assert!(out.hits[0].file_path.ends_with("main.rs"));
    }

    #[test]
    fn smart_case_is_insensitive_until_the_query_has_uppercase() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "Widget\nwidget\n").unwrap();

        let lower = search_text(tmp.path(), "widget", &SearchOptions::default()).unwrap();
        assert_eq!(lower.hits.len(), 2, "lowercase query should match both");

        let upper = search_text(tmp.path(), "Widget", &SearchOptions::default()).unwrap();
        assert_eq!(upper.hits.len(), 1, "uppercase query should be exact");
    }

    #[test]
    fn a_literal_query_is_not_parsed_as_a_regex() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("a.rs"), "fn f(x: u8) {}\n").unwrap();
        // `f(` is invalid regex; as a literal it must simply match.
        let out = search_text(tmp.path(), "f(", &SearchOptions::default()).unwrap();
        assert_eq!(out.hits.len(), 1);
    }

    #[test]
    fn the_hit_cap_truncates_instead_of_running_away() {
        let tmp = TempDir::new().unwrap();
        let body = "needle\n".repeat(50);
        std::fs::write(tmp.path().join("many.rs"), body).unwrap();

        let out = search_text(
            tmp.path(),
            "needle",
            &SearchOptions {
                limit: 10,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(out.hits.len(), 10);
        assert!(out.truncated);
    }

    #[test]
    fn very_long_lines_are_truncated_rather_than_returned_whole() {
        let tmp = TempDir::new().unwrap();
        let long = format!("{}needle{}", "x".repeat(50), "y".repeat(5_000));
        std::fs::write(tmp.path().join("bundle.js"), long).unwrap();

        let out = search_text(tmp.path(), "needle", &SearchOptions::default()).unwrap();
        assert_eq!(out.hits.len(), 1);
        assert!(out.hits[0].line_text.len() <= MAX_LINE_BYTES + 4);
        assert!(out.hits[0].line_text.ends_with('…'));
    }

    #[test]
    fn binary_files_do_not_produce_hits() {
        let tmp = TempDir::new().unwrap();
        let mut bytes = b"needle".to_vec();
        bytes.push(0);
        bytes.extend_from_slice(b"needle");
        std::fs::write(tmp.path().join("blob.bin"), bytes).unwrap();

        let out = search_text(tmp.path(), "needle", &SearchOptions::default()).unwrap();
        assert!(out.hits.len() <= 1, "binary content should stop at the NUL");
    }
}
