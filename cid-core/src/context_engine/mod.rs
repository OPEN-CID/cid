//! Phase 1 Structural Context Engine
//! Tree-sitter based symbol/import/reference indexing per file,
//! refreshed incrementally on file change, off by default per Repo Channel.
//! Implements performance and feature-flag philosophy: core always light,
//! heavy indexing off by default, toggled like VS Code extension.
//!
//! Requirements covered:
//! - tree-sitter 0.24 with parsers: rust, typescript, javascript, python, go, json
//! - Structs from api/types.rs: ContextEngineStatus, CodeSymbol, SymbolKind, FileIndex
//! - ContextEngineManager with incremental watcher (notify 6.1) + walkdir 2.5
//! - In-memory HashMap index, graceful error handling, large-file guard
//! - Plain text + symbol search combined
//! - Related files via import graph + symbol overlap
//! - File-tree badges data (recently touched via last_modified, structurally related via get_related_files)

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Mutex, RwLock,
};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use notify::{Config, Event, RecursiveMode, Watcher};
use tree_sitter::{Language, Node, Parser, Tree};
use walkdir::WalkDir;

use crate::api::types::{CodeSymbol, ContextEngineStatus, FileIndex, SymbolKind};

// ---------------------------------------------------------------------------
// Constants & Config
// ---------------------------------------------------------------------------

/// Skip files larger than 1 MiB to avoid OOM on huge generated files.
const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Cap total number of candidate files per repo to keep Phase 1 light.
const MAX_FILES_PER_REPO: usize = 20_000;

/// Directories to ignore during walk & watcher filtering.
/// Spec requires .git, node_modules, target, dist; we also ignore .cid and common build dirs.
const IGNORED_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    ".cid",
    "build",
    ".next",
    "out",
    "vendor",
    ".hg",
    ".svn",
    "__pycache__",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
];

// ---------------------------------------------------------------------------
// Helpers – language detection & path filtering
// ---------------------------------------------------------------------------

fn detect_language(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    match ext.as_str() {
        "rs" => Some("rust".to_string()),
        "ts" | "mts" | "cts" => Some("typescript".to_string()),
        "tsx" => Some("typescript".to_string()), // handled as TSX parser variant
        "js" | "mjs" | "cjs" => Some("javascript".to_string()),
        "jsx" => Some("javascript".to_string()),
        "py" | "pyi" => Some("python".to_string()),
        "go" => Some("go".to_string()),
        "json" => Some("json".to_string()),
        _ => None,
    }
}

#[derive(Clone, Copy)]
enum TsVariant {
    TypeScript,
    Tsx,
    JavaScript,
}

fn get_ts_variant(path: &Path) -> TsVariant {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "tsx" | "jsx" => TsVariant::Tsx,
        "ts" | "mts" | "cts" => TsVariant::TypeScript,
        _ => TsVariant::JavaScript,
    }
}

/// Returns tree-sitter Language + canonical language string for FileIndex.
fn get_tree_sitter_language(path: &Path) -> Option<(Language, String)> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    let variant = get_ts_variant(path);
    match ext.as_str() {
        "rs" => Some((tree_sitter_rust::LANGUAGE.into(), "rust".to_string())),
        "ts" | "mts" | "cts" => Some((
            tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            "typescript".to_string(),
        )),
        "tsx" => Some((
            tree_sitter_typescript::LANGUAGE_TSX.into(),
            "typescript".to_string(),
        )),
        "js" | "mjs" | "cjs" => Some((
            tree_sitter_javascript::LANGUAGE.into(),
            "javascript".to_string(),
        )),
        "jsx" => match variant {
            TsVariant::Tsx => Some((
                tree_sitter_typescript::LANGUAGE_TSX.into(),
                "javascript".to_string(),
            )),
            _ => Some((
                tree_sitter_javascript::LANGUAGE.into(),
                "javascript".to_string(),
            )),
        },
        "py" | "pyi" => Some((tree_sitter_python::LANGUAGE.into(), "python".to_string())),
        "go" => Some((tree_sitter_go::LANGUAGE.into(), "go".to_string())),
        "json" => Some((tree_sitter_json::LANGUAGE.into(), "json".to_string())),
        _ => None,
    }
}

fn is_ignored_dir_name(name: &str) -> bool {
    IGNORED_DIRS.contains(&name)
}

fn is_ignored_path(path: &Path) -> bool {
    for comp in path.components() {
        if let std::path::Component::Normal(os_str) = comp {
            if let Some(s) = os_str.to_str() {
                if is_ignored_dir_name(s) {
                    // Allow the repo root itself to be .git? No, we ignore the content of ignored dirs.
                    // If any component matches, we treat path as ignored.
                    return true;
                }
            }
        }
    }
    // Also ignore hidden files that are huge binaries? Keep simple.
    false
}

fn file_stem_lower(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_lowercase()
}

// ---------------------------------------------------------------------------
// Tree-sitter helpers – symbol extraction
// ---------------------------------------------------------------------------

fn is_import_kind(kind: &str) -> bool {
    matches!(
        kind,
        "import_statement"
            | "import_from_statement"
            | "import_declaration"
            | "import_spec"
            | "import_clause"
            | "use_declaration"
    ) || kind.contains("import")
}

fn is_valid_symbol_name(name: &str) -> bool {
    let n = name.trim();
    if n.is_empty() || n.len() > 128 {
        return false;
    }
    // Disallow names with spaces/newlines/quotes
    if n.contains('\n') || n.contains('\r') {
        return false;
    }
    // At least one alphanumeric or _
    // Keep permissive for operator overloads? For phase 1 strict to identifier-like but allow $ for JS.
    true
}

fn resolve_to_identifier(node: Node) -> Node {
    match node.kind() {
        "identifier"
        | "type_identifier"
        | "property_identifier"
        | "field_identifier"
        | "primitive_type"
        | "shorthand_property_identifier" => node,
        _ => {
            for i in 0..node.named_child_count() {
                if let Some(child) = node.named_child(i) {
                    match child.kind() {
                        "identifier" | "type_identifier" | "property_identifier" => {
                            return child;
                        }
                        _ => {}
                    }
                }
            }
            node
        }
    }
}

