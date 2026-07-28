/*!
 * Tantivy-backed full-text index for the Semantic Context Engine.
 *
 * Part 18's stack table names Tantivy for search. The index lives on disk under
 * the repo's `.cid/index` directory, so it survives a Core restart instead of
 * being rebuilt from scratch every launch.
 *
 * BM25 relevance comes from Tantivy itself; the engine layers embedding
 * similarity on top for hybrid retrieval.
 */

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tantivy::{
    collector::TopDocs,
    directory::MmapDirectory,
    query::QueryParser,
    schema::{Field, Schema, Value, FAST, STORED, STRING, TEXT},
    Index, IndexReader, IndexWriter, ReloadPolicy, TantivyDocument, Term,
};

/// Field handles for the code-chunk schema, resolved once at construction.
#[derive(Clone, Copy)]
struct Fields {
    file_path: Field,
    content: Field,
    symbol: Field,
    line_start: Field,
    line_end: Field,
}

pub struct SearchIndex {
    index: Index,
    reader: IndexReader,
    fields: Fields,
    dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub file_path: String,
    pub content: String,
    pub symbol_name: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
    pub score: f32,
}

fn build_schema() -> (Schema, Fields) {
    let mut builder = Schema::builder();
    // STRING (not TEXT) so a path is one exact term and can be deleted by term
    // when a file is re-indexed.
    let file_path = builder.add_text_field("file_path", STRING | STORED);
    let content = builder.add_text_field("content", TEXT | STORED);
    let symbol = builder.add_text_field("symbol", TEXT | STORED);
    let line_start = builder.add_u64_field("line_start", STORED | FAST);
    let line_end = builder.add_u64_field("line_end", STORED | FAST);
    let schema = builder.build();
    (
        schema,
        Fields {
            file_path,
            content,
            symbol,
            line_start,
            line_end,
        },
    )
}

impl SearchIndex {
    /// Open (or create) the on-disk index for a repository.
    pub fn open(repo_path: &str) -> Result<Self> {
        let dir = Path::new(repo_path).join(".cid").join("index");
        std::fs::create_dir_all(&dir)
            .with_context(|| format!("creating index directory {}", dir.display()))?;

        let (schema, fields) = build_schema();
        let mmap = MmapDirectory::open(&dir)
            .with_context(|| format!("opening index directory {}", dir.display()))?;
        let index = Index::open_or_create(mmap, schema)?;
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;

        Ok(Self {
            index,
            reader,
            fields,
            dir,
        })
    }

    /// An index held only in memory. Used by tests and by callers that do not
    /// want to write into the repository.
    pub fn in_memory() -> Result<Self> {
        let (schema, fields) = build_schema();
        let index = Index::create_in_ram(schema);
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        Ok(Self {
            index,
            reader,
            fields,
            dir: PathBuf::new(),
        })
    }

    pub fn location(&self) -> Option<&Path> {
        if self.dir.as_os_str().is_empty() {
            None
        } else {
            Some(&self.dir)
        }
    }

    fn writer(&self) -> Result<IndexWriter> {
        // 50MB heap: enough for repo-scale indexing without holding a large
        // allocation for the process lifetime.
        Ok(self.index.writer(50_000_000)?)
    }

    /// Replace every chunk belonging to `file_path`. Called on file change, so
    /// re-indexing a file cannot leave stale chunks behind.
    pub fn replace_file(&self, file_path: &str, chunks: &[IndexChunk]) -> Result<()> {
        let mut writer = self.writer()?;
        writer.delete_term(Term::from_field_text(self.fields.file_path, file_path));
        for chunk in chunks {
            writer.add_document(self.to_doc(file_path, chunk))?;
        }
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    /// Index many files in one commit — far cheaper than a commit per file
    /// during a full repository scan.
    pub fn replace_files(&self, files: &[(String, Vec<IndexChunk>)]) -> Result<()> {
        let mut writer = self.writer()?;
        for (path, chunks) in files {
            writer.delete_term(Term::from_field_text(self.fields.file_path, path));
            for chunk in chunks {
                writer.add_document(self.to_doc(path, chunk))?;
            }
        }
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn remove_file(&self, file_path: &str) -> Result<()> {
        let mut writer = self.writer()?;
        writer.delete_term(Term::from_field_text(self.fields.file_path, file_path));
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        let mut writer = self.writer()?;
        writer.delete_all_documents()?;
        writer.commit()?;
        self.reader.reload()?;
        Ok(())
    }

    fn to_doc(&self, file_path: &str, chunk: &IndexChunk) -> TantivyDocument {
        let mut doc = TantivyDocument::default();
        doc.add_text(self.fields.file_path, file_path);
        doc.add_text(self.fields.content, &chunk.content);
        if let Some(sym) = &chunk.symbol_name {
            doc.add_text(self.fields.symbol, sym);
        }
        doc.add_u64(self.fields.line_start, chunk.line_start as u64);
        doc.add_u64(self.fields.line_end, chunk.line_end as u64);
        doc
    }

    /// BM25 search over content and symbol names.
    ///
    /// A query that Tantivy cannot parse (a bare `:` or unbalanced quote from a
    /// user typing freely) is retried as a plain term query rather than
    /// surfacing a parse error.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        if query.trim().is_empty() {
            return Ok(vec![]);
        }
        let searcher = self.reader.searcher();
        let parser =
            QueryParser::for_index(&self.index, vec![self.fields.content, self.fields.symbol]);

        let parsed = parser
            .parse_query(query)
            .or_else(|_| parser.parse_query(&escape_query(query)))?;

        let top = searcher.search(&parsed, &TopDocs::with_limit(limit))?;
        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            hits.push(SearchHit {
                file_path: text_of(&doc, self.fields.file_path).unwrap_or_default(),
                content: text_of(&doc, self.fields.content).unwrap_or_default(),
                symbol_name: text_of(&doc, self.fields.symbol),
                line_start: u64_of(&doc, self.fields.line_start).unwrap_or(0) as usize,
                line_end: u64_of(&doc, self.fields.line_end).unwrap_or(0) as usize,
                score,
            });
        }
        Ok(hits)
    }

    pub fn document_count(&self) -> u64 {
        self.reader.searcher().num_docs()
    }
}

