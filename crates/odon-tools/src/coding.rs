use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const INDEX_VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_INDEXED_FILES: usize = 20_000;
const CHUNK_LINES: usize = 40;
const CHUNK_OVERLAP: usize = 5;

#[derive(Debug, Serialize, Deserialize)]
struct CodeIndex {
    version: u32,
    root: String,
    generated_at: u64,
    files: usize,
    chunks: Vec<CodeChunk>,
}

#[derive(Debug, Serialize, Deserialize)]
struct CodeChunk {
    path: String,
    start_line: usize,
    end_line: usize,
    scope: Option<String>,
    content: String,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    path: String,
    start_line: usize,
    end_line: usize,
    score: f64,
    scope: Option<String>,
    snippet: String,
}

pub struct CodeIndexTool;

#[async_trait]
impl Tool for CodeIndexTool {
    fn name(&self) -> &str {
        "code_index"
    }
    fn description(&self) -> &str {
        "Build a persistent, workspace-scoped index of source and configuration files for ranked code search"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Workspace-relative directory to index (default: workspace root)" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let workspace = canonical_workspace(ctx)?;
        let root = resolve_existing_path(
            &workspace,
            args.get("path").and_then(Value::as_str).unwrap_or("."),
        )?;
        if !root.is_dir() {
            return tool_error(
                self.name(),
                format!("{} is not a directory", root.display()),
            );
        }

        let mut files = Vec::new();
        collect_source_files(&root, &mut files)?;
        files.sort();
        if files.len() > MAX_INDEXED_FILES {
            return tool_error(
                self.name(),
                format!(
                    "refusing to index {} files; limit is {MAX_INDEXED_FILES}",
                    files.len()
                ),
            );
        }

        let mut chunks = Vec::new();
        let mut indexed_files = 0usize;
        for file in files {
            let metadata = match fs::metadata(&file) {
                Ok(metadata) if metadata.len() <= MAX_FILE_BYTES => metadata,
                _ => continue,
            };
            if !metadata.is_file() {
                continue;
            }
            let Ok(content) = fs::read_to_string(&file) else {
                continue;
            };
            if content.contains('\0') {
                continue;
            }
            let relative = file.strip_prefix(&workspace).unwrap_or(&file);
            chunks.extend(chunk_source(&path_for_output(relative), &content));
            indexed_files += 1;
        }

        let index = CodeIndex {
            version: INDEX_VERSION,
            root: path_for_output(root.strip_prefix(&workspace).unwrap_or(&root)),
            generated_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            files: indexed_files,
            chunks,
        };
        let index_path = index_path(&workspace);
        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(self.name(), e))?;
        }
        let encoded = serde_json::to_vec(&index).map_err(|e| tool_message(self.name(), e))?;
        fs::write(&index_path, encoded).map_err(|e| io_error(self.name(), e))?;

        Ok(json!({
            "index": path_for_output(index_path.strip_prefix(&workspace).unwrap_or(&index_path)),
            "root": index.root,
            "files": index.files,
            "chunks": index.chunks.len(),
            "version": index.version
        }))
    }
}

pub struct CodeSearchTool;

#[async_trait]
impl Tool for CodeSearchTool {
    fn name(&self) -> &str {
        "code_search"
    }
    fn description(&self) -> &str {
        "Search the persistent code index using BM25 lexical ranking with exact-text and path boosts"
    }
    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["query"],
            "properties": {
                "query": { "type": "string", "minLength": 1 },
                "max_results": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 },
                "path_filter": { "type": "string", "description": "Optional case-insensitive path substring" }
            }
        })
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let workspace = canonical_workspace(ctx)?;
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim();
        if query.is_empty() {
            return tool_error(self.name(), "query must not be empty".to_string());
        }
        let max_results = args
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(10)
            .clamp(1, 50) as usize;
        let path_filter = args
            .get("path_filter")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);

        let index_path = index_path(&workspace);
        let bytes = fs::read(&index_path).map_err(|_| OdonError::Tool {
            tool: self.name().to_string(),
            message: "code index not found; run code_index first".to_string(),
        })?;
        let index: CodeIndex =
            serde_json::from_slice(&bytes).map_err(|e| tool_message(self.name(), e))?;
        if index.version != INDEX_VERSION {
            return tool_error(
                self.name(),
                "code index version is stale; run code_index again".to_string(),
            );
        }

        let query_terms = unique_terms(query);
        if query_terms.is_empty() {
            return tool_error(self.name(), "query has no searchable terms".to_string());
        }
        let candidates = index
            .chunks
            .iter()
            .filter(|chunk| {
                path_filter
                    .as_ref()
                    .map(|filter| chunk.path.to_lowercase().contains(filter))
                    .unwrap_or(true)
            })
            .collect::<Vec<_>>();
        let results = rank_chunks(query, &query_terms, &candidates, max_results);

        Ok(json!({
            "query": query,
            "index_root": index.root,
            "indexed_files": index.files,
            "matches": results.len(),
            "results": results
        }))
    }
}