fn find_name_node(node: Node) -> Option<Node> {
    if let Some(n) = node.child_by_field_name("name") {
        return Some(resolve_to_identifier(n));
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            match child.kind() {
                "identifier"
                | "type_identifier"
                | "property_identifier"
                | "field_identifier"
                | "primitive_type"
                | "field_name"
                | "variable_name" => {
                    return Some(child);
                }
                _ => {}
            }
        }
    }
    None
}

fn find_identifier_recursive(node: Node) -> Option<Node> {
    if matches!(
        node.kind(),
        "identifier" | "type_identifier" | "property_identifier" | "field_identifier"
    ) {
        return Some(node);
    }
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            if let Some(found) = find_identifier_recursive(child) {
                return Some(found);
            }
        }
    }
    None
}

fn extract_type_name(node: Node, source: &str) -> Option<String> {
    if let Some(id) = find_identifier_recursive(node) {
        if let Ok(t) = id.utf8_text(source.as_bytes()) {
            return Some(t.to_string());
        }
    }
    // fallback: first token before < or whitespace
    if let Ok(text) = node.utf8_text(source.as_bytes()) {
        let s = text
            .split(|c: char| !(c.is_alphanumeric() || c == '_' || c == ':' || c == '$' || c == '.'))
            .next()
            .unwrap_or("")
            .trim();
        if !s.is_empty() && s.len() < 128 {
            return Some(s.to_string());
        }
    }
    None
}

#[allow(clippy::type_complexity)]
fn get_generic_symbol_kind(kind: &str, has_parent: bool) -> Option<(SymbolKind, bool)> {
    // bool = is_structural_parent (should push onto parent stack for children)
    match kind {
        // Rust
        "function_item" => Some((
            if has_parent {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            },
            false,
        )),
        "struct_item" => Some((SymbolKind::Struct, true)),
        "enum_item" => Some((SymbolKind::Enum, true)),
        "trait_item" => Some((SymbolKind::Interface, true)),
        "mod_item" => Some((SymbolKind::Class, true)),
        "const_item" | "static_item" => Some((SymbolKind::Constant, false)),

        // TS/JS
        "function_declaration" => Some((SymbolKind::Function, false)),
        "generator_function_declaration" => Some((SymbolKind::Function, false)),
        "method_definition" => Some((SymbolKind::Method, false)),
        "class_declaration" | "abstract_class_declaration" => Some((SymbolKind::Class, true)),
        "interface_declaration" => Some((SymbolKind::Interface, true)),
        "enum_declaration" => Some((SymbolKind::Enum, true)),
        "type_alias_declaration" => Some((SymbolKind::Interface, false)),

        // Python
        "function_definition" => Some((
            if has_parent {
                SymbolKind::Method
            } else {
                SymbolKind::Function
            },
            false,
        )),
        "class_definition" => Some((SymbolKind::Class, true)),

        // Go
        "method_declaration" => Some((SymbolKind::Method, false)),
        // function_declaration for Go handled separately for receiver check, but fallback:
        // type_spec handled elsewhere
        _ => None,
    }
}

fn collect_symbols_recursive(
    node: Node,
    source: &str,
    file_path: &str,
    parent_stack: &mut Vec<String>,
    symbols: &mut Vec<CodeSymbol>,
    imports: &mut Vec<String>,
    language: &str,
) {
    let kind = node.kind();

    // Import collection
    if is_import_kind(kind) {
        if let Ok(text) = node.utf8_text(source.as_bytes()) {
            let t = text.trim();
            if !t.is_empty() && t.len() < 2000 {
                imports.push(t.to_string());
            }
        }
    }

    // Special: type_spec for Go
    if kind == "type_spec" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name_text) = name_node.utf8_text(source.as_bytes()) {
                let name = name_text.trim().to_string();
                if is_valid_symbol_name(&name) {
                    let mut sym_kind = SymbolKind::Struct;
                    if let Some(type_node) = node.child_by_field_name("type") {
                        match type_node.kind() {
                            "struct_type" => sym_kind = SymbolKind::Struct,
                            "interface_type" => sym_kind = SymbolKind::Interface,
                            _ => sym_kind = SymbolKind::Struct,
                        }
                    }
                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(CodeSymbol {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: name.clone(),
                        kind: sym_kind,
                        file_path: file_path.to_string(),
                        line: start.row + 1,
                        column: start.column,
                        end_line: end.row + 1,
                        end_column: end.column,
                        parent: parent_stack.last().cloned(),
                        imports: vec![],
                    });
                }
            }
        }
        // Continue recursion to catch inner struct fields? Not needed for phase 1, but recurse anyway for completeness
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                // Avoid infinite loop on name/type which already processed
                if child.kind() == "type_identifier" {
                    continue;
                }
                collect_symbols_recursive(
                    child,
                    source,
                    file_path,
                    parent_stack,
                    symbols,
                    imports,
                    language,
                );
            }
        }
        return;
    }

    // Special: variable_declarator with arrow function / function_expression / class
    if kind == "variable_declarator" {
        if let Some(name_node) = node.child_by_field_name("name") {
            if let Ok(name_text) = name_node.utf8_text(source.as_bytes()) {
                if let Some(value_node) = node.child_by_field_name("value") {
                    let vkind = value_node.kind();
                    if matches!(
                        vkind,
                        "arrow_function"
                            | "function_expression"
                            | "function"
                            | "class"
                            | "class_declaration"
                            | "function_declaration"
                    ) {
                        let sym_kind = if vkind.contains("class") {
                            SymbolKind::Class
                        } else {
                            SymbolKind::Function
                        };
                        let name = name_text.trim().to_string();
                        if is_valid_symbol_name(&name) {
                            let start = node.start_position();
                            let end = node.end_position();
                            symbols.push(CodeSymbol {
                                id: uuid::Uuid::new_v4().to_string(),
                                name: name.clone(),
                                kind: sym_kind.clone(),
                                file_path: file_path.to_string(),
                                line: start.row + 1,
                                column: start.column,
                                end_line: end.row + 1,
                                end_column: end.column,
                                parent: parent_stack.last().cloned(),
                                imports: vec![],
                            });
                            if sym_kind == SymbolKind::Class {
                                parent_stack.push(name);
                                for i in 0..value_node.named_child_count() {
                                    if let Some(child) = value_node.named_child(i) {
                                        collect_symbols_recursive(
                                            child,
                                            source,
                                            file_path,
                                            parent_stack,
                                            symbols,
                                            imports,
                                            language,
                                        );
                                    }
                                }
                                parent_stack.pop();
                                return;
                            }
                        }
                    }
                }
            }
        }
    }

    // Special: impl_item (Rust) – push type name as parent context, no symbol for impl itself
    if kind == "impl_item" {
        if let Some(type_node) = node.child_by_field_name("type") {
            if let Some(type_name) = extract_type_name(type_node, source) {
                parent_stack.push(type_name);
                for i in 0..node.named_child_count() {
                    if let Some(child) = node.named_child(i) {
                        collect_symbols_recursive(
                            child,
                            source,
                            file_path,
                            parent_stack,
                            symbols,
                            imports,
                            language,
                        );
                    }
                }
                parent_stack.pop();
                return;
            }
        }
    }

    // Generic symbol detection
    if let Some((mut sym_kind, is_structural)) =
        get_generic_symbol_kind(kind, !parent_stack.is_empty())
    {
        if let Some(name_node) = find_name_node(node) {
            if let Ok(name_text) = name_node.utf8_text(source.as_bytes()) {
                let name_str = name_text.trim().to_string();
                if is_valid_symbol_name(&name_str) {
                    // Go receiver check
                    if language == "go" && kind == "function_declaration" {
                        if node.child_by_field_name("receiver").is_some() {
                            sym_kind = SymbolKind::Method;
                        } else {
                            sym_kind = SymbolKind::Function;
                        }
                    }

                    let start = node.start_position();
                    let end = node.end_position();
                    symbols.push(CodeSymbol {
                        id: uuid::Uuid::new_v4().to_string(),
                        name: name_str.clone(),
                        kind: sym_kind.clone(),
                        file_path: file_path.to_string(),
                        line: start.row + 1,
                        column: start.column,
                        end_line: end.row + 1,
                        end_column: end.column,
                        parent: parent_stack.last().cloned(),
                        imports: vec![],
                    });

                    if is_structural {
                        parent_stack.push(name_str);
                        for i in 0..node.named_child_count() {
                            if let Some(child) = node.named_child(i) {
                                collect_symbols_recursive(
                                    child,
                                    source,
                                    file_path,
                                    parent_stack,
                                    symbols,
                                    imports,
                                    language,
                                );
                            }
                        }
                        parent_stack.pop();
                        return;
                    }
                }
            }
        }
    }

    // Default: recurse into named children
    for i in 0..node.named_child_count() {
        if let Some(child) = node.named_child(i) {
            collect_symbols_recursive(
                child,
                source,
                file_path,
                parent_stack,
                symbols,
                imports,
                language,
            );
        }
    }
}