#[derive(Debug, Clone)]
pub struct IndexChunk {
    pub content: String,
    pub symbol_name: Option<String>,
    pub line_start: usize,
    pub line_end: usize,
}

/// Strip the characters Tantivy's query grammar treats as operators, so a
/// free-text query still returns results instead of a parse error.
fn escape_query(query: &str) -> String {
    query
        .chars()
        .map(|c| {
            if "+-&|!(){}[]^\"~*?:\\/".contains(c) {
                ' '
            } else {
                c
            }
        })
        .collect::<String>()
        .trim()
        .to_string()
}

fn text_of(doc: &TantivyDocument, field: Field) -> Option<String> {
    doc.get_first(field)
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn u64_of(doc: &TantivyDocument, field: Field) -> Option<u64> {
    doc.get_first(field).and_then(|v| v.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunk(content: &str, symbol: Option<&str>) -> IndexChunk {
        IndexChunk {
            content: content.to_string(),
            symbol_name: symbol.map(|s| s.to_string()),
            line_start: 1,
            line_end: 10,
        }
    }

    #[test]
    fn indexes_and_finds_a_chunk() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file(
            "src/auth.rs",
            &[chunk(
                "fn validate_token(token: &str) -> bool",
                Some("validate_token"),
            )],
        )
        .unwrap();

        let hits = idx.search("validate_token", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].file_path, "src/auth.rs");
        assert!(hits[0].score > 0.0);
    }

    #[test]
    fn re_indexing_a_file_replaces_its_chunks() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file("src/a.rs", &[chunk("original_symbol_here", None)])
            .unwrap();
        assert_eq!(idx.search("original_symbol_here", 10).unwrap().len(), 1);

        idx.replace_file("src/a.rs", &[chunk("replacement_symbol_here", None)])
            .unwrap();
        assert!(
            idx.search("original_symbol_here", 10).unwrap().is_empty(),
            "stale chunks must not survive a re-index"
        );
        assert_eq!(idx.search("replacement_symbol_here", 10).unwrap().len(), 1);
    }

    #[test]
    fn removing_a_file_removes_its_chunks() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file("src/gone.rs", &[chunk("doomed_function", None)])
            .unwrap();
        idx.remove_file("src/gone.rs").unwrap();
        assert!(idx.search("doomed_function", 10).unwrap().is_empty());
    }

    #[test]
    fn ranks_by_relevance() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_files(&[
            (
                "src/match.rs".to_string(),
                vec![chunk(
                    "authenticate authenticate authenticate",
                    Some("authenticate"),
                )],
            ),
            (
                "src/other.rs".to_string(),
                vec![chunk("something about authenticate once", None)],
            ),
        ])
        .unwrap();

        let hits = idx.search("authenticate", 10).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(
            hits[0].file_path, "src/match.rs",
            "denser match should rank first"
        );
        assert!(hits[0].score >= hits[1].score);
    }

    #[test]
    fn matches_on_symbol_names_too() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file(
            "src/x.rs",
            &[chunk("body text unrelated", Some("SpecialSymbolName"))],
        )
        .unwrap();
        assert_eq!(idx.search("SpecialSymbolName", 10).unwrap().len(), 1);
    }

    #[test]
    fn a_query_with_operator_characters_still_returns_results() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file("src/x.rs", &[chunk("handle request path", None)])
            .unwrap();
        let hits = idx.search("handle: request/*", 10).unwrap();
        assert!(
            !hits.is_empty(),
            "a messy free-text query must not error out"
        );
    }

    #[test]
    fn an_empty_query_returns_nothing_rather_than_everything() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file("src/x.rs", &[chunk("content", None)])
            .unwrap();
        assert!(idx.search("   ", 10).unwrap().is_empty());
    }

    #[test]
    fn limit_is_respected() {
        let idx = SearchIndex::in_memory().unwrap();
        let files: Vec<(String, Vec<IndexChunk>)> = (0..10)
            .map(|i| (format!("src/f{i}.rs"), vec![chunk("common term", None)]))
            .collect();
        idx.replace_files(&files).unwrap();
        assert_eq!(idx.search("common", 3).unwrap().len(), 3);
    }

    #[test]
    fn clear_empties_the_index() {
        let idx = SearchIndex::in_memory().unwrap();
        idx.replace_file("src/x.rs", &[chunk("content here", None)])
            .unwrap();
        assert_eq!(idx.document_count(), 1);
        idx.clear().unwrap();
        assert_eq!(idx.document_count(), 0);
    }

    #[test]
    fn an_on_disk_index_survives_being_reopened() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().to_string_lossy().to_string();

        {
            let idx = SearchIndex::open(&repo).unwrap();
            idx.replace_file("src/persist.rs", &[chunk("persisted_symbol", None)])
                .unwrap();
            assert_eq!(idx.search("persisted_symbol", 10).unwrap().len(), 1);
        }

        // A fresh handle, as though Core had restarted.
        let reopened = SearchIndex::open(&repo).unwrap();
        assert_eq!(
            reopened.search("persisted_symbol", 10).unwrap().len(),
            1,
            "the index must survive a Core restart"
        );
        assert!(reopened.location().is_some());
    }
}