fn canonical_workspace(ctx: &ToolContext) -> Result<PathBuf> {
    std::path::Path::new(&ctx.workspace_root)
        .canonicalize()
        .map_err(|e| io_error("code_index", e))
}

fn resolve_existing_path(workspace: &Path, input: &str) -> Result<PathBuf> {
    let candidate = Path::new(input);
    let joined = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace.join(candidate)
    };
    let resolved = joined
        .canonicalize()
        .map_err(|e| io_error("code_index", e))?;
    if !resolved.starts_with(workspace) {
        return tool_error(
            "code_index",
            "path escapes the workspace boundary".to_string(),
        );
    }
    Ok(resolved)
}

fn index_path(workspace: &Path) -> PathBuf {
    workspace
        .join(".odon")
        .join("cache")
        .join("code-index.json")
}

fn collect_source_files(directory: &Path, output: &mut Vec<PathBuf>) -> Result<()> {
    let entries = fs::read_dir(directory).map_err(|e| io_error("code_index", e))?;
    for entry in entries {
        let entry = entry.map_err(|e| io_error("code_index", e))?;
        let path = entry.path();
        let file_type = entry.file_type().map_err(|e| io_error("code_index", e))?;
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            if !ignored_directory(&entry.file_name().to_string_lossy()) {
                collect_source_files(&path, output)?;
            }
        } else if file_type.is_file() && indexable_file(&path) {
            output.push(path);
        }
        if output.len() > MAX_INDEXED_FILES {
            break;
        }
    }
    Ok(())
}

fn ignored_directory(name: &str) -> bool {
    matches!(
        name,
        ".git"
            | ".hg"
            | ".svn"
            | ".odon"
            | "node_modules"
            | "target"
            | "dist"
            | "build"
            | ".next"
            | ".nuxt"
            | ".venv"
            | "venv"
            | "__pycache__"
            | "coverage"
            | ".cache"
    )
}

fn indexable_file(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if matches!(
        name,
        "Cargo.toml" | "package.json" | "pyproject.toml" | "go.mod" | "Dockerfile" | "Makefile"
    ) {
        return true;
    }
    matches!(
        path.extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "rs" | "ts"
            | "tsx"
            | "js"
            | "jsx"
            | "mjs"
            | "cjs"
            | "py"
            | "go"
            | "java"
            | "kt"
            | "kts"
            | "c"
            | "h"
            | "cpp"
            | "hpp"
            | "cs"
            | "php"
            | "rb"
            | "swift"
            | "scala"
            | "sh"
            | "ps1"
            | "sql"
            | "graphql"
            | "proto"
            | "toml"
            | "yaml"
            | "yml"
            | "json"
            | "md"
            | "css"
            | "scss"
            | "html"
            | "vue"
            | "svelte"
    )
}

fn chunk_source(path: &str, content: &str) -> Vec<CodeChunk> {
    let lines = content.lines().collect::<Vec<_>>();
    if lines.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut start = 0usize;
    while start < lines.len() {
        let end = (start + CHUNK_LINES).min(lines.len());
        let scope = lines[..end]
            .iter()
            .rev()
            .find_map(|line| scope_header(line));
        chunks.push(CodeChunk {
            path: path.to_string(),
            start_line: start + 1,
            end_line: end,
            scope,
            content: lines[start..end].join("\n"),
        });
        if end == lines.len() {
            break;
        }
        start = end.saturating_sub(CHUNK_OVERLAP);
    }
    chunks
}

fn scope_header(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let prefixes = [
        "fn ",
        "pub fn ",
        "async fn ",
        "pub async fn ",
        "struct ",
        "pub struct ",
        "enum ",
        "pub enum ",
        "trait ",
        "pub trait ",
        "impl ",
        "class ",
        "def ",
        "function ",
        "export function ",
        "export class ",
        "interface ",
        "export interface ",
        "type ",
        "export type ",
    ];
    prefixes
        .iter()
        .find(|prefix| trimmed.starts_with(**prefix))
        .map(|_| trimmed.chars().take(180).collect())
}