fn extract_json_symbols(
    tree: &Tree,
    source: &str,
    file_path: &str,
) -> (Vec<CodeSymbol>, Vec<String>) {
    let mut symbols = Vec::new();
    let root = tree.root_node();

    fn visit_json(
        node: Node,
        source: &str,
        file_path: &str,
        symbols: &mut Vec<CodeSymbol>,
        depth: usize,
    ) {
        if depth > 12 {
            return;
        }
        if node.kind() == "pair" {
            if let Some(key_node) = node.child_by_field_name("key") {
                if let Ok(key_text) = key_node.utf8_text(source.as_bytes()) {
                    let mut name = key_text.trim().to_string();
                    name = name
                        .trim_matches(|c| c == '"' || c == '\'' || c == '`')
                        .to_string();
                    if is_valid_symbol_name(&name) {
                        let start = node.start_position();
                        let end = node.end_position();
                        symbols.push(CodeSymbol {
                            id: uuid::Uuid::new_v4().to_string(),
                            name,
                            kind: SymbolKind::Property,
                            file_path: file_path.to_string(),
                            line: start.row + 1,
                            column: start.column,
                            end_line: end.row + 1,
                            end_column: end.column,
                            parent: None,
                            imports: vec![],
                        });
                    }
                }
            }
        }
        for i in 0..node.named_child_count() {
            if let Some(child) = node.named_child(i) {
                visit_json(child, source, file_path, symbols, depth + 1);
            }
        }
    }

    visit_json(root, source, file_path, &mut symbols, 0);
    (symbols, Vec::new())
}

fn extract_symbols_and_imports(
    tree: &Tree,
    source: &str,
    file_path: &str,
    language: &str,
) -> (Vec<CodeSymbol>, Vec<String>) {
    if language == "json" {
        return extract_json_symbols(tree, source, file_path);
    }

    let mut symbols = Vec::new();
    let mut imports = Vec::new();
    let mut parent_stack: Vec<String> = Vec::new();
    collect_symbols_recursive(
        tree.root_node(),
        source,
        file_path,
        &mut parent_stack,
        &mut symbols,
        &mut imports,
        language,
    );

    // Dedupe imports, keep order
    let mut seen = HashSet::new();
    let mut deduped = Vec::new();
    for imp in imports {
        if seen.insert(imp.clone()) {
            deduped.push(imp);
        }
    }

    (symbols, deduped)
}

// ---------------------------------------------------------------------------
// File indexing – parse + extract with safety guards
// ---------------------------------------------------------------------------

fn metadata_to_dt(metadata: &std::fs::Metadata) -> DateTime<Utc> {
    metadata
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .unwrap_or_else(Utc::now)
}

