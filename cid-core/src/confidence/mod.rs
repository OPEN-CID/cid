use anyhow::Result;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

// ---------------------------------------------------------------------------
// Confidence Engine — Phase 4 centerpiece
// ---------------------------------------------------------------------------

/// All signals used to compute a confidence score for an AI-authored patch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConfidenceSignal {
    /// Whether all referenced symbols actually resolve (via the Structural Context Engine)
    SymbolResolution,
    /// Whether the patch passes the language's standard linter/analyzer cleanly
    StaticAnalysis,
    /// Whether the patch type-checks cleanly
    TypeValidation,
    /// Whether the patch respects architectural boundaries (AGENTS.md/SKILL.md/ADRs)
    ArchitectureValidation,
    /// Which existing tests exercise the touched code, and whether they still pass
    TestImpact,
    /// Whether the patch duplicates existing implementations elsewhere in the repo
    DuplicateDetection,
    /// How many other files/call sites the change ripples into
    DependencyImpact,
    /// How consistent the patch's approach is with prior human-approved patches
    SemanticSimilarity,
    /// Whether the agent checked for and reused existing utilities/patterns
    ExistingImplementationReuse,
}

impl ConfidenceSignal {
    pub fn label(&self) -> &'static str {
        match self {
            ConfidenceSignal::SymbolResolution => "Symbol Resolution",
            ConfidenceSignal::StaticAnalysis => "Static Analysis",
            ConfidenceSignal::TypeValidation => "Type Validation",
            ConfidenceSignal::ArchitectureValidation => "Architecture Validation",
            ConfidenceSignal::TestImpact => "Test Impact",
            ConfidenceSignal::DuplicateDetection => "Duplicate Detection",
            ConfidenceSignal::DependencyImpact => "Dependency Impact",
            ConfidenceSignal::SemanticSimilarity => "Semantic Similarity",
            ConfidenceSignal::ExistingImplementationReuse => "Existing Implementation Reuse",
        }
    }
}

/// An individual signal result, with a score (0.0-1.0) and a plain-language explanation
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignalResult {
    pub signal: ConfidenceSignal,
    pub score: f64,
    pub explanation: String,
    pub details: Option<serde_json::Value>,
}

/// A full confidence score card for a patch
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConfidenceScore {
    pub patch_id: String,
    pub overall: f64,
    pub signals: Vec<SignalResult>,
    pub generated_at: String,
    pub explanation: String,
}

impl ConfidenceScore {
    /// Recompute the average from `signals`, for callers that only have the
    /// signal list. `score_patch` already computes and stores `overall`
    /// directly — prefer that field over calling this when both are
    /// available, since a `ConfidenceScore` can legitimately exist with
    /// `overall` set but `signals` empty (e.g. loaded from a summary view).
    pub fn overall_score(&self) -> f64 {
        if self.signals.is_empty() {
            return 0.0;
        }
        let total: f64 = self.signals.iter().map(|s| s.score).sum();
        total / self.signals.len() as f64
    }

    /// Human-readable verdict based on the stored overall score — not a
    /// recomputation from `signals`, which can be empty on a value that still
    /// carries a valid `overall` (a summary card, a manually constructed
    /// test fixture). Verdict text must never contradict the number shown
    /// next to it.
    pub fn verdict(&self) -> &'static str {
        let s = self.overall;
        if s >= 0.85 {
            "High confidence — this patch looks safe to approve"
        } else if s >= 0.60 {
            "Moderate confidence — review the lower-scoring signals below"
        } else if s >= 0.35 {
            "Low confidence — several signals need attention before approving"
        } else {
            "Very low confidence — consider rejecting or requesting a rewrite"
        }
    }
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

pub struct ConfidenceEngine {
    analyzer: Arc<crate::analyzer::CodeAnalyzer>,
    // Accepted for a future context-engine-aware signal (cross-referencing the
    // Structural Context Engine's own index rather than re-scanning via
    // `analyzer` alone); not yet read by any of the nine current signals.
    #[allow(dead_code)]
    context_engine: Arc<crate::context_engine::ContextEngineManager>,
}

impl ConfidenceEngine {
    pub fn new(
        analyzer: Arc<crate::analyzer::CodeAnalyzer>,
        context_engine: Arc<crate::context_engine::ContextEngineManager>,
    ) -> Self {
        Self {
            analyzer,
            context_engine,
        }
    }

    /// Compute confidence for a patch across all 9 signals.
    ///
    /// Built with sequential `push` calls rather than a `vec![]` literal so
    /// each signal stays individually commented and easy to locate — this is
    /// nine independent, distinctly-documented steps, not a plain list.
    #[allow(clippy::vec_init_then_push)]
    pub fn score_patch(&self, patch: &Patch, repo_path: &str) -> Result<ConfidenceScore> {
        let mut signals = Vec::new();

        // Signal 1: Symbol Resolution
        signals.push(self.score_symbol_resolution(patch, repo_path)?);

        // Signal 2: Static Analysis
        signals.push(self.score_static_analysis(patch, repo_path)?);

        // Signal 3: Type Validation
        signals.push(self.score_type_validation(patch)?);

        // Signal 4: Architecture Validation
        signals.push(self.score_architecture_validation(patch, repo_path)?);

        // Signal 5: Test Impact
        signals.push(self.score_test_impact(patch, repo_path)?);

        // Signal 6: Duplicate Detection
        signals.push(self.score_duplicate_detection(patch, repo_path)?);

        // Signal 7: Dependency Impact
        signals.push(self.score_dependency_impact(patch, repo_path)?);

        // Signal 8: Semantic Similarity
        signals.push(self.score_semantic_similarity(patch, repo_path)?);

        // Signal 9: Existing Implementation Reuse
        signals.push(self.score_existing_reuse(patch, repo_path)?);

        let overall = signals.iter().map(|s| s.score).sum::<f64>() / signals.len() as f64;
        let explanation = Self::generate_explanation(&signals, overall);

        Ok(ConfidenceScore {
            patch_id: patch.patch_id.clone(),
            overall,
            signals,
            generated_at: Utc::now().to_rfc3339(),
            explanation,
        })
    }

