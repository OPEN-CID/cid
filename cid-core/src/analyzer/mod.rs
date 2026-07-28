use crate::api::types::{CodeSymbol, FileIndex, SymbolKind};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use tree_sitter::Parser;

pub struct CodeAnalyzer {
    languages: HashMap<String, tree_sitter::Language>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeAnalysisResult {
    pub file_path: String,
    pub language: String,
    pub symbols: Vec<CodeSymbol>,
    pub imports: Vec<String>,
    pub errors: Vec<String>,
}

impl Default for CodeAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl CodeAnalyzer {
    pub fn new() -> Self {
        let mut languages = HashMap::new();

        macro_rules! load_grammar {
            ($name:expr, $lang_const:expr) => {
                let lang: tree_sitter::Language = $lang_const.into();
                languages.insert($name.to_string(), lang);
            };
        }

        load_grammar!("rust", tree_sitter_rust::LANGUAGE);
        load_grammar!("typescript", tree_sitter_typescript::LANGUAGE_TYPESCRIPT);
        load_grammar!("javascript", tree_sitter_javascript::LANGUAGE);
        load_grammar!("python", tree_sitter_python::LANGUAGE);
        load_grammar!("go", tree_sitter_go::LANGUAGE);
        load_grammar!("json", tree_sitter_json::LANGUAGE);

        Self { languages }
    }

    pub fn analyze_file(&self, file_path: &str, content: &str) -> Result<CodeAnalysisResult> {
        let path = Path::new(file_path);
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        let language = match extension {
            "rs" => "rust",
            "ts" | "tsx" => "typescript",
            "js" | "jsx" => "javascript",
            "py" => "python",
            "go" => "go",
            "json" => "json",
            _ => {
                return Ok(CodeAnalysisResult {
                    file_path: file_path.to_string(),
                    language: "unknown".to_string(),
                    symbols: vec![],
                    imports: vec![],
                    errors: vec![format!("Unsupported file extension: {}", extension)],
                })
            }
        };

        let lang = match self.languages.get(language) {
            Some(l) => l.clone(),
            None => {
                return Ok(CodeAnalysisResult {
                    file_path: file_path.to_string(),
                    language: language.to_string(),
                    symbols: vec![],
                    imports: vec![],
                    errors: vec![format!("Language '{}' grammar not loaded", language)],
                })
            }
        };

        let mut parser = Parser::new();
        parser
            .set_language(&lang)
            .map_err(|e| anyhow::anyhow!("Failed to set language: {:?}", e))?;

        let tree = parser
            .parse(content, None)
            .context("Failed to parse content")?;

        let mut symbols = Vec::new();
        let mut imports = Vec::new();

        let root = tree.root_node();
        let mut cursor = root.walk();
        for child in root.children(&mut cursor) {
            self.extract_symbols(&child, &mut symbols, &mut imports, file_path, content);
        }

        Ok(CodeAnalysisResult {
            file_path: file_path.to_string(),
            language: language.to_string(),
            symbols,
            imports,
            errors: vec![],
        })
    }

    fn extract_symbols(
        &self,
        node: &tree_sitter::Node,
        symbols: &mut Vec<CodeSymbol>,
        imports: &mut Vec<String>,
        file_path: &str,
        content: &str,
    ) {
        match node.kind() {
            "function_item"
            | "function_definition"
            | "function_declaration"
            | "method_definition" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                        let kind = match node.kind() {
                            "method_definition" => SymbolKind::Method,
                            _ => SymbolKind::Function,
                        };
                        let (line, col) = position(node.start_position());
                        let (end_line, end_col) = position(node.end_position());
                        symbols.push(CodeSymbol {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: name.to_string(),
                            kind,
                            file_path: file_path.to_string(),
                            line,
                            column: col,
                            end_line,
                            end_column: end_col,
                            parent: None,
                            imports: vec![],
                        });
                    }
                }
            }
            "class_declaration" | "class_definition" | "struct_item" => {
                if let Some(name_node) = node.child_by_field_name("name") {
                    if let Ok(name) = name_node.utf8_text(content.as_bytes()) {
                        let kind = match node.kind() {
                            "struct_item" => SymbolKind::Struct,
                            _ => SymbolKind::Class,
                        };
                        let (line, col) = position(node.start_position());
                        let (end_line, end_col) = position(node.end_position());
                        symbols.push(CodeSymbol {
                            id: uuid::Uuid::new_v4().to_string(),
                            name: name.to_string(),
                            kind,
                            file_path: file_path.to_string(),
                            line,
                            column: col,
                            end_line,
                            end_column: end_col,
                            parent: None,
                            imports: vec![],
                        });
                    }
                }
            }
            "use_declaration" => {
                if let Ok(text) = node.utf8_text(content.as_bytes()) {
                    imports.push(text.to_string());
                }
            }
            "import_statement" | "import_declaration" => {
                if let Some(src) = node.child_by_field_name("source") {
                    if let Ok(text) = src.utf8_text(content.as_bytes()) {
                        imports.push(text.to_string());
                    }
                }
            }
            _ => {}
        }

        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            self.extract_symbols(&child, symbols, imports, file_path, content);
        }
    }

    pub fn analyze_directory(&self, dir_path: &str) -> Result<Vec<FileIndex>> {
        let mut files = Vec::new();
        let dir = Path::new(dir_path);
        if !dir.exists() {
            return Ok(files);
        }

        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    if matches!(ext, "rs" | "ts" | "js" | "py" | "go" | "json") {
                        if let Ok(content) = std::fs::read_to_string(&path) {
                            let fp = path.to_string_lossy().to_string();
                            if let Ok(result) = self.analyze_file(&fp, &content) {
                                files.push(FileIndex {
                                    path: fp,
                                    language: result.language,
                                    symbols: result.symbols,
                                    imports: result.imports,
                                    last_modified: chrono::Utc::now(),
                                    size: content.len(),
                                });
                            }
                        }
                    }
                }
            } else if path.is_dir() {
                files.extend(self.analyze_directory(path.to_string_lossy().as_ref())?);
            }
        }
        Ok(files)
    }
}