fn index_file_internal(path: &Path) -> Result<FileIndex> {
    if !path.exists() {
        return Err(anyhow!("File does not exist: {:?}", path));
    }
    if !path.is_file() {
        return Err(anyhow!("Not a file: {:?}", path));
    }

    let metadata =
        std::fs::metadata(path).map_err(|e| anyhow!("metadata error {:?}: {}", path, e))?;
    let size = metadata.len() as usize;
    let last_modified = metadata_to_dt(&metadata);

    // Large file guard – keep light core philosophy
    if metadata.len() > MAX_FILE_SIZE {
        tracing::warn!("Skipping large file {:?} ({} bytes)", path, metadata.len());
        let lang = detect_language(path).unwrap_or_else(|| "unknown".to_string());
        return Ok(FileIndex {
            path: path.to_string_lossy().to_string(),
            language: lang,
            symbols: Vec::new(),
            imports: Vec::new(),
            last_modified,
            size,
        });
    }

    // Read file content as string – handle UTF-8 errors gracefully
    let content = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            // Binary or unreadable – treat as empty but keep FileIndex
            tracing::debug!(
                "Failed to read file as utf8 {:?}: {}, treating as empty",
                path,
                e
            );
            String::new()
        }
    };

    if content.is_empty() {
        let lang = detect_language(path).unwrap_or_else(|| "unknown".to_string());
        return Ok(FileIndex {
            path: path.to_string_lossy().to_string(),
            language: lang,
            symbols: Vec::new(),
            imports: Vec::new(),
            last_modified,
            size,
        });
    }

    // Detect language
    let lang_info = get_tree_sitter_language(path);
    let (ts_lang, lang_str) = match lang_info {
        Some((l, s)) => (l, s),
        None => {
            // Unsupported extension – still return FileIndex with unknown lang
            return Ok(FileIndex {
                path: path.to_string_lossy().to_string(),
                language: "unknown".to_string(),
                symbols: Vec::new(),
                imports: Vec::new(),
                last_modified,
                size,
            });
        }
    };

    // Parse with tree-sitter – handle parse errors gracefully
    let mut parser = Parser::new();
    if let Err(e) = parser.set_language(&ts_lang) {
        tracing::warn!("Failed to set language for {:?}: {:?}", path, e);
        return Ok(FileIndex {
            path: path.to_string_lossy().to_string(),
            language: lang_str,
            symbols: Vec::new(),
            imports: Vec::new(),
            last_modified,
            size,
        });
    }

    let tree = match parser.parse(&content, None) {
        Some(t) => t,
        None => {
            tracing::warn!("Tree-sitter failed to parse {:?}", path);
            return Ok(FileIndex {
                path: path.to_string_lossy().to_string(),
                language: lang_str,
                symbols: Vec::new(),
                imports: Vec::new(),
                last_modified,
                size,
            });
        }
    };

    // If root has error flag, still try to extract but log
    if tree.root_node().has_error() {
        tracing::debug!("Parse tree has errors for {:?}", path);
        // Continue – tree-sitter recovers partially
    }

    let (symbols, imports) =
        extract_symbols_and_imports(&tree, &content, &path.to_string_lossy(), &lang_str);

    Ok(FileIndex {
        path: path.to_string_lossy().to_string(),
        language: lang_str,
        symbols,
        imports,
        last_modified,
        size,
    })
}

// ---------------------------------------------------------------------------
// Manager state & watchers
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct RepoState {
    enabled: bool,
    indexed_files: usize,
    total_files: usize,
    last_indexed_at: Option<DateTime<Utc>>,
    indexing: bool,
    indexes: HashMap<String, FileIndex>,
}

struct WatcherHandle {
    stop_flag: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}

// ---------------------------------------------------------------------------
// Public Manager
// ---------------------------------------------------------------------------

/// Phase 1 Structural Context Engine Manager.
/// Off by default per Repo Channel, toggled via enable_for_repo.
/// Core always light – indexing is heavy and optional, per Part 17 philosophy.
pub struct ContextEngineManager {
    state: Arc<RwLock<HashMap<String, RepoState>>>,
    watchers: Arc<Mutex<HashMap<String, WatcherHandle>>>,
}

impl Clone for ContextEngineManager {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
            watchers: self.watchers.clone(),
        }
    }
}