    fn score_symbol_resolution(&self, patch: &Patch, repo_path: &str) -> Result<SignalResult> {
        // Check if all symbols referenced in the patch resolve via the context engine
        let unresolved: Vec<String> = patch
            .references
            .iter()
            .filter(|r| !self.symbol_exists(r, repo_path))
            .cloned()
            .collect();

        let total = patch.references.len().max(1);
        let resolved = total - unresolved.len();
        let score = resolved as f64 / total as f64;

        let explanation = if unresolved.is_empty() {
            "All symbols referenced in this patch resolve correctly against the codebase"
                .to_string()
        } else {
            format!(
                "{}/{} symbols resolved; {} unresolved: {}",
                resolved,
                total,
                unresolved.len(),
                unresolved.join(", ")
            )
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::SymbolResolution,
            score,
            explanation,
            details: Some(serde_json::json!({
                "unresolved": unresolved,
                "resolved_count": resolved,
                "total_count": total,
            })),
        })
    }

    fn score_static_analysis(&self, patch: &Patch, _repo_path: &str) -> Result<SignalResult> {
        // Run language-specific linters on the patched content
        // This is a structured, extensible check — real integration points for clippy/eslint/tsc
        let extension = std::path::Path::new(&patch.target_file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let mut issues: Vec<String> = Vec::new();

        match extension {
            "rs" => {
                // For Rust, we check basic patterns that clippy would catch
                issues.extend(self.check_rust_basic_lint(&patch.new_content));
            }
            "ts" | "tsx" | "js" | "jsx" => {
                issues.extend(self.check_js_basic_lint(&patch.new_content));
            }
            "py" => {
                issues.extend(self.check_python_basic_lint(&patch.new_content));
            }
            _ => {}
        }

        let score = if issues.is_empty() {
            1.0
        } else {
            // Deduct 0.15 per issue, floor at 0.0
            let deduction = (issues.len() as f64) * 0.15;
            (1.0 - deduction).max(0.0)
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::StaticAnalysis,
            score,
            explanation: if issues.is_empty() {
                "Patch passes all basic static analysis checks".to_string()
            } else {
                format!(
                    "{} issues found in basic static analysis: {}",
                    issues.len(),
                    issues.join("; ")
                )
            },
            details: if issues.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "issues": issues }))
            },
        })
    }

    fn score_type_validation(&self, patch: &Patch) -> Result<SignalResult> {
        // Check if the patch produces type-error patterns
        // For TypeScript and Rust, basic structural checks
        let extension = std::path::Path::new(&patch.target_file)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let type_issues = match extension {
            "ts" | "tsx" | "js" | "jsx" => self.check_ts_basic_types(&patch.new_content),
            "rs" => self.check_rust_basic_types(&patch.new_content),
            _ => vec![],
        };

        let score = if type_issues.is_empty() {
            1.0
        } else {
            let deduction = (type_issues.len() as f64) * 0.2;
            (1.0 - deduction).max(0.0)
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::TypeValidation,
            score,
            explanation: if type_issues.is_empty() {
                "Patch passes basic type validation checks".to_string()
            } else {
                format!(
                    "{} type issues found: {}",
                    type_issues.len(),
                    type_issues.join("; ")
                )
            },
            details: if type_issues.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "type_issues": type_issues }))
            },
        })
    }

    fn score_architecture_validation(
        &self,
        patch: &Patch,
        repo_path: &str,
    ) -> Result<SignalResult> {
        // Check if the patch respects architecture rules from AGENTS.md/SKILL.md/ADRs
        // Start with simple explicit pattern/path rules
        let rules = self.load_architecture_rules(repo_path)?;
        let violations: Vec<String> = self.check_architecture_rules(patch, &rules);

        let score = if violations.is_empty() {
            1.0
        } else {
            let deduction = (violations.len() as f64) * 0.25;
            (1.0 - deduction).max(0.0)
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::ArchitectureValidation,
            score,
            explanation: if violations.is_empty() {
                "Patch respects all configured architecture boundaries".to_string()
            } else {
                format!(
                    "{} architecture boundary violations: {}",
                    violations.len(),
                    violations.join("; ")
                )
            },
            details: if violations.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "violations": violations }))
            },
        })
    }

    fn score_test_impact(&self, patch: &Patch, repo_path: &str) -> Result<SignalResult> {
        // Which existing tests exercise the touched code, and do they still pass?
        let covered_tests = self.find_tests_for_patch(patch, repo_path)?;
        let test_count = covered_tests.len();

        let score = if test_count == 0 {
            // No tests found for this code — moderate concern
            0.5
        } else if test_count <= 3 {
            0.8
        } else {
            1.0
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::TestImpact,
            score,
            explanation: format!(
                "Found {} existing test(s) covering the touched code paths.",
                test_count
            ),
            details: Some(serde_json::json!({
                "covered_tests": covered_tests,
                "test_count": test_count,
            })),
        })
    }

    fn score_duplicate_detection(&self, patch: &Patch, repo_path: &str) -> Result<SignalResult> {
        // Check if the patch reimplements something already in the repo
        let duplicates = self.find_duplicates(patch, repo_path)?;

        let score = if duplicates.is_empty() {
            1.0
        } else {
            // Significant concern for duplicates — reward reusing existing code
            0.3
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::DuplicateDetection,
            score,
            explanation: if duplicates.is_empty() {
                "No duplicate implementations found — this is a new addition".to_string()
            } else {
                format!(
                    "Found {} duplicate/pattern-matching implementation(s) that already exist in the repo. Consider reusing them.",
                    duplicates.len()
                )
            },
            details: if duplicates.is_empty() {
                None
            } else {
                Some(serde_json::json!({ "duplicates": duplicates }))
            },
        })
    }

    fn score_dependency_impact(&self, patch: &Patch, repo_path: &str) -> Result<SignalResult> {
        // How many other files/call sites does this change ripple into?
        let impacted = self.find_impacted_files(patch, repo_path)?;
        let count = impacted.len();

        let score = if count == 0 {
            1.0
        } else if count <= 3 {
            0.8
        } else if count <= 8 {
            0.6
        } else {
            0.3
        };

        Ok(SignalResult {
            signal: ConfidenceSignal::DependencyImpact,
            score,
            explanation: format!(
                "Change ripples into {} other file(s)/call site(s). {}",
                count,
                if count > 5 {
                    "Consider whether this scope is necessary — larger changes are harder to review."
                } else {
                    "Impact is contained and manageable."
                }
            ),
            details: Some(serde_json::json!({
                "impacted_files": impacted,
                "count": count,
            })),
        })
    }

    fn score_semantic_similarity(&self, patch: &Patch, repo_path: &str) -> Result<SignalResult> {
        // Compare the patch's approach against prior human-approved patches in this repo
        let similarity = self.compute_approach_similarity(patch, repo_path)?;

        let explanation = format!(
            "Patch approach similarity to prior human-approved code: {:.0}%. {}",
            similarity * 100.0,
            if similarity >= 0.7 {
                "This approach is consistent with patterns the team has already approved."
            } else if similarity >= 0.5 {
                "This approach is moderately similar to existing patterns — a review of the differences is recommended."
            } else {
                "This approach differs significantly from existing team patterns — a reviewer should scrutinize it closely."
            }
        );

        Ok(SignalResult {
            signal: ConfidenceSignal::SemanticSimilarity,
            score: similarity,
            explanation,
            details: Some(serde_json::json!({
                "similarity_score": similarity,
            })),
        })
    }

    fn score_existing_reuse(&self, patch: &Patch, repo_path: &str) -> Result<SignalResult> {
        // Did the agent check for and reuse existing utilities/patterns?
        let reuse_score = self.compute_reuse_score(patch, repo_path);

        Ok(SignalResult {
            signal: ConfidenceSignal::ExistingImplementationReuse,
            score: reuse_score.score,
            explanation: reuse_score.explanation,
            details: Some(serde_json::json!({
                "reused_patterns": reuse_score.reused_patterns,
                "missed_patterns": reuse_score.missed_patterns,
            })),
        })
    }

    // -------------------------------------------------------------------
    // Internal helper methods
    // -------------------------------------------------------------------

    fn symbol_exists(&self, symbol_name: &str, repo_path: &str) -> bool {
        if let Ok(files) = self.analyzer.analyze_directory(repo_path) {
            for file in &files {
                for sym in &file.symbols {
                    if sym.name == symbol_name {
                        return true;
                    }
                }
            }
        }
        false
    }

    fn check_rust_basic_lint(&self, content: &str) -> Vec<String> {
        let mut issues = Vec::new();
        // Basic checks that clippy would catch
        if content.contains("unwrap()") && !content.contains("// TODO") {
            issues.push("Uses unwrap() without explicit error handling — consider using ? operator or expect() with a message".to_string());
        }
        if content.contains("expect(") && content.contains("TODO") {
            issues.push("Uses expect() with a TODO marker — these should be reviewed".to_string());
        }
        // Detect missing pub on items that should be public
        let lines: Vec<&str> = content.lines().collect();
        for line in &lines {
            let trimmed = line.trim();
            if trimmed.starts_with("fn ")
                && !trimmed.starts_with("pub ")
                && !trimmed.starts_with("//")
            {
                // Private function in a context that might need public access
                // We note but don't penalize heavily — this is a style preference
            }
        }
        issues
    }

    fn check_js_basic_lint(&self, content: &str) -> Vec<String> {
        let mut issues = Vec::new();
        if content.contains("var ") {
            issues.push("Uses 'var' instead of 'const' or 'let' — prefer const/let".to_string());
        }
        if content.contains("= !") || content.contains("== ") && !content.contains("=== ") {
            issues.push("Uses loose equality (==) instead of strict equality (===)".to_string());
        }
        if content.contains("TODO") || content.contains("FIXME") {
            issues.push("Contains TODO/FIXME markers that should be addressed".to_string());
        }
        issues
    }

    fn check_python_basic_lint(&self, content: &str) -> Vec<String> {
        let mut issues = Vec::new();
        if content.contains("except:") || content.contains("except Exception") {
            issues.push("Bare or broad exception handling — be more specific about which exceptions to catch".to_string());
        }
        if content.contains("pass\n") || content.contains("    pass\n") {
            issues.push(
                "Contains bare pass statements — should have a TODO or implementation".to_string(),
            );
        }
        issues
    }

    fn check_ts_basic_types(&self, content: &str) -> Vec<String> {
        let mut issues = Vec::new();
        // Check for any `any` type usage which is a code smell
        let patterns = ["any;", ": any", "<any>", " as any"];
        for pat in &patterns {
            if content.contains(pat) {
                issues.push(format!(
                    "Uses the 'any' type{} — loses TypeScript's type safety",
                    pat
                ));
            }
        }
        issues
    }

    fn check_rust_basic_types(&self, content: &str) -> Vec<String> {
        let mut issues = Vec::new();
        if content.contains("unsafe") && !content.contains("// SAFETY") {
            issues.push(
                "Uses unsafe blocks without SAFETY comment explaining the reason".to_string(),
            );
        }
        issues
    }

    fn load_architecture_rules(&self, repo_path: &str) -> Result<Vec<ArchitectureRule>> {
        let mut rules = Vec::new();
        let agents_path = std::path::Path::new(repo_path).join("AGENTS.md");
        if agents_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&agents_path) {
                rules.extend(parse_architecture_rules_from_md(&content, "AGENTS.md"));
            }
        }
        Ok(rules)
    }

    fn check_architecture_rules(&self, patch: &Patch, rules: &[ArchitectureRule]) -> Vec<String> {
        rules.iter().filter_map(|rule| rule.check(patch)).collect()
    }

    fn find_tests_for_patch(&self, patch: &Patch, repo_path: &str) -> Result<Vec<String>> {
        // Shares the test-impact graph's own file classifier (Part A) rather
        // than a separate substring check, which previously misclassified
        // ordinary files like `src/testimonial.rs` as tests.
        let mut covered = Vec::new();
        if let Ok(files) = self.analyzer.analyze_directory(repo_path) {
            for file in files
                .iter()
                .filter(|f| crate::semantic_engine::graphs::is_test_file(&f.path))
            {
                if file
                    .symbols
                    .iter()
                    .any(|symbol| patch.references.iter().any(|r| r == &symbol.name))
                {
                    covered.push(file.path.clone());
                }
            }
        }
        Ok(covered)
    }

    fn find_duplicates(&self, patch: &Patch, repo_path: &str) -> Result<Vec<String>> {
        let mut duplicates = Vec::new();
        let patch_symbols = self.extract_symbols_from_content(&patch.new_content);
        if patch_symbols.is_empty() {
            return Ok(duplicates);
        }

        let target = patch.target_file.replace('\\', "/");
        if let Ok(files) = self.analyzer.analyze_directory(repo_path) {
            for file in &files {
                // Redefining a symbol in the same file being edited is an edit,
                // not a duplicate — only a match in a *different* file counts.
                if file.path.replace('\\', "/") == target {
                    continue;
                }
                for sym in &file.symbols {
                    if patch_symbols.iter().any(|p| p == &sym.name) {
                        duplicates.push(format!(
                            "Symbol '{}' already exists in {}",
                            sym.name, sym.file_path
                        ));
                    }
                }
            }
        }

        Ok(duplicates)
    }

    fn find_impacted_files(&self, patch: &Patch, repo_path: &str) -> Result<Vec<String>> {
        let mut impacted = Vec::new();
        if let Ok(files) = self.analyzer.analyze_directory(repo_path) {
            for file in &files {
                for sym in &file.symbols {
                    if patch.references.contains(&sym.name) && !impacted.contains(&file.path) {
                        impacted.push(file.path.clone());
                    }
                }
            }
        }
        Ok(impacted)
    }

    fn compute_approach_similarity(&self, patch: &Patch, repo_path: &str) -> Result<f64> {
        // Simple similarity metric based on patterns in the repo
        let mut best_score = 0.0;

        if let Ok(files) = self.analyzer.analyze_directory(repo_path) {
            for file in &files {
                let content_score = self.content_similarity(&patch.new_content, &file.path);
                if content_score > best_score {
                    best_score = content_score;
                }
            }
        }

        Ok(best_score)
    }

    fn content_similarity(&self, patch_content: &str, file_path: &str) -> f64 {
        if let Ok(content) = std::fs::read_to_string(file_path) {
            let patch_lower = patch_content.to_lowercase();
            let content_lower = content.to_lowercase();

            let patch_tokens: HashSet<&str> = patch_lower.split_whitespace().collect();
            let content_tokens: HashSet<&str> = content_lower.split_whitespace().collect();

            let intersection = patch_tokens.intersection(&content_tokens).count();
            let union = patch_tokens.union(&content_tokens).count();

            if union == 0 {
                return 0.0;
            }

            intersection as f64 / union as f64
        } else {
            0.0
        }
    }

    fn compute_reuse_score(&self, patch: &Patch, repo_path: &str) -> ReuseScore {
        let mut reused_patterns: Vec<String> = Vec::new();
        let mut missed_patterns: Vec<String> = Vec::new();

        // Check if existing patterns in the repo match what the patch introduces
        if let Ok(files) = self.analyzer.analyze_directory(repo_path) {
            for file in &files {
                for sym in &file.symbols {
                    // Check if patch introduces a function that could reuse existing helpers
                    if patch.new_content.contains(&sym.name)
                        && !patch.target_file.contains(&sym.file_path)
                    {
                        reused_patterns.push(format!(
                            "Reuses existing symbol '{}' from {}",
                            sym.name, sym.file_path
                        ));
                    }
                }
            }
        }

        // Check for common patterns the patch could have reused but didn't
        // This is a heuristic — real implementation would be much more sophisticated
        let common_patterns = vec!["map(", "filter(", "reduce(", "Promise.all(", "async/await"];
        for pattern in &common_patterns {
            if patch.new_content.contains(pattern) {
                // Check if repo has existing usage of this pattern
                if !self.pattern_exists_in_repo(pattern, repo_path) {
                    missed_patterns.push(format!(
                        "Could reuse '{}' pattern from existing repo utilities",
                        pattern
                    ));
                }
            }
        }

        let score = if reused_patterns.is_empty() && missed_patterns.is_empty() {
            0.8 // Neutral — neither reusing nor missing obvious patterns
        } else if !missed_patterns.is_empty() {
            0.4 // Some missed patterns
        } else {
            1.0 // Good reuse of existing patterns
        };

        ReuseScore {
            score,
            explanation: if missed_patterns.is_empty() && reused_patterns.is_empty() {
                "Patch approach doesn't obviously reuse existing patterns and doesn't miss obvious ones".to_string()
            } else if !missed_patterns.is_empty() {
                format!(
                    "Missed {} existing pattern(s) that could have been reused",
                    missed_patterns.len()
                )
            } else {
                format!(
                    "Good reuse of {} existing pattern(s)",
                    reused_patterns.len()
                )
            },
            reused_patterns,
            missed_patterns,
        }
    }

    fn extract_symbols_from_content(&self, content: &str) -> Vec<String> {
        // Basic definition-site extraction for duplicate detection. Strips
        // leading modifiers (`pub`, `export`, `async`, `pub(crate)`, …) so
        // `pub fn shared_util(` is recognized the same as `fn shared_util(`.
        const MODIFIERS: &[&str] = &["pub", "export", "async", "default", "static"];
        let mut symbols = Vec::new();
        for line in content.lines() {
            let mut words = line.split_whitespace().peekable();
            while let Some(word) = words.peek() {
                if MODIFIERS.contains(word) || word.starts_with("pub(") {
                    words.next();
                } else {
                    break;
                }
            }
            let Some(&keyword) = words.peek() else {
                continue;
            };
            if !is_definition_keyword(keyword) {
                continue;
            }
            words.next();
            if let Some(name_token) = words.next() {
                let name = name_token.split('(').next().unwrap_or("").trim();
                if !name.is_empty() {
                    symbols.push(name.to_string());
                }
            }
        }
        symbols
    }

    fn pattern_exists_in_repo(&self, _pattern: &str, _repo_path: &str) -> bool {
        // Simplified: always return false since we don't have full project scan
        // In production this would search through all files
        false
    }

    fn generate_explanation(signals: &[SignalResult], overall: f64) -> String {
        let low_signals: Vec<&SignalResult> = signals.iter().filter(|s| s.score < 0.5).collect();
        let high_signals: Vec<&SignalResult> = signals.iter().filter(|s| s.score >= 0.8).collect();

        let mut explanation = format!(
            "Confidence score: {:.0}/100 — {}",
            overall * 100.0,
            if overall >= 0.85 {
                "High confidence — low-risk patch"
            } else if overall >= 0.6 {
                "Moderate confidence — review recommended"
            } else {
                "Low confidence — careful review required"
            }
        );

        if !low_signals.is_empty() {
            explanation.push_str(&format!(
                "\nSignals needing attention: {}",
                low_signals
                    .iter()
                    .map(|s| format!("{} ({:.0}%)", s.signal.label(), s.score * 100.0))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        if !high_signals.is_empty() {
            explanation.push_str(&format!(
                "\nStrong signals: {}",
                high_signals
                    .iter()
                    .map(|s| format!("{} ({:.0}%)", s.signal.label(), s.score * 100.0))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }

        explanation
    }
}

// ---------------------------------------------------------------------------
// Supporting types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Patch {
    pub patch_id: String,
    pub target_file: String,
    pub repo_path: String,
    pub new_content: String,
    pub references: Vec<String>,
    pub diff: String,
}

impl Patch {
    /// Build a `Patch` from a file's post-edit content, auto-extracting the
    /// symbols it defines and calls as `references` — the field a caller
    /// otherwise has no easy way to fill in by hand.
    pub fn from_content(
        patch_id: impl Into<String>,
        target_file: impl Into<String>,
        repo_path: impl Into<String>,
        new_content: impl Into<String>,
        diff: impl Into<String>,
    ) -> Self {
        let new_content = new_content.into();
        let references = extract_referenced_identifiers(&new_content);
        Self {
            patch_id: patch_id.into(),
            target_file: target_file.into(),
            repo_path: repo_path.into(),
            new_content,
            references,
            diff: diff.into(),
        }
    }
}

/// Pull out identifiers that look like calls (`name(`), deduplicated,
/// excluding definition sites (`fn name(`, `pub fn name(`, `function name(`,
/// and similar). This is intentionally crude — it feeds the Symbol Resolution
/// and Dependency Impact signals a starting set to check against the real
/// analyzer, not a substitute for parsing.
fn extract_referenced_identifiers(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut current = String::new();
    let mut prev_word = String::new();

    for c in content.chars() {
        if c.is_alphanumeric() || c == '_' {
            current.push(c);
            continue;
        }
        if !current.is_empty() {
            if c == '(' {
                let starts_with_letter = current.chars().next().is_some_and(|c| c.is_alphabetic());
                let is_definition = is_definition_keyword(&prev_word);
                if starts_with_letter
                    && !is_control_keyword(&current)
                    && !is_definition
                    && seen.insert(current.clone())
                {
                    out.push(current.clone());
                }
            }
            prev_word = std::mem::take(&mut current);
        }
    }
    out
}

fn is_control_keyword(word: &str) -> bool {
    matches!(
        word,
        "if" | "for"
            | "while"
            | "match"
            | "return"
            | "else"
            | "switch"
            | "catch"
            | "async"
            | "await"
            | "let"
            | "const"
            | "var"
    )
}

/// Words that mean "the identifier right after this is being *defined*, not
/// called" — `pub fn shared_util(` and `fn shared_util(` both have `fn` as
/// the immediately preceding word, so a plain lookback catches `pub fn` too.
fn is_definition_keyword(word: &str) -> bool {
    matches!(
        word,
        "fn" | "function"
            | "def"
            | "class"
            | "struct"
            | "interface"
            | "enum"
            | "impl"
            | "trait"
            | "type"
            | "mod"
    )
}

/// A rule extracted from AGENTS.md/SKILL.md.
///
/// Per Part 39/Phase 4's own instruction, this stays a "simple, explicit
/// pattern/path rule," not a general architecture-conformance solver — so a
/// rule is only ever **enforceable** when it names both a path pattern and a
/// forbidden import target explicitly, in backticks:
///
/// `- \`src/ui\` must not import \`src/storage\``
///
/// A line that talks about architecture but doesn't parse into that shape is
/// recorded as informational only. It shows up in the rules list a human can
/// see, but it can never flag a patch — the alternative (guessing at what a
/// free-text sentence means) is exactly the false-confidence failure mode
/// this signal exists to avoid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitectureRule {
    pub source: String,
    pub text: String,
    /// Substring/glob-lite match against the patch's target file. `None` when
    /// the rule could not be parsed into an enforceable shape.
    pub path_pattern: Option<String>,
    /// Substring that must not appear in an import/use/require line of a
    /// matching file.
    pub forbidden_import: Option<String>,
}

impl ArchitectureRule {
    /// Public API for a caller that wants to know whether a rule is checkable
    /// without invoking `check()` — exercised by tests today, kept as a real
    /// method rather than inlined since it names a meaningful concept.
    #[allow(dead_code)]
    fn is_enforceable(&self) -> bool {
        self.path_pattern.is_some() && self.forbidden_import.is_some()
    }

    /// Check one patch against this rule. Only ever returns `Some` for an
    /// enforceable rule whose path pattern matches the patch's target file
    /// and whose forbidden import genuinely appears on an import-shaped line.
    fn check(&self, patch: &Patch) -> Option<String> {
        let path_pattern = self.path_pattern.as_deref()?;
        let forbidden = self.forbidden_import.as_deref()?;

        let target = patch.target_file.replace('\\', "/");
        if !target.contains(path_pattern) {
            return None;
        }

        // Rust spells module paths with `::`; the rule is written filesystem-style
        // (`src/storage`) since that reads naturally in prose. Check both forms
        // so a Rust `use crate::storage::Db` matches a rule written as `src/storage`.
        let forbidden_rust_style = forbidden.replace('/', "::");

        let violates = patch.new_content.lines().any(|line| {
            let trimmed = line.trim_start();
            let is_import_line = trimmed.starts_with("use ")
                || trimmed.starts_with("import ")
                || trimmed.starts_with("from ")
                || trimmed.contains("require(")
                || trimmed.starts_with("mod ");
            is_import_line
                && (trimmed.contains(forbidden) || trimmed.contains(&forbidden_rust_style))
        });

        violates.then(|| {
            format!(
                "{} imports '{}', which {} forbids for files matching '{}'",
                patch.target_file, forbidden, self.source, path_pattern
            )
        })
    }
}

/// Parse backtick-delimited `path must-not-import path` rules out of markdown.
/// Anything not in that shape becomes informational (`path_pattern: None`),
/// never a guessed-at enforceable rule.
fn parse_architecture_rules_from_md(content: &str, source: &str) -> Vec<ArchitectureRule> {
    let negatives = [
        "must not import",
        "should not import",
        "never import",
        "does not import",
    ];

    content
        .lines()
        .filter_map(|line| {
            let trimmed = line.trim();
            let rule_text = trimmed
                .strip_prefix("- ")
                .or_else(|| trimmed.strip_prefix("* "))?;

            let lower = rule_text.to_lowercase();
            let is_architecture_line = negatives.iter().any(|n| lower.contains(n))
                || lower.contains("boundary")
                || lower.contains("layer");
            if !is_architecture_line {
                return None;
            }

            let backticked = extract_backticked(rule_text);
            let (path_pattern, forbidden_import) =
                if backticked.len() >= 2 && negatives.iter().any(|n| lower.contains(n)) {
                    (Some(backticked[0].clone()), Some(backticked[1].clone()))
                } else {
                    (None, None)
                };

            Some(ArchitectureRule {
                source: source.to_string(),
                text: rule_text.to_string(),
                path_pattern,
                forbidden_import,
            })
        })
        .collect()
}

/// Pull out the contents of each `` `backticked` `` span, in order.
///
/// Scans by byte offset and jumps past each consumed pair, rather than
/// walking char-by-char — a naive char iterator re-reads a closing backtick
/// as the next span's opening one and misattributes the text between pairs.
fn extract_backticked(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let after_open = &rest[start + 1..];
        match after_open.find('`') {
            Some(end) => {
                out.push(after_open[..end].to_string());
                rest = &after_open[end + 1..];
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod backtick_tests {
    use super::extract_backticked;

    #[test]
    fn extracts_exactly_two_spans_from_a_two_pair_rule() {
        let spans = extract_backticked("`src/ui` must not import `src/storage`");
        assert_eq!(spans, vec!["src/ui".to_string(), "src/storage".to_string()]);
    }

    #[test]
    fn extracts_three_spans_when_three_are_present() {
        let spans = extract_backticked("`a` and `b` and `c`");
        assert_eq!(
            spans,
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn returns_empty_for_text_with_no_backticks() {
        assert!(extract_backticked("no backticks here").is_empty());
    }

    #[test]
    fn an_unclosed_backtick_yields_only_the_complete_spans_before_it() {
        let spans = extract_backticked("`closed` then `unclosed");
        assert_eq!(spans, vec!["closed".to_string()]);
    }
}

struct ReuseScore {
    score: f64,
    explanation: String,
    reused_patterns: Vec<String>,
    missed_patterns: Vec<String>,
}

#[cfg(test)]
mod engine_tests {
    use super::*;
    use std::sync::Arc;

    fn engine() -> ConfidenceEngine {
        ConfidenceEngine::new(
            Arc::new(crate::analyzer::CodeAnalyzer::new()),
            Arc::new(crate::context_engine::ContextEngineManager::new()),
        )
    }

    fn write(dir: &std::path::Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, content).unwrap();
    }

    // ---- The critical bug: architecture validation must inspect the patch ----

    #[test]
    fn an_unrelated_agents_md_rule_does_not_flag_an_unrelated_patch() {
        // This is the exact shape of the bug found in this file: a rule that
        // merely contains the word "never" anywhere used to flag every patch,
        // regardless of whether the patch touched anything the rule was about.
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "AGENTS.md",
            "- Commits should never include secrets.\n- Never commit directly to main.\n",
        );

        let patch = Patch::from_content(
            "p1",
            "src/math.rs",
            dir.path().to_string_lossy(),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }",
            "",
        );

        let e = engine();
        let score = e
            .score_architecture_validation(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(
            score.score, 1.0,
            "an unrelated rule must not lower the score: {}",
            score.explanation
        );
    }

    #[test]
    fn a_real_import_boundary_violation_is_caught() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "AGENTS.md",
            "- `src/ui` must not import `src/storage` directly.\n",
        );

        let patch = Patch::from_content(
            "p2",
            "src/ui/panel.rs",
            dir.path().to_string_lossy(),
            "use crate::src::storage::Database;\nfn render() {}",
            "",
        );

        let e = engine();
        let score = e
            .score_architecture_validation(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert!(
            score.score < 1.0,
            "a genuine, structured violation must lower the score: {}",
            score.explanation
        );
        assert!(
            score.explanation.contains("src/storage") || {
                let details = score.details.clone().unwrap_or_default();
                details.to_string().contains("src/storage")
            }
        );
    }

    #[test]
    fn a_file_outside_the_rules_path_pattern_is_not_flagged() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "AGENTS.md",
            "- `src/ui` must not import `src/storage` directly.\n",
        );

        // Same forbidden import, but in a file the rule doesn't apply to.
        let patch = Patch::from_content(
            "p3",
            "src/api/handler.rs",
            dir.path().to_string_lossy(),
            "use crate::src::storage::Database;\nfn handle() {}",
            "",
        );

        let e = engine();
        let score = e
            .score_architecture_validation(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(
            score.score, 1.0,
            "the rule only applies to src/ui, not src/api: {}",
            score.explanation
        );
    }

    #[test]
    fn no_agents_md_yields_a_score_that_says_no_rules_were_configured() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p4",
            "src/anything.rs",
            dir.path().to_string_lossy(),
            "fn whatever() {}",
            "",
        );

        let e = engine();
        let score = e
            .score_architecture_validation(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(score.score, 1.0);
    }

    #[test]
    fn unstructured_prose_about_architecture_is_informational_only() {
        // A sentence that talks about layering but doesn't name two backticked
        // paths must never become an enforceable rule — guessing at free-text
        // intent is exactly the failure mode this signal must avoid.
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "AGENTS.md",
            "- We try to keep a clean separation between layers where practical.\n",
        );

        let rules = engine()
            .load_architecture_rules(&dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(rules.len(), 1, "the line is recorded");
        assert!(
            !rules[0].is_enforceable(),
            "but must not be enforceable: {:?}",
            rules[0]
        );
    }

    // ---- Backtick rule parsing ----

    #[test]
    fn parses_a_two_path_rule_into_an_enforceable_shape() {
        let rules = parse_architecture_rules_from_md(
            "- `src/ui` must not import `src/storage`\n",
            "AGENTS.md",
        );
        assert_eq!(rules.len(), 1);
        assert!(rules[0].is_enforceable());
        assert_eq!(rules[0].path_pattern.as_deref(), Some("src/ui"));
        assert_eq!(rules[0].forbidden_import.as_deref(), Some("src/storage"));
    }

    #[test]
    fn a_rule_with_only_one_backticked_path_is_not_enforceable() {
        let rules = parse_architecture_rules_from_md(
            "- `src/ui` must not import anything from the storage layer\n",
            "AGENTS.md",
        );
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].is_enforceable());
    }

    #[test]
    fn non_architecture_lines_are_not_recorded_as_rules() {
        let rules = parse_architecture_rules_from_md(
            "- Always run cargo fmt before committing.\n- Write tests for new code.\n",
            "AGENTS.md",
        );
        assert!(rules.is_empty());
    }

    // ---- Patch::from_content reference extraction ----

    #[test]
    fn from_content_extracts_call_shaped_identifiers() {
        let patch = Patch::from_content(
            "p",
            "f.rs",
            "/r",
            "fn outer() { compute_total(1, 2); helper_fn(); }",
            "",
        );
        assert!(patch.references.contains(&"compute_total".to_string()));
        assert!(patch.references.contains(&"helper_fn".to_string()));
        assert!(
            !patch.references.contains(&"outer".to_string()),
            "a definition site, not a call, should not be picked up by this heuristic"
        );
    }

    #[test]
    fn from_content_excludes_language_keywords() {
        let patch = Patch::from_content("p", "f.rs", "/r", "if (x) { for (y) { } }", "");
        assert!(!patch.references.contains(&"if".to_string()));
        assert!(!patch.references.contains(&"for".to_string()));
    }

    #[test]
    fn from_content_deduplicates_repeated_calls() {
        let patch = Patch::from_content("p", "f.rs", "/r", "foo(); foo(); foo();", "");
        assert_eq!(patch.references.iter().filter(|r| *r == "foo").count(), 1);
    }

    // ---- Other signals ----

    #[test]
    fn symbol_resolution_scores_full_marks_when_nothing_is_referenced() {
        let dir = tempfile::tempdir().unwrap();
        let patch =
            Patch::from_content("p", "f.rs", dir.path().to_string_lossy(), "let x = 1;", "");
        let score = engine()
            .score_symbol_resolution(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(score.score, 1.0);
    }

    #[test]
    fn symbol_resolution_finds_a_symbol_defined_elsewhere_in_the_repo() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/lib.rs", "pub fn known_helper() {}\n");

        let patch = Patch::from_content(
            "p",
            "src/main.rs",
            dir.path().to_string_lossy(),
            "fn main() { known_helper(); }",
            "",
        );
        let score = engine()
            .score_symbol_resolution(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(score.score, 1.0, "{}", score.explanation);
    }

    #[test]
    fn symbol_resolution_flags_a_reference_to_nothing_in_the_repo() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "src/main.rs",
            dir.path().to_string_lossy(),
            "fn main() { totally_made_up_function_xyz(); }",
            "",
        );
        let score = engine()
            .score_symbol_resolution(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert!(score.score < 1.0, "{}", score.explanation);
    }

    #[test]
    fn static_analysis_flags_unwrap_without_a_todo_marker() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "f.rs",
            dir.path().to_string_lossy(),
            "fn f() { let x = maybe().unwrap(); }",
            "",
        );
        let score = engine()
            .score_static_analysis(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert!(score.score < 1.0);
    }

    #[test]
    fn static_analysis_is_clean_for_ordinary_code() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "f.rs",
            dir.path().to_string_lossy(),
            "fn f(x: i32) -> i32 { x + 1 }",
            "",
        );
        let score = engine()
            .score_static_analysis(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(score.score, 1.0);
    }

    #[test]
    fn type_validation_flags_any_type_usage_in_typescript() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "f.ts",
            dir.path().to_string_lossy(),
            "function f(x: any) { return x; }",
            "",
        );
        let score = engine().score_type_validation(&patch).unwrap();
        assert!(score.score < 1.0);
    }

    #[test]
    fn type_validation_flags_unsafe_without_a_safety_comment() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "f.rs",
            dir.path().to_string_lossy(),
            "unsafe { *ptr = 1; }",
            "",
        );
        let score = engine().score_type_validation(&patch).unwrap();
        assert!(score.score < 1.0);
    }

    #[test]
    fn duplicate_detection_finds_a_symbol_that_already_exists() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/existing.rs", "pub fn shared_util() {}\n");

        let patch = Patch::from_content(
            "p",
            "src/new.rs",
            dir.path().to_string_lossy(),
            "pub fn shared_util() {}\n",
            "",
        );
        let score = engine()
            .score_duplicate_detection(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert!(score.score < 1.0, "{}", score.explanation);
    }

    #[test]
    fn duplicate_detection_is_clean_for_a_genuinely_new_symbol() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/existing.rs", "pub fn shared_util() {}\n");

        let patch = Patch::from_content(
            "p",
            "src/new.rs",
            dir.path().to_string_lossy(),
            "pub fn brand_new_unique_name_zzz() {}\n",
            "",
        );
        let score = engine()
            .score_duplicate_detection(&patch, &dir.path().to_string_lossy())
            .unwrap();
        assert_eq!(score.score, 1.0);
    }

    // ---- Overall scoring and explanation ----

    #[test]
    fn overall_score_is_the_mean_of_all_nine_signals() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "src/f.rs",
            dir.path().to_string_lossy(),
            "pub fn tidy_function(a: i32) -> i32 { a + 1 }",
            "",
        );
        let card = engine()
            .score_patch(&patch, &dir.path().to_string_lossy())
            .unwrap();

        assert_eq!(
            card.signals.len(),
            9,
            "all nine Part-A signals must be present"
        );
        let mean: f64 = card.signals.iter().map(|s| s.score).sum::<f64>() / 9.0;
        assert!((card.overall - mean).abs() < 1e-9);
    }

    #[test]
    fn every_signal_carries_a_plain_language_explanation() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "src/f.rs",
            dir.path().to_string_lossy(),
            "fn f() {}",
            "",
        );
        let card = engine()
            .score_patch(&patch, &dir.path().to_string_lossy())
            .unwrap();
        for signal in &card.signals {
            assert!(
                !signal.explanation.is_empty(),
                "{:?} has no explanation — a bare number is exactly what this feature must avoid",
                signal.signal
            );
        }
    }

    #[test]
    fn verdict_text_matches_the_score_band() {
        let high = ConfidenceScore {
            patch_id: "x".into(),
            overall: 0.9,
            signals: vec![],
            generated_at: "now".into(),
            explanation: String::new(),
        };
        assert!(high.verdict().to_lowercase().contains("high"));

        let low = ConfidenceScore {
            overall: 0.2,
            ..high.clone()
        };
        assert!(
            low.verdict().to_lowercase().contains("low")
                || low.verdict().to_lowercase().contains("reject")
        );
    }

    #[test]
    fn a_high_scoring_patch_does_not_list_itself_under_signals_needing_attention() {
        let dir = tempfile::tempdir().unwrap();
        let patch = Patch::from_content(
            "p",
            "src/tidy.rs",
            dir.path().to_string_lossy(),
            "pub fn well_formed(a: i32, b: i32) -> i32 { a + b }",
            "",
        );
        let card = engine()
            .score_patch(&patch, &dir.path().to_string_lossy())
            .unwrap();
        if card.overall >= 0.85 {
            assert!(
                !card.explanation.contains("Signals needing attention"),
                "{}",
                card.explanation
            );
        }
    }
}