fn unique_terms(value: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    tokenize(value)
        .into_iter()
        .filter(|term| seen.insert(term.clone()))
        .collect()
}

fn tokenize(value: &str) -> Vec<String> {
    value
        .split(|ch: char| !ch.is_alphanumeric() && ch != '_')
        .filter(|term| term.len() > 1)
        .map(str::to_lowercase)
        .collect()
}

fn rank_chunks(
    query: &str,
    query_terms: &[String],
    chunks: &[&CodeChunk],
    limit: usize,
) -> Vec<SearchResult> {
    if chunks.is_empty() {
        return Vec::new();
    }
    let documents = chunks
        .iter()
        .map(|chunk| tokenize(&format!("{} {}", chunk.path, chunk.content)))
        .collect::<Vec<_>>();
    let average_length =
        documents.iter().map(Vec::len).sum::<usize>() as f64 / documents.len() as f64;
    let mut document_frequency = HashMap::<&str, usize>::new();
    for term in query_terms {
        document_frequency.insert(
            term,
            documents
                .iter()
                .filter(|tokens| tokens.contains(term))
                .count(),
        );
    }

    let lowered_query = query.to_lowercase();
    let mut scored = chunks
        .iter()
        .zip(documents.iter())
        .filter_map(|(chunk, tokens)| {
            let mut frequencies = HashMap::<&str, usize>::new();
            for token in tokens {
                *frequencies.entry(token).or_default() += 1;
            }
            let mut score = 0.0f64;
            for term in query_terms {
                let frequency = *frequencies.get(term.as_str()).unwrap_or(&0) as f64;
                if frequency == 0.0 {
                    continue;
                }
                let df = *document_frequency.get(term.as_str()).unwrap_or(&0) as f64;
                let idf = (((chunks.len() as f64 - df + 0.5) / (df + 0.5)) + 1.0).ln();
                let normalization = frequency
                    + 1.2 * (1.0 - 0.75 + 0.75 * tokens.len() as f64 / average_length.max(1.0));
                score += idf * (frequency * 2.2) / normalization;
            }
            let lowered_content = chunk.content.to_lowercase();
            let lowered_path = chunk.path.to_lowercase();
            if lowered_content.contains(&lowered_query) {
                score += 8.0;
            }
            if lowered_path.contains(&lowered_query) {
                score += 6.0;
            }
            score += query_terms
                .iter()
                .filter(|term| lowered_path.contains(term.as_str()))
                .count() as f64
                * 1.5;
            (score > 0.0).then(|| SearchResult {
                path: chunk.path.clone(),
                start_line: chunk.start_line,
                end_line: chunk.end_line,
                score: (score * 1000.0).round() / 1000.0,
                scope: chunk.scope.clone(),
                snippet: chunk.content.chars().take(4000).collect(),
            })
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.path.cmp(&b.path))
    });
    scored.truncate(limit);
    scored
}

fn path_for_output(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn tool_error<T>(tool: &str, message: String) -> Result<T> {
    Err(OdonError::Tool {
        tool: tool.to_string(),
        message,
    })
}

fn io_error(tool: &str, error: std::io::Error) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: error.to_string(),
    }
}

fn tool_message(tool: &str, error: impl std::fmt::Display) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_preserve_line_ranges_and_scope() {
        let source =
            "pub struct Agent {\n    name: String,\n}\n\nimpl Agent {\n    pub fn run(&self) {}\n}";
        let chunks = chunk_source("src/agent.rs", source);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].start_line, 1);
        assert_eq!(chunks[0].end_line, 7);
        assert_eq!(chunks[0].scope.as_deref(), Some("pub fn run(&self) {}"));
    }

    #[test]
    fn exact_and_path_matches_rank_first() {
        let exact = CodeChunk {
            path: "src/session.rs".into(),
            start_line: 1,
            end_line: 1,
            scope: None,
            content: "fn switch_session() {}".into(),
        };
        let loose = CodeChunk {
            path: "src/other.rs".into(),
            start_line: 1,
            end_line: 1,
            scope: None,
            content: "session state and switch logic".into(),
        };
        let terms = unique_terms("switch_session");
        let results = rank_chunks("switch_session", &terms, &[&loose, &exact], 10);
        assert_eq!(results[0].path, "src/session.rs");
    }

    #[test]
    fn ignores_generated_and_dependency_directories() {
        assert!(ignored_directory("node_modules"));
        assert!(ignored_directory("target"));
        assert!(!ignored_directory("src"));
    }
}