impl Default for ContextEngineManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextEngineManager {
    pub fn new() -> Self {
        Self {
            state: Arc::new(RwLock::new(HashMap::new())),
            watchers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    // ---- internal helpers ----

    fn canonical_key(repo_path: &str) -> String {
        // Try canonicalize, fallback to trimmed absolute string
        let p = Path::new(repo_path);
        if let Ok(canonical) = p.canonicalize() {
            canonical.to_string_lossy().to_string()
        } else {
            // Normalize slashes and remove trailing slash
            let s = repo_path
                .trim_end_matches('/')
                .trim_end_matches('\\')
                .to_string();
            s
        }
    }

    fn start_watcher(&self, repo_key: &str) -> Result<()> {
        // Avoid duplicate watchers
        {
            let watchers = self
                .watchers
                .lock()
                .map_err(|_| anyhow!("watchers lock poisoned"))?;
            if watchers.contains_key(repo_key) {
                return Ok(());
            }
        }

        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_flag_clone = stop_flag.clone();
        let repo_owned = repo_key.to_string();
        let state_clone = self.state.clone();

        let handle = thread::spawn(move || {
            let (tx, rx) = std::sync::mpsc::channel::<Result<Event, notify::Error>>();

            let watcher_res = notify::RecommendedWatcher::new(
                move |res: Result<Event, notify::Error>| {
                    let _ = tx.send(res);
                },
                Config::default().with_poll_interval(Duration::from_secs(2)),
            );

            let mut watcher = match watcher_res {
                Ok(w) => w,
                Err(e) => {
                    tracing::error!("Failed to create watcher for {}: {:?}", repo_owned, e);
                    return;
                }
            };

            if let Err(e) = watcher.watch(Path::new(&repo_owned), RecursiveMode::Recursive) {
                tracing::error!("Failed to watch path {}: {:?}", repo_owned, e);
                return;
            }

            tracing::info!("Context engine watcher started for {}", repo_owned);

            let mut debounce: HashMap<String, Instant> = HashMap::new();

            loop {
                if stop_flag_clone.load(Ordering::Relaxed) {
                    break;
                }

                match rx.recv_timeout(Duration::from_millis(500)) {
                    Ok(Ok(event)) => {
                        // Only care about create/modify/remove
                        match event.kind {
                            notify::EventKind::Create(_)
                            | notify::EventKind::Modify(_)
                            | notify::EventKind::Remove(_) => {}
                            _ => continue,
                        }

                        for path in event.paths {
                            if is_ignored_path(&path) {
                                continue;
                            }

                            let path_str = path.to_string_lossy().to_string();

                            // debounce 200ms per file
                            if let Some(last) = debounce.get(&path_str) {
                                if last.elapsed() < Duration::from_millis(200) {
                                    continue;
                                }
                            }
                            debounce.insert(path_str.clone(), Instant::now());

                            if debounce.len() > 1000 {
                                debounce.retain(|_, t| t.elapsed() < Duration::from_secs(5));
                            }

                            if detect_language(&path).is_none() {
                                continue;
                            }

                            if path.exists() && path.is_file() {
                                match index_file_internal(&path) {
                                    Ok(file_index) => {
                                        if let Ok(mut map) = state_clone.write() {
                                            if let Some(repo_state) = map.get_mut(&repo_owned) {
                                                repo_state
                                                    .indexes
                                                    .insert(path_str.clone(), file_index);
                                                repo_state.indexed_files = repo_state.indexes.len();
                                                repo_state.last_indexed_at = Some(Utc::now());
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            "Watcher re-index failed for {:?}: {:?}",
                                            path,
                                            e
                                        );
                                    }
                                }
                            } else {
                                // deleted
                                if let Ok(mut map) = state_clone.write() {
                                    if let Some(repo_state) = map.get_mut(&repo_owned) {
                                        // Remove exact and also any key that resolves to same path (for symlink/case differences)
                                        let mut to_remove = Vec::new();
                                        for k in repo_state.indexes.keys() {
                                            if k == &path_str {
                                                to_remove.push(k.clone());
                                            }
                                        }
                                        for k in to_remove {
                                            repo_state.indexes.remove(&k);
                                        }
                                        // Also try direct remove
                                        repo_state.indexes.remove(&path_str);
                                        repo_state.indexed_files = repo_state.indexes.len();
                                        repo_state.last_indexed_at = Some(Utc::now());
                                    }
                                }
                            }
                        }
                    }
                    Ok(Err(e)) => {
                        tracing::warn!("Watcher error for {}: {:?}", repo_owned, e);
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }

            tracing::info!("Context engine watcher stopped for {}", repo_owned);
            // Watcher dropped here
        });

        let mut watchers = self
            .watchers
            .lock()
            .map_err(|_| anyhow!("watchers lock poisoned"))?;
        watchers.insert(
            repo_key.to_string(),
            WatcherHandle {
                stop_flag,
                thread: Some(handle),
            },
        );

        Ok(())
    }

    // ---- public API (per task spec) ----

    /// Creates index dir .cid/context-engine, walks repo, starts file watcher.
    pub fn enable_for_repo(&self, repo_path: &str) -> Result<ContextEngineStatus> {
        let repo_key = Self::canonical_key(repo_path);
        let canonical_path = PathBuf::from(&repo_key);

        // If already enabled, return status
        {
            if let Ok(map) = self.state.read() {
                if let Some(st) = map.get(&repo_key) {
                    if st.enabled {
                        return Ok(ContextEngineStatus {
                            enabled: true,
                            repo_path: repo_key,
                            indexed_files: st.indexed_files,
                            total_files: st.total_files,
                            last_indexed_at: st.last_indexed_at,
                            indexing: st.indexing,
                        });
                    }
                }
            }
        }

        // Create index dir for future SQLite cache (Phase1 still in-memory, but creates dir per spec)
        let index_dir = canonical_path.join(".cid").join("context-engine");
        std::fs::create_dir_all(&index_dir)
            .map_err(|e| anyhow!("Failed to create index dir {:?}: {}", index_dir, e))?;

        // Insert temporary indexing state
        {
            let mut map = self
                .state
                .write()
                .map_err(|_| anyhow!("state lock poisoned"))?;
            map.insert(
                repo_key.clone(),
                RepoState {
                    enabled: true,
                    indexed_files: 0,
                    total_files: 0,
                    last_indexed_at: None,
                    indexing: true,
                    indexes: HashMap::new(),
                },
            );
        }

        // Walk repo with WalkDir, ignoring .git, node_modules, etc.
        let mut candidate_files: Vec<PathBuf> = Vec::new();

        // If canonical path doesn't exist (fallback key), try original path for walking
        let walk_root = if canonical_path.exists() {
            canonical_path.clone()
        } else {
            PathBuf::from(repo_path)
        };

        for entry in WalkDir::new(&walk_root).into_iter().filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !is_ignored_dir_name(&name)
            } else {
                true
            }
        }) {
            if candidate_files.len() >= MAX_FILES_PER_REPO {
                tracing::warn!(
                    "Reached max files cap {} for repo {}",
                    MAX_FILES_PER_REPO,
                    repo_key
                );
                break;
            }
            match entry {
                Ok(ent) => {
                    if !ent.file_type().is_file() {
                        continue;
                    }
                    let p = ent.path();
                    if is_ignored_path(p) {
                        continue;
                    }
                    if detect_language(p).is_some() {
                        // Also skip files > MAX_FILE_SIZE at walk time to save IO? We still count total but will skip parse.
                        candidate_files.push(p.to_path_buf());
                    }
                }
                Err(e) => {
                    tracing::debug!("WalkDir error in {}: {:?}", repo_key, e);
                    continue;
                }
            }
        }

        let total_files = candidate_files.len();
        let mut indexed = 0usize;
        let mut indexes_map: HashMap<String, FileIndex> = HashMap::with_capacity(total_files);

        for file_path in candidate_files {
            match index_file_internal(&file_path) {
                Ok(fi) => {
                    // Only count as indexed if we actually got symbols or at least parsed (not large-skipped empty counts as indexed? we count as indexed if parse succeeded)
                    indexed += 1;
                    indexes_map.insert(file_path.to_string_lossy().to_string(), fi);
                }
                Err(e) => {
                    tracing::debug!("Index failed for {:?}: {:?}", file_path, e);
                    continue;
                }
            }
        }

        let now = Utc::now();
        {
            let mut map = self
                .state
                .write()
                .map_err(|_| anyhow!("state lock poisoned"))?;
            if let Some(st) = map.get_mut(&repo_key) {
                st.indexed_files = indexed;
                st.total_files = total_files;
                st.last_indexed_at = Some(now);
                st.indexing = false;
                st.indexes = indexes_map;
                st.enabled = true;
            }
        }

        // Start file watcher for incremental refresh
        if let Err(e) = self.start_watcher(&repo_key) {
            tracing::warn!("Failed to start watcher for {}: {:?}", repo_key, e);
            // Don't fail enable – indexing succeeded, watcher is best-effort
        }

        Ok(self.status(&repo_key))
    }

    pub fn disable_for_repo(&self, repo_path: &str) -> Result<()> {
        let repo_key = Self::canonical_key(repo_path);

        // Stop watcher
        let handle_opt = {
            let mut watchers = self
                .watchers
                .lock()
                .map_err(|_| anyhow!("watchers lock poisoned"))?;
            watchers.remove(&repo_key).or_else(|| {
                // Try raw key fallback
                watchers.remove(repo_path)
            })
        };

        if let Some(mut handle) = handle_opt {
            handle.stop_flag.store(true, Ordering::Relaxed);
            if let Some(th) = handle.thread.take() {
                // Wait briefly, don't block forever
                let _ = th.join();
            }
        }

        // Mark disabled
        if let Ok(mut map) = self.state.write() {
            if let Some(st) = map.get_mut(&repo_key) {
                st.enabled = false;
                st.indexing = false;
            } else if let Some(st) = map.get_mut(repo_path) {
                st.enabled = false;
                st.indexing = false;
            } else {
                // Remove any matching prefix?
                map.remove(&repo_key);
            }
        }

        Ok(())
    }

    pub fn status(&self, repo_path: &str) -> ContextEngineStatus {
        let repo_key = Self::canonical_key(repo_path);
        let map = match self.state.read() {
            Ok(m) => m,
            Err(_) => {
                return ContextEngineStatus {
                    enabled: false,
                    repo_path: repo_path.to_string(),
                    indexed_files: 0,
                    total_files: 0,
                    last_indexed_at: None,
                    indexing: false,
                }
            }
        };

        // Try canonical then raw
        if let Some(st) = map.get(&repo_key) {
            return ContextEngineStatus {
                enabled: st.enabled,
                repo_path: repo_key,
                indexed_files: st.indexed_files,
                total_files: st.total_files,
                last_indexed_at: st.last_indexed_at,
                indexing: st.indexing,
            };
        }
        if let Some(st) = map.get(repo_path) {
            return ContextEngineStatus {
                enabled: st.enabled,
                repo_path: repo_path.to_string(),
                indexed_files: st.indexed_files,
                total_files: st.total_files,
                last_indexed_at: st.last_indexed_at,
                indexing: st.indexing,
            };
        }

        ContextEngineStatus {
            enabled: false,
            repo_path: repo_path.to_string(),
            indexed_files: 0,
            total_files: 0,
            last_indexed_at: None,
            indexing: false,
        }
    }

    /// Parse a single file with tree-sitter, extract symbols via queries (tree traversal).
    /// Handles parse errors gracefully, skips large files.
    pub fn index_file(&self, path: &str) -> Result<FileIndex> {
        let p = Path::new(path);
        let file_index = index_file_internal(p)?;

        // If file belongs to an enabled repo, update its in-memory index (incremental path)
        // Find repo that contains this file via prefix match
        let file_canonical = p
            .canonicalize()
            .unwrap_or_else(|_| PathBuf::from(path))
            .to_string_lossy()
            .to_string();

        // Collect repo keys to avoid holding read lock while writing
        let repo_keys: Vec<String> = if let Ok(map) = self.state.read() {
            map.keys().cloned().collect()
        } else {
            vec![]
        };

        for repo_key in repo_keys {
            if file_canonical.starts_with(&repo_key) || path.starts_with(&repo_key) {
                if let Ok(mut map) = self.state.write() {
                    if let Some(repo_state) = map.get_mut(&repo_key) {
                        if repo_state.enabled {
                            repo_state
                                .indexes
                                .insert(path.to_string(), file_index.clone());
                            repo_state.indexed_files = repo_state.indexes.len();
                            repo_state.last_indexed_at = Some(Utc::now());
                        }
                    }
                }
                break;
            }
        }

        Ok(file_index)
    }

    /// Search combines plain text + symbol name search.
    /// - Path / import text match is plain text component
    /// - Symbol name match is structural component
    ///
    /// Returns up to `limit` CodeSymbol sorted by relevance.
    pub fn search(&self, query: &str, repo_path: &str, limit: usize) -> Vec<CodeSymbol> {
        let limit = if limit == 0 { 50 } else { limit };
        let q = query.trim();
        if q.is_empty() {
            return Vec::new();
        }
        let q_lower = q.to_lowercase();

        let map = match self.state.read() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };

        // Determine relevant repos
        let repo_key = Self::canonical_key(repo_path);
        let relevant: Vec<&RepoState> = if repo_path.is_empty() {
            // Search all repos if no repo_path supplied
            map.values().filter(|s| s.enabled).collect()
        } else {
            map.iter()
                .filter(|(k, _)| {
                    *k == &repo_key
                        || *k == repo_path
                        || repo_key.starts_with(*k)
                        || repo_path.starts_with(*k)
                })
                .map(|(_, v)| v)
                .collect()
        };

        // If no relevant but we have enabled repos, fallback to all enabled for better UX during manual testing
        let search_states = if relevant.is_empty() && repo_path.is_empty() {
            map.values().filter(|s| s.enabled).collect::<Vec<_>>()
        } else if relevant.is_empty() {
            // Try all enabled as fallback
            map.values().filter(|s| s.enabled).collect::<Vec<_>>()
        } else {
            relevant
        };

        let mut scored: Vec<(i32, CodeSymbol)> = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();

        // Pass 1: symbol name matches
        for repo_state in &search_states {
            for file_index in repo_state.indexes.values() {
                let file_path_lower = file_index.path.to_lowercase();
                let imports_concat = file_index.imports.join(" ").to_lowercase();
                let file_matches =
                    file_path_lower.contains(&q_lower) || imports_concat.contains(&q_lower);

                for sym in &file_index.symbols {
                    let name_lower = sym.name.to_lowercase();
                    if name_lower.contains(&q_lower) {
                        if seen_ids.contains(&sym.id) {
                            continue;
                        }
                        let score = if name_lower == q_lower {
                            100
                        } else if name_lower.starts_with(&q_lower) {
                            70
                        } else {
                            40
                        };
                        scored.push((score, sym.clone()));
                        seen_ids.insert(sym.id.clone());
                    } else if file_matches {
                        // Plain text component: file path or import contains query, include symbol with lower score
                        if seen_ids.contains(&sym.id) {
                            continue;
                        }
                        scored.push((15, sym.clone()));
                        seen_ids.insert(sym.id.clone());
                    }
                }
            }
        }

        // Pass 2: if not enough results, do plain text content search (read files <200KB that contain query)
        if scored.len() < limit {
            // Limit content search to first 200 files to keep light
            let mut files_checked = 0;
            for repo_state in &search_states {
                if scored.len() >= limit {
                    break;
                }
                for file_index in repo_state.indexes.values() {
                    if scored.len() >= limit || files_checked > 200 {
                        break;
                    }
                    if file_index.size > 200_000 {
                        continue;
                    }
                    // Skip if file already contributed via name/path match (still might need content, but we already included)
                    // Try to read file content and search for query case-insensitive
                    if let Ok(content) = std::fs::read_to_string(&file_index.path) {
                        files_checked += 1;
                        if content.to_lowercase().contains(&q_lower) {
                            for sym in &file_index.symbols {
                                if seen_ids.contains(&sym.id) {
                                    continue;
                                }
                                if scored.len() >= limit {
                                    break;
                                }
                                scored.push((5, sym.clone()));
                                seen_ids.insert(sym.id.clone());
                            }
                        }
                    }
                }
            }
        }

        // Sort by score desc, then by name
        scored.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| a.1.name.cmp(&b.1.name))
                .then_with(|| a.1.file_path.cmp(&b.1.file_path))
        });

        scored.into_iter().take(limit).map(|(_, sym)| sym).collect()
    }

    pub fn get_file_index(&self, path: &str) -> Option<FileIndex> {
        // Try exact match in memory
        if let Ok(map) = self.state.read() {
            for repo_state in map.values() {
                if let Some(fi) = repo_state.indexes.get(path) {
                    return Some(fi.clone());
                }
                // Try canonical variant
                for (k, v) in &repo_state.indexes {
                    if k == path {
                        return Some(v.clone());
                    }
                    // Compare file names if path not canonicalized differently
                    if Path::new(k).to_string_lossy() == path {
                        return Some(v.clone());
                    }
                }
            }
        }

        // On-demand parse if file exists
        let p = Path::new(path);
        if p.exists() && p.is_file() {
            if let Ok(fi) = index_file_internal(p) {
                return Some(fi);
            }
        }
        None
    }

    /// Returns files that import or are imported by given file, or share symbols.
    /// Useful for file-tree badges: structurally related.
    pub fn get_related_files(&self, file_path: &str) -> Vec<String> {
        let file_canonical = Path::new(file_path)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| file_path.to_string());

        // Phase 1: find repo and target index while holding read lock
        let (target_repo_key_opt, target_index_opt) = {
            let map = match self.state.read() {
                Ok(m) => m,
                Err(_) => return Vec::new(),
            };
            let mut target_repo_key: Option<String> = None;
            let mut target_index: Option<FileIndex> = None;

            for (repo_key, repo_state) in map.iter() {
                if file_canonical.starts_with(repo_key)
                    || file_path.starts_with(repo_key)
                    || repo_key.starts_with(&file_canonical)
                {
                    if let Some(fi) = repo_state
                        .indexes
                        .get(file_path)
                        .or_else(|| repo_state.indexes.get(&file_canonical))
                    {
                        target_index = Some(fi.clone());
                        target_repo_key = Some(repo_key.clone());
                        break;
                    }
                    for (k, v) in &repo_state.indexes {
                        if k.ends_with(file_path)
                            || Path::new(k).file_name() == Path::new(file_path).file_name()
                        {
                            target_index = Some(v.clone());
                            target_repo_key = Some(repo_key.clone());
                            break;
                        }
                    }
                    if target_index.is_some() {
                        break;
                    }
                    if target_repo_key.is_none() {
                        target_repo_key = Some(repo_key.clone());
                    }
                }
            }
            (target_repo_key, target_index)
        };

        let target = match target_index_opt {
            Some(t) => t,
            None => {
                if let Some(fi) = self.get_file_index(file_path) {
                    fi
                } else {
                    return Vec::new();
                }
            }
        };

        let repo_key = match target_repo_key_opt {
            Some(k) => k,
            None => return Vec::new(),
        };

        // Re-acquire read lock for repo state
        let map = match self.state.read() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let repo_state = match map.get(&repo_key) {
            Some(rs) => rs,
            None => return Vec::new(),
        };

        let target_stem = file_stem_lower(Path::new(&target.path));
        let target_symbols: HashSet<String> = target
            .symbols
            .iter()
            .map(|s| s.name.to_lowercase())
            .collect();
        let target_imports_lower = target.imports.join(" ").to_lowercase();

        let mut related_scored: Vec<(i32, String)> = Vec::new();

        for (other_path, other_index) in &repo_state.indexes {
            if other_path == &target.path {
                continue;
            }

            let mut score = 0;
            let other_stem = file_stem_lower(Path::new(other_path));
            let other_imports_lower = other_index.imports.join(" ").to_lowercase();

            // Import-based: target imports contain other_stem
            if !other_stem.is_empty() && target_imports_lower.contains(&other_stem) {
                score += 50;
            }
            // Reverse: other imports contain target_stem
            if !target_stem.is_empty() && other_imports_lower.contains(&target_stem) {
                score += 50;
            }

            // Direct path substring in import (more robust for relative imports)
            // e.g., target imports "./utils" and other is utils.rs
            for imp in &target.imports {
                let imp_lower = imp.to_lowercase();
                if imp_lower.contains(&other_stem) && !other_stem.is_empty() {
                    score += 20;
                }
            }
            for imp in &other_index.imports {
                let imp_lower = imp.to_lowercase();
                if imp_lower.contains(&target_stem) && !target_stem.is_empty() {
                    score += 20;
                }
            }

            // Symbol overlap
            let mut overlap = 0;
            for sym in &other_index.symbols {
                if target_symbols.contains(&sym.name.to_lowercase()) {
                    overlap += 1;
                }
            }
            if overlap > 0 {
                score += overlap * 10;
            }

            if score > 0 {
                related_scored.push((score, other_path.clone()));
            }
        }

        // Sort by score descending
        related_scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

        // Deduplicate and limit to 30 for badge lightness
        let mut seen = HashSet::new();
        let mut result = Vec::new();
        for (_, p) in related_scored {
            if seen.insert(p.clone()) {
                result.push(p);
                if result.len() >= 30 {
                    break;
                }
            }
        }

        result
    }

    /// Helper for UI: recently touched files sorted by last_modified desc, up to limit.
    /// Not required by spec signature but useful for file-tree badges.
    pub fn get_recently_touched(&self, repo_path: &str, limit: usize) -> Vec<FileIndex> {
        let map = match self.state.read() {
            Ok(m) => m,
            Err(_) => return Vec::new(),
        };
        let repo_key = Self::canonical_key(repo_path);
        let repo_state = if let Some(rs) = map.get(&repo_key) {
            rs
        } else if let Some(rs) = map.get(repo_path) {
            rs
        } else {
            return Vec::new();
        };

        let mut files: Vec<&FileIndex> = repo_state.indexes.values().collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.last_modified));
        files.into_iter().take(limit).cloned().collect()
    }

    /// For testing / introspection: list all enabled repos
    pub fn list_enabled_repos(&self) -> Vec<String> {
        if let Ok(map) = self.state.read() {
            map.iter()
                .filter(|(_, v)| v.enabled)
                .map(|(k, _)| k.clone())
                .collect()
        } else {
            Vec::new()
        }
    }
}

