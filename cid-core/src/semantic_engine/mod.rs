//! Phase 2 Semantic Context Engine
//!
//! Hybrid retrieval over a repository:
//!   - Tantivy BM25 full-text index, persisted on disk under `.cid/index` so it
//!     survives a Core restart (Part 18's named search stack).
//!   - Embedding vectors with cosine similarity, blended with the BM25 score.
//!   - A symbol/dependency graph for "what else touches this" queries.
//!   - Ownership overlays from git blame.
//!
//! Opt-in per Repo Channel, off by default (Part 17).

pub mod embeddings;
pub mod graphs;
pub mod index;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::api::types::{
    new_id, now_utc, DependencyEdge, DependencyNode, GitBlameInfo, SemanticIndexStatus,
};
use graphs::{DocGraph, StaleDocEntry, TestImpactEntry, TestImpactGraph};
use index::{IndexChunk, SearchIndex};

/// Split content into identifier-shaped word candidates — used by the
/// test-impact graph to find which known symbols a test file mentions,
/// without a full parse.
pub(crate) fn extract_identifier_like_tokens(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in content.chars() {
        if c.is_alphanumeric() || c == '_' {
            current.push(c);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

#[derive(Debug, Clone)]
struct TextChunk {
    pub file_path: String,
    pub content: String,
    pub line_start: usize,
    pub line_end: usize,
    pub symbol_name: Option<String>,
}

struct RepoIndex {
    status: SemanticIndexStatus,
    chunks: HashMap<String, TextChunk>,
    nodes: HashMap<String, DependencyNode>,
    edges: Vec<DependencyEdge>,
    blame_cache: HashMap<String, Vec<GitBlameInfo>>,
    embeddings: HashMap<String, Vec<f32>>,
    word_index: HashMap<String, HashSet<String>>,
    /// Persistent BM25 index. `None` when the on-disk index could not be opened
    /// (a read-only checkout, for instance) — search then falls back to the
    /// in-memory word index rather than failing outright.
    search: Option<Arc<SearchIndex>>,
    test_impact: TestImpactGraph,
    doc_graph: DocGraph,
    /// Snapshot of every symbol name known to the repo, refreshed on each
    /// scan — both graphs need this to tell "references nothing real" apart
    /// from "references something real."
    known_symbols: HashSet<String>,
}

impl RepoIndex {
    fn new(repo_path: &str) -> Self {
        let search = match SearchIndex::open(repo_path) {
            Ok(idx) => Some(Arc::new(idx)),
            Err(e) => {
                warn!(
                    "Falling back to the in-memory word index for {}: {}",
                    repo_path, e
                );
                None
            }
        };
        Self {
            status: SemanticIndexStatus {
                repo_path: repo_path.to_string(),
                enabled: false,
                indexed_files: 0,
                total_file_chunks: 0,
                dependency_nodes: 0,
                dependency_edges: 0,
                last_indexed_at: None,
                indexing: false,
                embedding_model_ready: false,
            },
            chunks: HashMap::new(),
            nodes: HashMap::new(),
            edges: Vec::new(),
            blame_cache: HashMap::new(),
            embeddings: HashMap::new(),
            word_index: HashMap::new(),
            search,
            test_impact: TestImpactGraph::new(),
            doc_graph: DocGraph::new(),
            known_symbols: HashSet::new(),
        }
    }
}

pub struct SemanticEngine {
    indexes: Arc<RwLock<HashMap<String, RepoIndex>>>,
    analyzer: Arc<crate::analyzer::CodeAnalyzer>,
    /// Set at most once, by `ensure_embedding_model` — see
    /// `embeddings.rs`'s module doc for what "not loaded" degrades to and
    /// why that's an honest fallback rather than a silent correctness bug.
    embedding_model: Arc<std::sync::OnceLock<embeddings::EmbeddingModel>>,
    http_client: reqwest::Client,
}

impl SemanticEngine {
    pub fn new(analyzer: Arc<crate::analyzer::CodeAnalyzer>) -> Self {
        Self {
            indexes: Arc::new(RwLock::new(HashMap::new())),
            analyzer,
            embedding_model: Arc::new(std::sync::OnceLock::new()),
            http_client: reqwest::Client::new(),
        }
    }

    /// Downloads (if needed) and loads the real embedding model, replacing
    /// the hash-based fallback for every `text_to_embedding` call from this
    /// point on in this Core's lifetime. Best-effort: called from `enable()`
    /// but never blocks or fails it — no network, huggingface.co blocked by
    /// a corporate proxy, or disk full all degrade to the hash fallback
    /// rather than refusing to enable the Context Engine at all.
    pub async fn ensure_embedding_model(&self) {
        Self::ensure_embedding_model_static(&self.embedding_model, &self.http_client).await;
    }

    async fn ensure_embedding_model_static(
        embedding_model: &Arc<std::sync::OnceLock<embeddings::EmbeddingModel>>,
        http_client: &reqwest::Client,
    ) {
        if embedding_model.get().is_some() {
            return;
        }
        let dir = match embeddings::ensure_model_downloaded(http_client).await {
            Ok(dir) => dir,
            Err(e) => {
                warn!("Embedding model download failed, using hash-based fallback: {e}");
                return;
            }
        };
        let loaded =
            tokio::task::spawn_blocking(move || embeddings::EmbeddingModel::load(&dir)).await;
        match loaded {
            Ok(Ok(model)) => {
                info!(
                    "Real embedding model loaded ({} dim)",
                    embeddings::EMBEDDING_DIM
                );
                // OnceLock::set fails only if another caller already won the
                // race to set it first — that's success from this caller's
                // perspective too, so the Err case is not logged as a failure.
                let _ = embedding_model.set(model);
            }
            Ok(Err(e)) => warn!("Embedding model failed to load, using hash-based fallback: {e}"),
            Err(e) => warn!("Embedding model load task panicked: {e}"),
        }
    }

    pub fn embedding_model_ready(&self) -> bool {
        self.embedding_model.get().is_some()
    }

    /// Share the index map with a background task so it can record results
    /// after the scan completes.
    fn indexes_handle(&self) -> Arc<RwLock<HashMap<String, RepoIndex>>> {
        self.indexes.clone()
    }

    pub async fn status(&self, repo_path: &str) -> SemanticIndexStatus {
        let guard = self.indexes.read().await;
        let mut status = guard
            .get(repo_path)
            .map(|idx| idx.status.clone())
            .unwrap_or_else(|| SemanticIndexStatus {
                repo_path: repo_path.to_string(),
                enabled: false,
                indexed_files: 0,
                total_file_chunks: 0,
                dependency_nodes: 0,
                dependency_edges: 0,
                last_indexed_at: None,
                indexing: false,
                embedding_model_ready: false,
            });
        status.embedding_model_ready = self.embedding_model_ready();
        status
    }

    pub async fn enable(&self, repo_path: &str) -> anyhow::Result<SemanticIndexStatus> {
        let mut guard = self.indexes.write().await;
        let index = guard
            .entry(repo_path.to_string())
            .or_insert_with(|| RepoIndex::new(repo_path));
        index.status.enabled = true;
        debug!("Semantic engine enabled for {}", repo_path);

        index.status.indexing = true;
        let idx_clone = index.status.clone();
        let search = index.search.clone();
        drop(guard);

        // Indexing is a background scan so `enable` returns immediately; status
        // reports `indexing: true` until it finishes.
        let repo = repo_path.to_string();
        let engine_indexes = self.indexes_handle();
        let analyzer = self.analyzer.clone();
        let embedding_model = self.embedding_model.clone();
        let http_client = self.http_client.clone();
        tokio::spawn(async move {
            // Awaited here, inside the background task — not before
            // `enable()` returns to its caller — so the very first scan for
            // a repo already benefits from real embeddings when the model
            // is available, without making the RPC caller wait for a
            // multi-second (or, on first-ever use, multi-tens-of-seconds
            // download) model load.
            Self::ensure_embedding_model_static(&embedding_model, &http_client).await;

            let outcome = tokio::task::spawn_blocking({
                let repo = repo.clone();
                move || index_repository_blocking(&repo, search.as_deref())
            })
            .await;

            // Test-impact and doc graphs need real symbol data, which the
            // Tantivy scan above doesn't produce — run the Structural Context
            // Engine's own analyzer over the same repo for that.
            let graphs = tokio::task::spawn_blocking({
                let repo = repo.clone();
                move || build_graphs(&analyzer, &repo)
            })
            .await;

            let mut guard = engine_indexes.write().await;
            if let Some(entry) = guard.get_mut(&repo) {
                if let Ok((test_impact, doc_graph, known_symbols)) = graphs {
                    entry.test_impact = test_impact;
                    entry.doc_graph = doc_graph;
                    entry.known_symbols = known_symbols;
                }
                entry.status.indexing = false;
                match outcome {
                    Ok(Ok(stats)) => {
                        entry.status.indexed_files = stats.files;
                        entry.status.total_file_chunks = stats.chunks;
                        entry.status.last_indexed_at = Some(now_utc());
                        info!(
                            "Indexed {} files ({} chunks) in {}",
                            stats.files, stats.chunks, repo
                        );
                    }
                    Ok(Err(e)) => warn!("Failed to index repository {}: {}", repo, e),
                    Err(e) => warn!("Indexing task for {} panicked: {}", repo, e),
                }
            }
        });

        Ok(idx_clone)
    }

    pub async fn disable(&self, repo_path: &str) -> anyhow::Result<()> {
        let mut guard = self.indexes.write().await;
        if let Some(index) = guard.get_mut(repo_path) {
            index.status.enabled = false;
        }
        Ok(())
    }

    pub async fn search(
        &self,
        query: &str,
        repo_path: &str,
        limit: usize,
        include_dependencies: bool,
        include_blame: bool,
    ) -> Vec<crate::api::types::SemanticSearchResult> {
        let guard = self.indexes.read().await;
        let index = match guard.get(repo_path) {
            Some(idx) => idx,
            None => return vec![],
        };

        // Tantivy is the primary retriever: it gives real BM25 ranking over a
        // persisted index. The in-memory scan below remains as a fallback for
        // repos whose on-disk index could not be opened, and for chunks added
        // via `index_file` before a full scan has run.
        if let Some(search_index) = index.search.as_ref() {
            match search_index.search(query, limit) {
                Ok(hits) if !hits.is_empty() => {
                    let query_embedding = self.text_to_embedding(query);
                    let max_score = hits
                        .iter()
                        .map(|h| h.score)
                        .fold(0.0f32, f32::max)
                        .max(1e-6);

                    return hits
                        .into_iter()
                        .map(|hit| {
                            // BM25 scores are unbounded; normalise against the top
                            // hit so the blend with cosine similarity is meaningful.
                            let lexical = (hit.score / max_score) as f64;
                            let semantic = index
                                .embeddings
                                .get(&hit.file_path)
                                .map(|emb| cosine_similarity(&query_embedding, emb))
                                .unwrap_or(0.0);

                            let deps = if include_dependencies {
                                self.get_dependencies_for_file(index, &hit.file_path)
                            } else {
                                vec![]
                            };
                            let blame = if include_blame {
                                index
                                    .blame_cache
                                    .get(&hit.file_path)
                                    .and_then(|b| b.get(hit.line_start.saturating_sub(1)).cloned())
                            } else {
                                None
                            };

                            crate::api::types::SemanticSearchResult {
                                file_path: hit.file_path,
                                content: hit.content,
                                score: lexical * 0.7 + semantic * 0.3,
                                line: Some(hit.line_start),
                                symbol_name: hit.symbol_name,
                                dependencies: deps,
                                blame,
                            }
                        })
                        .collect();
                }
                Ok(_) => debug!(
                    "Tantivy returned no hits for {:?}; falling back to scan",
                    query
                ),
                Err(e) => warn!("Tantivy search failed, falling back to scan: {}", e),
            }
        }

        let query_lower = query.to_lowercase();
        let query_terms: Vec<&str> = query_lower.split_whitespace().collect();

        let candidates = if query_terms.is_empty() {
            HashSet::new()
        } else {
            let mut candidates: HashSet<String> = HashSet::new();
            for term in &query_terms {
                let term_lower = term.to_string();
                let prefix_len = term_lower.len().min(4);

                for (word, chunks) in &index.word_index {
                    if word.starts_with(&term_lower[..prefix_len]) || word.contains(&*term_lower) {
                        candidates.extend(chunks.iter().cloned());
                    }
                }
            }
            candidates
        };

        let mut results: Vec<(String, f64)> = Vec::new();

        if candidates.is_empty() {
            debug!("No word index candidates, using full scan");
            for (chunk_id, chunk) in &index.chunks {
                let content_lower = chunk.content.to_lowercase();
                let score = self.score_content(&content_lower, &query_terms);
                if score > 0.0 {
                    results.push((chunk_id.clone(), score));
                }
            }
        } else {
            for chunk_id in &candidates {
                if let Some(chunk) = index.chunks.get(chunk_id) {
                    let content_lower = chunk.content.to_lowercase();
                    let score = self.score_content(&content_lower, &query_terms);
                    if score > 0.0 {
                        results.push((chunk_id.clone(), score));
                    }
                }
            }
        }

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(limit);

        let embedding_scores = self.search_embeddings(index, query, &results);

        results
            .into_iter()
            .map(|(chunk_id, term_score)| {
                let emb_score = embedding_scores.get(&chunk_id).copied().unwrap_or(0.0);
                let combined_score = term_score * 0.7 + emb_score * 0.3;
                let chunk = index.chunks.get(&chunk_id);

                let deps = match chunk {
                    Some(c) if include_dependencies => {
                        self.get_dependencies_for_file(index, &c.file_path)
                    }
                    _ => vec![],
                };

                let blame = match chunk {
                    Some(c) if include_blame => index
                        .blame_cache
                        .get(&c.file_path)
                        .and_then(|blames| blames.get(c.line_start.saturating_sub(1)).cloned()),
                    _ => None,
                };

                crate::api::types::SemanticSearchResult {
                    file_path: chunk.map(|c| c.file_path.clone()).unwrap_or_default(),
                    content: chunk.map(|c| c.content.clone()).unwrap_or_default(),
                    score: combined_score,
                    line: chunk.map(|c| c.line_start),
                    symbol_name: chunk.and_then(|c| c.symbol_name.clone()),
                    dependencies: deps,
                    blame,
                }
            })
            .collect()
    }

    fn score_content(&self, content: &str, terms: &[&str]) -> f64 {
        if terms.is_empty() {
            return 0.0;
        }
        let mut score: f64 = 0.0;
        for term in terms {
            if content.contains(*term) {
                let count = content.matches(*term).count() as f64;
                let density = count / content.len().max(1) as f64;
                score += 1.0 + density * 10.0;
            }
        }
        score / terms.len() as f64
    }

    fn search_embeddings(
        &self,
        index: &RepoIndex,
        query: &str,
        candidates: &[(String, f64)],
    ) -> HashMap<String, f64> {
        let query_embedding = self.text_to_embedding(query);
        let mut scores = HashMap::new();

        for (chunk_id, _) in candidates {
            if let Some(emb) = index.embeddings.get(chunk_id) {
                let sim = cosine_similarity(&query_embedding, emb);
                scores.insert(chunk_id.clone(), sim);
            }
        }

        scores
    }

    /// Real model when `ensure_embedding_model` has successfully loaded one
    /// this Core run, the previous hash-based projection otherwise. See
    /// `embeddings.rs`'s module doc for the honest limitation this implies
    /// about mixing embeddings computed before/after the model becomes
    /// available.
    fn text_to_embedding(&self, text: &str) -> Vec<f32> {
        if let Some(model) = self.embedding_model.get() {
            match model.embed(text) {
                Ok(v) => return v,
                Err(e) => {
                    warn!(
                        "Real embedding inference failed, falling back to hash for this call: {e}"
                    );
                }
            }
        }
        Self::hash_embedding(text)
    }

    /// The original deterministic projection (review_prompt.md /
    /// Gemini-checklist: "a hash-based mathematical projection rather than a
    /// learned neural model"). Kept as the fallback `text_to_embedding` uses
    /// when the real model isn't loaded — better than no embedding at all,
    /// worse than a real one, and never silently claimed to be the latter.
    fn hash_embedding(text: &str) -> Vec<f32> {
        let bytes = text.as_bytes();
        let dim = 64;
        let mut embedding = vec![0.0f32; dim];

        for (i, &b) in bytes.iter().enumerate() {
            let idx = i % dim;
            embedding[idx] += (b as f32) / 255.0;
            embedding[(idx * 7 + 3) % dim] += ((b as f32) * 0.5) / 255.0;
        }

        let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for v in &mut embedding {
                *v /= norm;
            }
        }

        embedding
    }

    pub async fn dependency_graph(
        &self,
        repo_path: &str,
        file_path: Option<&str>,
        symbol_name: Option<&str>,
        depth: Option<usize>,
    ) -> anyhow::Result<(Vec<DependencyNode>, Vec<DependencyEdge>)> {
        let guard = self.indexes.read().await;
        let index = match guard.get(repo_path) {
            Some(idx) => idx,
            None => return Ok((vec![], vec![])),
        };

        let max_depth = depth.unwrap_or(3).min(10);

        let mut visited: HashSet<String> = HashSet::new();
        let mut result_nodes: Vec<DependencyNode> = Vec::new();
        let mut result_edges: Vec<DependencyEdge> = Vec::new();

        let start_nodes: Vec<String> = if let Some(sym) = symbol_name {
            index
                .nodes
                .values()
                .filter(|n| n.name.to_lowercase().contains(&sym.to_lowercase()))
                .map(|n| n.id.clone())
                .collect()
        } else if let Some(fp) = file_path {
            index
                .nodes
                .values()
                .filter(|n| n.file_path == fp)
                .map(|n| n.id.clone())
                .collect()
        } else {
            index.nodes.keys().cloned().collect()
        };

        let mut current = start_nodes;
        for _ in 0..max_depth {
            let mut next: HashSet<String> = HashSet::new();
            for node_id in &current {
                if !visited.insert(node_id.clone()) {
                    continue;
                }
                if let Some(node) = index.nodes.get(node_id) {
                    result_nodes.push(node.clone());
                }
                for edge in &index.edges {
                    if &edge.from == node_id {
                        result_edges.push(edge.clone());
                        next.insert(edge.to.clone());
                    }
                    if &edge.to == node_id {
                        result_edges.push(edge.clone());
                        next.insert(edge.from.clone());
                    }
                }
            }
            current = next.into_iter().collect();
        }

        Ok((result_nodes, result_edges))
    }

    pub async fn git_blame(
        &self,
        repo_path: &str,
        file_path: &str,
        line: Option<usize>,
    ) -> Option<Vec<GitBlameInfo>> {
        let guard = self.indexes.read().await;
        let index = guard.get(repo_path)?;

        let blames = index.blame_cache.get(file_path)?;

        if let Some(l) = line {
            blames.get(l.saturating_sub(1)).map(|b| vec![b.clone()])
        } else {
            Some(blames.clone())
        }
    }

    pub async fn index_file(
        &self,
        repo_path: &str,
        file_path: &str,
        content: &str,
    ) -> anyhow::Result<()> {
        let mut guard = self.indexes.write().await;
        let index = guard
            .entry(repo_path.to_string())
            .or_insert_with(|| RepoIndex::new(repo_path));

        let chunks = self.chunk_content(file_path, content);

        // Drop this file's previous chunks first — incremental refresh on file
        // change must not accumulate stale copies of edited code.
        let stale: Vec<String> = index
            .chunks
            .iter()
            .filter(|(_, c)| c.file_path == file_path)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale {
            index.chunks.remove(id);
            index.embeddings.remove(id);
        }
        if !stale.is_empty() {
            let stale_set: HashSet<&String> = stale.iter().collect();
            index
                .word_index
                .values_mut()
                .for_each(|ids| ids.retain(|id| !stale_set.contains(id)));
            index.word_index.retain(|_, ids| !ids.is_empty());
        }

        for chunk in &chunks {
            let chunk_id = new_id();
            let embedding = self.text_to_embedding(&chunk.content);

            for word in chunk.content.split_whitespace() {
                let word_lower = word
                    .trim_matches(|c: char| !c.is_alphanumeric())
                    .to_lowercase();
                if !word_lower.is_empty() {
                    index
                        .word_index
                        .entry(word_lower)
                        .or_default()
                        .insert(chunk_id.clone());
                }
            }

            index.chunks.insert(chunk_id.clone(), chunk.clone());
            index.embeddings.insert(chunk_id, embedding);
        }

        // Mirror into the persistent index so a restart keeps this file.
        if let Some(search) = index.search.as_ref() {
            let persisted: Vec<IndexChunk> = chunks
                .iter()
                .map(|c| IndexChunk {
                    content: c.content.clone(),
                    symbol_name: c.symbol_name.clone(),
                    line_start: c.line_start,
                    line_end: c.line_end,
                })
                .collect();
            if let Err(e) = search.replace_file(file_path, &persisted) {
                warn!("Failed to persist index for {}: {}", file_path, e);
            }
        }

        self.extract_dependencies(index, file_path, content);

        // Incremental refresh for whichever graph this file belongs to —
        // Part 7's "refreshed incrementally on file change," not a full
        // repository rescan for a single edited file.
        let known: HashSet<&str> = index.known_symbols.iter().map(|s| s.as_str()).collect();
        if graphs::is_test_file(file_path) {
            index.test_impact.update_file(file_path, content, &known);
        } else if file_path.ends_with(".md") || file_path.ends_with(".mdx") {
            index.doc_graph.update_doc(file_path, content);
        }

        index.status.indexed_files += 1;
        index.status.total_file_chunks += chunks.len();
        index.status.dependency_nodes = index.nodes.len();
        index.status.dependency_edges = index.edges.len();
        index.status.last_indexed_at = Some(now_utc());

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Test-impact graph queries
    // -----------------------------------------------------------------------

    pub async fn tests_for_symbol(&self, repo_path: &str, symbol: &str) -> Vec<String> {
        let guard = self.indexes.read().await;
        guard
            .get(repo_path)
            .map(|idx| idx.test_impact.tests_for_symbol(symbol))
            .unwrap_or_default()
    }

    pub async fn tests_for_symbols(&self, repo_path: &str, symbols: &[String]) -> Vec<String> {
        let guard = self.indexes.read().await;
        guard
            .get(repo_path)
            .map(|idx| idx.test_impact.tests_for_symbols(symbols))
            .unwrap_or_default()
    }

    pub async fn test_impact_entries(&self, repo_path: &str) -> Vec<TestImpactEntry> {
        let guard = self.indexes.read().await;
        guard
            .get(repo_path)
            .map(|idx| idx.test_impact.entries())
            .unwrap_or_default()
    }

    // -----------------------------------------------------------------------
    // Documentation graph queries
    // -----------------------------------------------------------------------

    pub async fn docs_for_symbol(&self, repo_path: &str, symbol: &str) -> Vec<String> {
        let guard = self.indexes.read().await;
        guard
            .get(repo_path)
            .map(|idx| idx.doc_graph.docs_for_symbol(symbol))
            .unwrap_or_default()
    }

    pub async fn stale_docs(&self, repo_path: &str) -> Vec<StaleDocEntry> {
        let guard = self.indexes.read().await;
        match guard.get(repo_path) {
            Some(idx) => {
                let known: HashSet<&str> = idx.known_symbols.iter().map(|s| s.as_str()).collect();
                idx.doc_graph.stale_docs(&known)
            }
            None => vec![],
        }
    }

    pub async fn load_git_blame(
        &self,
        repo_path: &str,
        file_path: &str,
        blames: Vec<GitBlameInfo>,
    ) {
        let mut guard = self.indexes.write().await;
        let index = guard
            .entry(repo_path.to_string())
            .or_insert_with(|| RepoIndex::new(repo_path));
        index.blame_cache.insert(file_path.to_string(), blames);
    }

    fn chunk_content(&self, file_path: &str, content: &str) -> Vec<TextChunk> {
        let lines: Vec<&str> = content.lines().collect();
        let chunk_size = 50;
        let mut chunks = Vec::new();

        for (i, window) in lines.chunks(chunk_size).enumerate() {
            chunks.push(TextChunk {
                file_path: file_path.to_string(),
                content: window.join("\n"),
                line_start: i * chunk_size + 1,
                line_end: ((i + 1) * chunk_size).min(lines.len()),
                symbol_name: None,
            });
        }

        chunks
    }

    fn extract_dependencies(&self, index: &mut RepoIndex, file_path: &str, content: &str) {
        for (_line_num, line) in content.lines().enumerate() {
            let trimmed = line.trim();

            if trimmed.starts_with("use ")
                || trimmed.starts_with("import ")
                || trimmed.starts_with("from ")
            {
                let parts: Vec<&str> = trimmed.split_whitespace().collect();
                if parts.len() >= 2 {
                    let dep_path = parts[1].trim_end_matches(';').trim_matches('"');
                    let current_id = format!("{}:{}", file_path, trimmed);
                    let dep_id = format!("dep:{}", dep_path);

                    index
                        .nodes
                        .entry(current_id.clone())
                        .or_insert_with(|| DependencyNode {
                            id: current_id.clone(),
                            name: trimmed.to_string(),
                            kind: "import".to_string(),
                            file_path: file_path.to_string(),
                            line: _line_num + 1,
                            metadata: serde_json::json!({}),
                        });

                    index.edges.push(DependencyEdge {
                        from: current_id,
                        to: dep_id,
                        relation: "imports".to_string(),
                        file_path: Some(file_path.to_string()),
                    });
                }
            }
        }
    }

    fn get_dependencies_for_file(&self, index: &RepoIndex, file_path: &str) -> Vec<String> {
        index
            .edges
            .iter()
            .filter(|e| {
                e.file_path.as_deref() == Some(file_path)
                    || e.from.starts_with(file_path)
                    || e.to.starts_with(file_path)
            })
            .map(|e| {
                if e.from.starts_with(file_path) {
                    e.to.clone()
                } else {
                    e.from.clone()
                }
            })
            .collect()
    }
}

pub struct IndexStats {
    pub files: usize,
    pub chunks: usize,
}

const INDEXABLE_EXTENSIONS: &[&str] = &[
    "rs", "ts", "tsx", "js", "jsx", "py", "go", "java", "rb", "c", "h", "cpp", "hpp", "cs", "php",
    "swift", "kt", "scala", "sh", "json", "md", "toml", "yaml", "yml",
];

const SKIP_DIRS: &[&str] = &[
    ".git",
    ".cid",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".venv",
];

/// Files above this size are almost always generated or vendored; indexing them
/// costs far more than it returns.
const MAX_INDEXABLE_BYTES: u64 = 1_000_000;

/// Walk a repository and write its contents into the search index.
///
/// Synchronous and CPU/IO-bound by nature, so callers run it on the blocking
/// pool rather than holding up the async runtime.
/// Run the Structural Context Engine's own analyzer over a repo to get real
/// symbol data, then build the test-impact and documentation graphs from it.
/// Synchronous and CPU-bound — callers run it on the blocking pool.
fn build_graphs(
    analyzer: &crate::analyzer::CodeAnalyzer,
    repo_path: &str,
) -> (TestImpactGraph, DocGraph, HashSet<String>) {
    let files = analyzer.analyze_directory(repo_path).unwrap_or_default();
    let known_symbols: HashSet<String> = files
        .iter()
        .filter(|f| !graphs::is_test_file(&f.path))
        .flat_map(|f| f.symbols.iter().map(|s| s.name.clone()))
        .collect();

    let test_contents: Vec<(String, String)> = files
        .iter()
        .filter(|f| graphs::is_test_file(&f.path))
        .filter_map(|f| {
            std::fs::read_to_string(&f.path)
                .ok()
                .map(|content| (f.path.clone(), content))
        })
        .collect();
    let test_impact = TestImpactGraph::build(&files, &test_contents);

    let doc_paths = collect_doc_paths(repo_path);
    let doc_contents: Vec<(String, String)> = doc_paths
        .into_iter()
        .filter_map(|p| {
            std::fs::read_to_string(&p)
                .ok()
                .map(|content| (p.to_string_lossy().replace('\\', "/"), content))
        })
        .collect();
    let doc_graph = DocGraph::build(&doc_contents);

    (test_impact, doc_graph, known_symbols)
}

fn collect_doc_paths(repo_path: &str) -> Vec<std::path::PathBuf> {
    use walkdir::WalkDir;
    WalkDir::new(repo_path)
        .into_iter()
        .filter_entry(|e| {
            e.depth() == 0
                || e.file_name()
                    .to_str()
                    .map(|n| !SKIP_DIRS.contains(&n))
                    .unwrap_or(false)
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|ext| ext.to_str())
                .map(|ext| ext == "md" || ext == "mdx")
                .unwrap_or(false)
        })
        .map(|e| e.path().to_path_buf())
        .collect()
}

pub fn index_repository_blocking(
    repo_path: &str,
    search: Option<&SearchIndex>,
) -> anyhow::Result<IndexStats> {
    use walkdir::WalkDir;

    let mut batch: Vec<(String, Vec<IndexChunk>)> = Vec::new();
    let mut files = 0usize;
    let mut chunks = 0usize;

    let walker = WalkDir::new(repo_path).into_iter().filter_entry(|e| {
        if e.depth() == 0 {
            return true;
        }
        e.file_name()
            .to_str()
            .map(|name| !SKIP_DIRS.contains(&name))
            .unwrap_or(false)
    });

    for entry in walker.filter_map(|e| e.ok()).filter(|e| e.path().is_file()) {
        let path = entry.path();
        let indexable = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|ext| INDEXABLE_EXTENSIONS.contains(&ext))
            .unwrap_or(false);
        if !indexable {
            continue;
        }
        if entry
            .metadata()
            .map(|m| m.len() > MAX_INDEXABLE_BYTES)
            .unwrap_or(true)
        {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            // Binary or non-UTF8 content is skipped rather than failing the scan.
            Err(_) => continue,
        };

        let rel_path = path
            .strip_prefix(repo_path)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");

        let file_chunks = chunk_source(&content);
        chunks += file_chunks.len();
        files += 1;
        batch.push((rel_path, file_chunks));

        // Commit in batches so a very large repo does not build one enormous
        // in-memory batch before the first document is searchable.
        if batch.len() >= 500 {
            if let Some(idx) = search {
                idx.replace_files(&batch)?;
            }
            batch.clear();
        }
    }

    if let Some(idx) = search {
        if !batch.is_empty() {
            idx.replace_files(&batch)?;
        }
    }

    debug!("index scan of {} produced {} chunks", repo_path, chunks);
    Ok(IndexStats { files, chunks })
}

/// Split a source file into overlapping line windows.
///
/// Windows overlap so a symbol near a boundary is still retrievable with the
/// lines around it, which a hard split would lose.
pub(crate) fn chunk_source(content: &str) -> Vec<IndexChunk> {
    const WINDOW: usize = 60;
    const STRIDE: usize = 45;

    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return vec![];
    }

    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + WINDOW).min(lines.len());
        let body = lines[start..end].join("\n");
        if !body.trim().is_empty() {
            chunks.push(IndexChunk {
                symbol_name: leading_symbol(&lines[start..end]),
                content: body,
                line_start: start + 1,
                line_end: end,
            });
        }
        if end == lines.len() {
            break;
        }
        start += STRIDE;
    }
    chunks
}