fn position(p: tree_sitter::Point) -> (usize, usize) {
    (p.row + 1, p.column + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn test_analyzer_creation() {
        let analyzer = CodeAnalyzer::new();
        // At least Rust grammar should be loaded
        assert!(analyzer.languages.contains_key("rust"));
    }

    #[test]
    fn test_analyze_rust_file() {
        let analyzer = CodeAnalyzer::new();
        let content = r#"
fn main() {
    println!("Hello");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

struct Config {
    name: String,
}
"#;
        let result = analyzer.analyze_file("test.rs", content).unwrap();
        assert_eq!(result.language, "rust");

        let func_names: Vec<&str> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .map(|s| s.name.as_str())
            .collect();
        assert!(func_names.contains(&"main"), "Should find 'main' function");
        assert!(func_names.contains(&"add"), "Should find 'add' function");

        let struct_names: Vec<&str> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Struct)
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            struct_names.contains(&"Config"),
            "Should find 'Config' struct"
        );
    }

    #[test]
    fn test_analyze_python_file() {
        let analyzer = CodeAnalyzer::new();
        let content = r#"
import os

class DataProcessor:
    def __init__(self):
        self.data = []
    
    def process(self, item):
        return item.upper()

def helper():
    pass
"#;
        let result = analyzer.analyze_file("test.py", content).unwrap();
        assert_eq!(result.language, "python");

        let class_names: Vec<&str> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Class)
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            class_names.contains(&"DataProcessor"),
            "Should find 'DataProcessor' class, found: {:?}",
            class_names
        );

        let func_names: Vec<&str> = result
            .symbols
            .iter()
            .filter(|s| s.kind == SymbolKind::Function)
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            func_names.contains(&"helper"),
            "Should find 'helper' function, found: {:?}",
            func_names
        );
    }

    #[test]
    fn test_analyze_go_file() {
        let analyzer = CodeAnalyzer::new();
        let content = r#"
package main

import "fmt"

type Server struct {
    Port int
}

func (s *Server) Start() error {
    return nil
}

func NewServer(port int) *Server {
    return &Server{Port: port}
}
"#;
        let result = analyzer.analyze_file("test.go", content).unwrap();
        assert_eq!(result.language, "go");
        assert!(!result.symbols.is_empty());
    }

    #[test]
    fn test_unsupported_extension() {
        let analyzer = CodeAnalyzer::new();
        let result = analyzer.analyze_file("test.txt", "hello").unwrap();
        assert_eq!(result.language, "unknown");
        assert!(!result.errors.is_empty());
    }

    #[test]
    fn test_analyze_directory() {
        let analyzer = CodeAnalyzer::new();
        let tmp = TempDir::new().unwrap();

        let rs_file_path = tmp.path().join("main.rs");
        let mut rs_file = std::fs::File::create(&rs_file_path).unwrap();
        writeln!(rs_file, "fn main() {{}}").unwrap();

        let py_file_path = tmp.path().join("utils.py");
        let mut py_file = std::fs::File::create(&py_file_path).unwrap();
        writeln!(py_file, "def util(): pass").unwrap();

        let results = analyzer
            .analyze_directory(tmp.path().to_str().unwrap())
            .unwrap();
        assert_eq!(results.len(), 2);

        let languages: Vec<&str> = results.iter().map(|f| f.language.as_str()).collect();
        assert!(languages.contains(&"rust"));
        assert!(languages.contains(&"python"));
    }

    #[test]
    fn test_search_symbols() {
        let analyzer = CodeAnalyzer::new();
        let tmp = TempDir::new().unwrap();

        let rs_file_path = tmp.path().join("main.rs");
        let mut rs_file = std::fs::File::create(&rs_file_path).unwrap();
        writeln!(
            rs_file,
            "fn main() {{}}\nfn helper() {{}}\nstruct MyStruct {{}}"
        )
        .unwrap();

        let files = analyzer
            .analyze_directory(tmp.path().to_str().unwrap())
            .unwrap();
        let all_symbols: Vec<&CodeSymbol> = files.iter().flat_map(|f| f.symbols.iter()).collect();

        let main_symbols: Vec<&&CodeSymbol> =
            all_symbols.iter().filter(|s| s.name == "main").collect();
        assert_eq!(main_symbols.len(), 1);

        let my_structs: Vec<&&CodeSymbol> = all_symbols
            .iter()
            .filter(|s| s.name == "MyStruct")
            .collect();
        assert_eq!(my_structs.len(), 1);
    }
}