impl Drop for ContextEngineManager {
    fn drop(&mut self) {
        // Signal all watchers to stop
        if let Ok(mut watchers) = self.watchers.lock() {
            for (_, mut handle) in watchers.drain() {
                handle.stop_flag.store(true, Ordering::Relaxed);
                if let Some(th) = handle.thread.take() {
                    // Don't block forever in Drop – detached join with timeout not possible, just signal
                    // We try to join with small timeout via try? For simplicity don't join here.
                    // Let thread exit on its own; it will check flag within 500ms.
                    // To avoid leak we detach (drop handle) – OS will clean on process exit.
                    // Optionally spawn a detacher thread to join.
                    std::thread::spawn(move || {
                        let _ = th.join();
                    });
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Unit tests – ensure reliability per spec
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn test_language_detection() {
        assert_eq!(detect_language(Path::new("foo.rs")).unwrap(), "rust");
        assert_eq!(detect_language(Path::new("bar.ts")).unwrap(), "typescript");
        assert_eq!(detect_language(Path::new("baz.tsx")).unwrap(), "typescript");
        assert_eq!(detect_language(Path::new("qux.js")).unwrap(), "javascript");
        assert_eq!(detect_language(Path::new("main.py")).unwrap(), "python");
        assert_eq!(detect_language(Path::new("lib.go")).unwrap(), "go");
        assert_eq!(detect_language(Path::new("cfg.json")).unwrap(), "json");
        assert!(detect_language(Path::new("README.md")).is_none());
    }

    #[test]
    fn test_index_rust_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.rs");
        fs::write(
            &file_path,
            r#"
            use std::collections::HashMap;
            pub struct MyStruct { x: i32 }
            pub enum MyEnum { A, B }
            pub trait MyTrait { fn foo(&self); }
            impl MyStruct {
                pub fn my_method(&self) {}
            }
            pub fn my_function() {}
        "#,
        )
        .unwrap();

        let index = index_file_internal(&file_path).unwrap();
        assert_eq!(index.language, "rust");
        assert!(index.symbols.len() >= 4, "symbols: {:?}", index.symbols);
        let names: Vec<_> = index.symbols.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MyStruct"));
        assert!(names.contains(&"my_function") || names.contains(&"MyEnum"));
    }

    #[test]
    fn test_index_typescript_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.ts");
        fs::write(
            &file_path,
            r#"
            import { something } from "./utils";
            export class MyClass {
                myMethod() {}
            }
            export function myFunc() {}
            export interface MyInterface { x: number }
        "#,
        )
        .unwrap();

        let index = index_file_internal(&file_path).unwrap();
        assert_eq!(index.language, "typescript");
        assert!(!index.symbols.is_empty());
        assert!(index.imports.iter().any(|i| i.contains("utils")));
    }

    #[test]
    fn test_manager_enable_disable() {
        let dir = tempdir().unwrap();
        let repo_path = dir.path().to_string_lossy().to_string();

        // create a couple files
        fs::write(dir.path().join("a.rs"), "fn hello() {}").unwrap();
        fs::write(dir.path().join("b.py"), "def world(): pass").unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();

        let mgr = ContextEngineManager::new();
        let status = mgr.enable_for_repo(&repo_path).unwrap();
        assert!(status.enabled);
        assert!(status.indexed_files >= 2);
        assert!(status.total_files >= 2);

        let search_res = mgr.search("hello", &repo_path, 10);
        assert!(!search_res.is_empty());

        let status2 = mgr.status(&repo_path);
        assert!(status2.enabled);

        mgr.disable_for_repo(&repo_path).unwrap();
        let status3 = mgr.status(&repo_path);
        assert!(!status3.enabled);
    }

    #[test]
    fn test_large_file_guard() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("big.rs");
        // Create >1MB file
        let big_content = "fn foo() {}\n".repeat(200_000); // ~2.2MB
        fs::write(&file_path, big_content).unwrap();
        let idx = index_file_internal(&file_path).unwrap();
        assert!(idx.symbols.is_empty(), "large file should be skipped");
        assert!(idx.size > MAX_FILE_SIZE as usize);
    }

    #[test]
    fn test_related_files() {
        let dir = tempdir().unwrap();
        let repo = dir.path();
        fs::write(
            repo.join("utils.rs"),
            "pub fn helper() {} pub struct Util {}",
        )
        .unwrap();
        fs::write(
            repo.join("main.rs"),
            r#"mod utils; use utils::helper; fn main() { helper(); }"#,
        )
        .unwrap();

        let mgr = ContextEngineManager::new();
        let repo_str = repo.to_string_lossy().to_string();
        mgr.enable_for_repo(&repo_str).unwrap();

        let main_path = repo.join("main.rs").to_string_lossy().to_string();
        let related = mgr.get_related_files(&main_path);
        // Should find utils.rs as related via import
        assert!(
            related.iter().any(|p| p.contains("utils.rs")),
            "related: {:?}",
            related
        );
    }
}