/// Best-effort name for a chunk: the first declaration it contains. Used to
/// boost symbol matches, not as a substitute for real parsing.
fn leading_symbol(lines: &[&str]) -> Option<String> {
    const KEYWORDS: &[&str] = &[
        "fn ",
        "func ",
        "def ",
        "class ",
        "struct ",
        "interface ",
        "enum ",
        "impl ",
        "type ",
        "const ",
        "export function ",
    ];
    for line in lines {
        let trimmed = line.trim_start();
        for kw in KEYWORDS {
            if let Some(rest) = trimmed.strip_prefix(kw) {
                let name: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !name.is_empty() {
                    return Some(name);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod scan_tests {
    use super::*;

    fn write(dir: &std::path::Path, rel: &str, content: &str) {
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn scanning_a_repository_actually_indexes_its_files() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            "src/auth.rs",
            "fn validate_credentials() -> bool { true }",
        );
        write(dir.path(), "src/api.rs", "pub fn handle_request() {}");

        let index = SearchIndex::in_memory().unwrap();
        let stats = index_repository_blocking(&dir.path().to_string_lossy(), Some(&index)).unwrap();

        assert_eq!(stats.files, 2, "both source files should be indexed");
        assert!(stats.chunks >= 2);
        assert_eq!(
            index.search("validate_credentials", 10).unwrap().len(),
            1,
            "a scanned repository must be searchable"
        );
    }

    #[test]
    fn the_scan_skips_build_output_and_vcs_directories() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "src/real.rs", "fn kept_symbol() {}");
        write(
            dir.path(),
            "target/debug/gen.rs",
            "fn build_artifact_symbol() {}",
        );
        write(
            dir.path(),
            "node_modules/pkg/index.js",
            "function vendored_symbol() {}",
        );
        write(
            dir.path(),
            ".git/hooks/sample.sh",
            "echo git_internal_symbol",
        );

        let index = SearchIndex::in_memory().unwrap();
        let stats = index_repository_blocking(&dir.path().to_string_lossy(), Some(&index)).unwrap();

        assert_eq!(stats.files, 1, "only src/real.rs should be indexed");
        assert!(index
            .search("build_artifact_symbol", 10)
            .unwrap()
            .is_empty());
        assert!(index.search("vendored_symbol", 10).unwrap().is_empty());
        assert_eq!(index.search("kept_symbol", 10).unwrap().len(), 1);
    }

    #[test]
    fn the_scan_skips_unsupported_extensions() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "notes.docx", "unsupported_extension_symbol");
        write(dir.path(), "image.png", "not really a png");

        let index = SearchIndex::in_memory().unwrap();
        let stats = index_repository_blocking(&dir.path().to_string_lossy(), Some(&index)).unwrap();
        assert_eq!(stats.files, 0);
    }

    #[test]
    fn chunks_overlap_so_boundary_code_stays_retrievable() {
        let source: String = (1..=150).map(|i| format!("line {i}\n")).collect();
        let chunks = chunk_source(&source);
        assert!(
            chunks.len() > 1,
            "a 150-line file should produce several chunks"
        );

        // Consecutive windows must share lines, otherwise a symbol on a boundary
        // loses the context around it.
        assert!(
            chunks[1].line_start <= chunks[0].line_end,
            "windows should overlap: {:?} then {:?}",
            (chunks[0].line_start, chunks[0].line_end),
            (chunks[1].line_start, chunks[1].line_end)
        );
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks.last().unwrap().line_end, 150);
    }

    #[test]
    fn an_empty_file_produces_no_chunks() {
        assert!(chunk_source("").is_empty());
        assert!(chunk_source("\n\n   \n").is_empty());
    }

    #[test]
    fn chunk_symbol_names_come_from_declarations() {
        let chunks = chunk_source("use std::io;\n\nfn compute_total() -> u32 { 0 }\n");
        assert_eq!(chunks[0].symbol_name.as_deref(), Some("compute_total"));

        let chunks = chunk_source("export function renderPage() {}\n");
        assert_eq!(chunks[0].symbol_name.as_deref(), Some("renderPage"));

        let chunks = chunk_source("just some prose with no declarations\n");
        assert_eq!(chunks[0].symbol_name, None);
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    let len = a.len().min(b.len());
    if len == 0 {
        return 0.0;
    }

    let (dot, norm_a, norm_b) = a[..len].iter().zip(b[..len].iter()).fold(
        (0.0f64, 0.0f64, 0.0f64),
        |(d, na, nb), (&x, &y)| {
            (
                d + x as f64 * y as f64,
                na + x as f64 * x as f64,
                nb + y as f64 * y as f64,
            )
        },
    );

    let denom = (norm_a.sqrt() * norm_b.sqrt()).max(1e-10);
    (dot / denom).clamp(0.0, 1.0)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cosine_similarity_identical() {
        let v1 = vec![1.0, 0.0, 0.0];
        let v2 = vec![1.0, 0.0, 0.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_cosine_similarity_orthogonal() {
        let v1 = vec![1.0, 0.0];
        let v2 = vec![0.0, 1.0];
        let sim = cosine_similarity(&v1, &v2);
        assert!((sim - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_embedding_deterministic() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let e1 = engine.text_to_embedding("hello world");
        let e2 = engine.text_to_embedding("hello world");
        assert_eq!(e1.len(), e2.len());
        for (a, b) in e1.iter().zip(e2.iter()) {
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn test_chunk_content() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let content = (0..120)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let chunks = engine.chunk_content("test.rs", &content);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].line_start, 1);
        assert_eq!(chunks[0].line_end, 50);
    }

    #[test]
    fn test_score_content() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let score = engine.score_content("hello world this is a test", &["hello", "test"]);
        assert!(score > 0.0);

        let score = engine.score_content("completely different", &["hello"]);
        assert_eq!(score, 0.0);
    }

    #[tokio::test]
    async fn test_engine_creation() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let status = engine.status("/test/repo").await;
        assert!(!status.enabled);
        assert_eq!(status.repo_path, "/test/repo");
    }

    #[tokio::test]
    async fn test_enable_disable() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let status = engine.enable("/test/repo").await.unwrap();
        assert!(status.enabled);

        engine.disable("/test/repo").await.unwrap();
        let status = engine.status("/test/repo").await;
        assert!(!status.enabled);
    }

    #[tokio::test]
    async fn test_index_file_and_search() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        engine.enable("/test/repo").await.unwrap();

        let content = "fn main() {\n    println!(\"Hello world\");\n}\n\npub fn calculate_sum(a: i32, b: i32) -> i32 {\n    a + b\n}\n";
        engine
            .index_file("/test/repo", "src/main.rs", content)
            .await
            .unwrap();

        let results = engine
            .search("calculate_sum", "/test/repo", 10, false, false)
            .await;
        assert!(!results.is_empty());
    }

    #[tokio::test]
    async fn test_search_empty_index() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let results = engine
            .search("nothing", "/test/repo", 10, false, false)
            .await;
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_dependency_graph() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        engine.enable("/test/repo").await.unwrap();

        let content = "use std::collections::HashMap;\nuse crate::utils::helper;\n\nfn test() {}\n";
        engine
            .index_file("/test/repo", "src/lib.rs", content)
            .await
            .unwrap();

        let (nodes, edges) = engine
            .dependency_graph("/test/repo", Some("src/lib.rs"), None, Some(2))
            .await
            .unwrap();

        assert!(!nodes.is_empty());
        assert!(!edges.is_empty());
    }

    #[tokio::test]
    async fn test_git_blame() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        engine.enable("/test/repo").await.unwrap();

        let blames = vec![GitBlameInfo {
            file_path: "src/main.rs".to_string(),
            line: 1,
            author: "dev".to_string(),
            email: "dev@example.com".to_string(),
            commit_hash: "abc123".to_string(),
            commit_date: now_utc(),
            commit_summary: "initial commit".to_string(),
        }];

        engine
            .load_git_blame("/test/repo", "src/main.rs", blames)
            .await;

        let result = engine.git_blame("/test/repo", "src/main.rs", None).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().len(), 1);

        let line_result = engine.git_blame("/test/repo", "src/main.rs", Some(1)).await;
        assert!(line_result.is_some());
    }

    #[tokio::test]
    async fn test_dependency_graph_empty() {
        let engine = SemanticEngine::new(Arc::new(crate::analyzer::CodeAnalyzer::new()));
        let (nodes, edges) = engine
            .dependency_graph("/nonexistent", None, None, None)
            .await
            .unwrap();
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }
}
