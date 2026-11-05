use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const INDEX_VERSION: u32 = 2;
const MAX_FILE_BYTES: u64 = 512 * 1024;
const MAX_INDEXED_FILES: usize = 20_000;
const CHUNK_LINES: usize = 40;
const CHUNK_OVERLAP: usize = 5;

/// Maximum characters of one chunk snippet handed back per hit.
///
/// Retrieval is ranked, so breadth beats depth: ten hits of 1 200 characters
/// tell the model far more than two hits of 6 000, and the whole result set has
/// to stay inside the request budget of the turn that asked for it.
const MAX_SNIPPET_CHARS: usize = 1_200;
/// Upper bound on OR terms derived from one query, so a pasted paragraph cannot
/// become a thousand-clause FTS expression.
const MAX_QUERY_TERMS: usize = 12;

#[derive(Debug)]
struct IndexedFile {
    path: String,
    modified_unix_ns: i64,
    bytes: i64,
}

#[derive(Debug, Default)]
struct IndexUpdate {
    indexed_files: usize,
    indexed_chunks: usize,
    changed_files: usize,
    removed_files: usize,
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
    /// True when the hit came from the file name rather than its body.
    path_match: bool,
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

        let index_path = index_path(&workspace);
        if let Some(parent) = index_path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(self.name(), e))?;
        }
        let mut connection = open_index(&index_path, self.name())?;
        let root_name = path_for_output(root.strip_prefix(&workspace).unwrap_or(&root));
        let update = update_index(&mut connection, &workspace, &root, &root_name, files)?;

        Ok(json!({
            "index": path_for_output(index_path.strip_prefix(&workspace).unwrap_or(&index_path)),
            "root": root_name,
            "files": update.indexed_files,
            "chunks": update.indexed_chunks,
            "changed_files": update.changed_files,
            "removed_files": update.removed_files,
            "version": INDEX_VERSION
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
        "Search the persistent code index: file-name hits first, then BM25-ranked chunk matches. Call code_index first if no index exists."
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
        if !index_path.is_file() {
            return Err(OdonError::Tool {
                tool: self.name().to_string(),
                message: "code index not found; run code_index first".to_string(),
            });
        }
        let connection = open_index(&index_path, self.name())?;
        let (index_root, indexed_files, results) = search_index(
            &connection,
            query,
            path_filter.as_deref(),
            max_results,
            self.name(),
        )?;

        let match_count = results.len();
        let index_age_secs = index_age_secs(&connection);
        // An hour is the point where a snapshot stops resembling the tree the
        // operator is editing in this session; the number itself is always
        // reported, so the model can judge instead of trusting the flag.
        const STALE_AFTER_SECS: u64 = 3600;
        let stale = index_age_secs.is_some_and(|age| age > STALE_AFTER_SECS);
        let note = if match_count == 0 {
            Some(format!(
                "no indexed chunk matched '{query}' across {indexed_files} indexed file(s); \
the query may be wrong, or the index stale. Re-run code_index, or verify with fs_find/fs_read \
before concluding the code does not exist."
            ))
        } else if stale {
            Some(format!(
                "these hits come from an index {} second(s) old. Re-run code_index before \
reporting this as the current state of the file, and prefer fs_read when the line matters.",
                index_age_secs.unwrap_or_default()
            ))
        } else {
            None
        };
        Ok(json!({
            "query": query,
            "index_root": index_root,
            "indexed_files": indexed_files,
            "index_age_secs": index_age_secs,
            "stale": stale,
            "matches": match_count,
            "results": results,
            "note": note
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
        .join("code-index.sqlite3")
}

/// Age of the snapshot in seconds, when the index recorded one.
///
/// A stale index is more dangerous than a missing one: it answers confidently
/// with text the operator already edited away, and nothing in the result says
/// so. Reporting the age lets the model decide whether to re-index instead of
/// asserting yesterday's code as today's.
fn index_age_secs(connection: &Connection) -> Option<u64> {
    let indexed_at = connection
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'indexed_at'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .ok()
        .flatten()?
        .parse::<u64>()
        .ok()?;
    let now = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(indexed_at);
    Some(now.saturating_sub(indexed_at))
}

fn database_error(tool: &str, error: rusqlite::Error) -> OdonError {
    OdonError::Tool {
        tool: tool.to_string(),
        message: format!("code index database error: {error}"),
    }
}

fn open_index(path: &Path, tool: &str) -> Result<Connection> {
    let connection = Connection::open(path).map_err(|error| database_error(tool, error))?;
    connection
        .execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS index_meta (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS indexed_files (
                 path TEXT PRIMARY KEY,
                 modified_unix_ns INTEGER NOT NULL,
                 bytes INTEGER NOT NULL
             );
             CREATE VIRTUAL TABLE IF NOT EXISTS code_chunks USING fts5(
                 path UNINDEXED,
                 start_line UNINDEXED,
                 end_line UNINDEXED,
                 scope,
                 snippet,
                 tokenize = 'unicode61 tokenchars _'
             );",
        )
        .map_err(|error| database_error(tool, error))?;
    Ok(connection)
}

fn file_metadata(path: &Path, relative: String) -> Option<IndexedFile> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    let modified_unix_ns = metadata
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos()
        .min(i64::MAX as u128) as i64;
    Some(IndexedFile {
        path: relative,
        modified_unix_ns,
        bytes: metadata.len().min(i64::MAX as u64) as i64,
    })
}

fn delete_file_chunks(transaction: &Transaction<'_>, path: &str, tool: &str) -> Result<()> {
    transaction
        .execute("DELETE FROM code_chunks WHERE path = ?1", params![path])
        .map_err(|error| database_error(tool, error))?;
    transaction
        .execute("DELETE FROM indexed_files WHERE path = ?1", params![path])
        .map_err(|error| database_error(tool, error))?;
    Ok(())
}

fn update_index(
    connection: &mut Connection,
    workspace: &Path,
    root: &Path,
    root_name: &str,
    files: Vec<PathBuf>,
) -> Result<IndexUpdate> {
    let tool = "code_index";
    let transaction = connection
        .transaction()
        .map_err(|error| database_error(tool, error))?;
    let previous_root: Option<String> = transaction
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'root'",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| database_error(tool, error))?;
    if previous_root
        .as_deref()
        .is_some_and(|value| value != root_name)
    {
        transaction
            .execute_batch("DELETE FROM code_chunks; DELETE FROM indexed_files;")
            .map_err(|error| database_error(tool, error))?;
    }

    let mut seen = HashSet::new();
    let mut changed_files = 0usize;
    for file in files {
        let relative = path_for_output(file.strip_prefix(workspace).unwrap_or(&file));
        let Some(indexed) = file_metadata(&file, relative.clone()) else {
            continue;
        };
        seen.insert(relative.clone());
        let unchanged = transaction
            .query_row(
                "SELECT modified_unix_ns = ?2 AND bytes = ?3 FROM indexed_files WHERE path = ?1",
                params![&relative, indexed.modified_unix_ns, indexed.bytes],
                |row| row.get::<_, bool>(0),
            )
            .optional()
            .map_err(|error| database_error(tool, error))?
            .unwrap_or(false);
        if unchanged {
            continue;
        }
        let Ok(content) = fs::read_to_string(&file) else {
            delete_file_chunks(&transaction, &relative, tool)?;
            continue;
        };
        if content.contains('\0') {
            continue;
        }
        delete_file_chunks(&transaction, &relative, tool)?;
        for chunk in chunk_source(&relative, &content) {
            transaction
                .execute(
                    "INSERT INTO code_chunks(path, start_line, end_line, scope, snippet)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![
                        chunk.path,
                        chunk.start_line as i64,
                        chunk.end_line as i64,
                        chunk.scope,
                        chunk.content
                    ],
                )
                .map_err(|error| database_error(tool, error))?;
        }
        transaction
            .execute(
                "INSERT INTO indexed_files(path, modified_unix_ns, bytes) VALUES (?1, ?2, ?3)",
                params![indexed.path, indexed.modified_unix_ns, indexed.bytes],
            )
            .map_err(|error| database_error(tool, error))?;
        changed_files += 1;
    }

    let existing = {
        let mut statement = transaction
            .prepare("SELECT path FROM indexed_files")
            .map_err(|error| database_error(tool, error))?;
        let paths = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| database_error(tool, error))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|error| database_error(tool, error))?;
        paths
    };
    let mut removed_files = 0usize;
    for path in existing {
        let absolute = workspace.join(Path::new(&path));
        if !seen.contains(&path) || !absolute.starts_with(root) {
            delete_file_chunks(&transaction, &path, tool)?;
            removed_files += 1;
        }
    }
    let now_unix = std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    transaction
        .execute(
            "INSERT OR REPLACE INTO index_meta(key, value) VALUES ('version', ?1), ('root', ?2), ('indexed_at', ?3)",
            params![INDEX_VERSION.to_string(), root_name, now_unix.to_string()],
        )
        .map_err(|error| database_error(tool, error))?;
    let indexed_files = transaction
        .query_row("SELECT COUNT(*) FROM indexed_files", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| database_error(tool, error))? as usize;
    let indexed_chunks = transaction
        .query_row("SELECT COUNT(*) FROM code_chunks", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| database_error(tool, error))? as usize;
    transaction
        .commit()
        .map_err(|error| database_error(tool, error))?;
    Ok(IndexUpdate {
        indexed_files,
        indexed_chunks,
        changed_files,
        removed_files,
    })
}

fn fts_query(query: &str) -> String {
    unique_terms(query)
        .into_iter()
        .take(MAX_QUERY_TERMS)
        .map(|term| format!("\"{}\"*", term.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" OR ")
}

/// Trim a chunk to the per-hit budget, saying so rather than silently cutting.
fn cap_snippet(snippet: &str) -> String {
    if snippet.chars().count() <= MAX_SNIPPET_CHARS {
        return snippet.to_string();
    }
    let kept: String = snippet.chars().take(MAX_SNIPPET_CHARS).collect();
    format!("{kept}\n... [chunk clipped here; open the file at this range for the rest]")
}

fn search_index(
    connection: &Connection,
    query: &str,
    path_filter: Option<&str>,
    limit: usize,
    tool: &str,
) -> Result<(String, usize, Vec<SearchResult>)> {
    let match_query = fts_query(query);
    if match_query.is_empty() {
        return tool_error(tool, "query has no searchable terms".to_string());
    }
    let index_root = connection
        .query_row(
            "SELECT value FROM index_meta WHERE key = 'root'",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| database_error(tool, error))?
        .ok_or_else(|| OdonError::Tool {
            tool: tool.to_string(),
            message: "code index is empty; run code_index first".to_string(),
        })?;
    let indexed_files = connection
        .query_row("SELECT COUNT(*) FROM indexed_files", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|error| database_error(tool, error))? as usize;
    let filter = path_filter.unwrap_or("").to_lowercase();
    let pattern = format!("%{filter}%");
    let terms = unique_terms(query);

    // Pass 1: file-name hits. `path` is UNINDEXED in the FTS table, so a query
    // that names a file or module ("runtime.rs", "session pool") would otherwise
    // only find chunks whose *body* repeats the words. Top half of the budget
    // goes to these, because naming a file is the strongest relevance signal a
    // query can carry.
    let mut results: Vec<SearchResult> = Vec::new();
    let mut seen: HashSet<(String, usize)> = HashSet::new();
    let mut seen_paths: HashSet<String> = HashSet::new();
    let path_budget = limit.div_ceil(2);
    {
        let mut statement = connection
            .prepare(
                "SELECT path, start_line, end_line, scope, snippet
                 FROM code_chunks
                 WHERE (?1 = '' OR lower(path) LIKE ?2)
                 ORDER BY path, start_line
                 LIMIT 4000",
            )
            .map_err(|error| database_error(tool, error))?;
        let rows = statement
            .query_map(params![filter, pattern], |row| {
                Ok(SearchResult {
                    path: row.get(0)?,
                    start_line: row.get::<_, i64>(1)? as usize,
                    end_line: row.get::<_, i64>(2)? as usize,
                    score: 0.0,
                    scope: row.get(3)?,
                    snippet: row.get(4)?,
                    path_match: true,
                })
            })
            .map_err(|error| database_error(tool, error))?;
        for row in rows {
            let mut hit = row.map_err(|error| database_error(tool, error))?;
            let lower_path = hit.path.to_lowercase();
            if !terms.iter().any(|term| lower_path.contains(term)) {
                continue;
            }
            // One chunk per file keeps the list about files, not about the
            // forty chunks a large file happens to have.
            if !seen_paths.insert(hit.path.clone()) {
                continue;
            }
            hit.snippet = cap_snippet(&hit.snippet);
            seen.insert((hit.path.clone(), hit.start_line));
            results.push(hit);
            if results.len() >= path_budget {
                break;
            }
        }
    }

    // Pass 2: ranked body matches, which is what BM25 is actually for.
    if results.len() < limit {
        let match_query = fts_query(query);
        if match_query.is_empty() && results.is_empty() {
            return tool_error(tool, "query has no searchable terms".to_string());
        }
        if !match_query.is_empty() {
            let remaining = limit - results.len();
            let mut statement = connection
                .prepare(
                    "SELECT path, start_line, end_line, scope, snippet,
                            -bm25(code_chunks, 0.0, 0.0, 0.0, 0.8, 1.0) AS score
                     FROM code_chunks
                     WHERE code_chunks MATCH ?1
                       AND (?2 = '' OR lower(path) LIKE ?3)
                     ORDER BY bm25(code_chunks, 0.0, 0.0, 0.0, 0.8, 1.0)
                     LIMIT ?4",
                )
                .map_err(|error| database_error(tool, error))?;
            // Over-fetch a little: the dedupe against pass 1 may drop rows.
            let rows = statement
                .query_map(
                    params![match_query, filter, pattern, (remaining * 2) as i64],
                    |row| {
                        Ok(SearchResult {
                            path: row.get(0)?,
                            start_line: row.get::<_, i64>(1)? as usize,
                            end_line: row.get::<_, i64>(2)? as usize,
                            scope: row.get(3)?,
                            snippet: row.get(4)?,
                            score: row.get(5)?,
                            path_match: false,
                        })
                    },
                )
                .map_err(|error| database_error(tool, error))?;
            for row in rows {
                let mut hit = row.map_err(|error| database_error(tool, error))?;
                if !seen.insert((hit.path.clone(), hit.start_line)) {
                    continue;
                }
                hit.snippet = cap_snippet(&hit.snippet);
                results.push(hit);
                if results.len() >= limit {
                    break;
                }
            }
        }
    }

    // An empty result is a valid answer, not a failure: `code_search` reports
    // `matches: 0` plus the index coverage, so the model can distinguish "the
    // index has nothing for this query" from "the search did not run". Turning
    // it into a tool error invited the model to conclude the workspace was
    // empty and to invent an answer instead.
    Ok((index_root, indexed_files, results))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

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
    fn fts_query_preserves_symbols_as_searchable_terms() {
        assert_eq!(fts_query("switch_session"), "\"switch_session\"*");
        assert_eq!(
            fts_query("spawn child-agent"),
            "\"spawn\"* OR \"child\"* OR \"agent\"*"
        );
    }

    #[test]
    fn sqlite_index_updates_and_removes_files_incrementally() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("odon-fts-{unique}"));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("session_pool.rs");
        fs::write(&source, "pub fn spawn_child() { /* session pool */ }").unwrap();
        let database = root.join("index.sqlite3");
        let mut connection = open_index(&database, "test").unwrap();

        let first = update_index(&mut connection, &root, &root, ".", vec![source.clone()]).unwrap();
        assert_eq!(first.changed_files, 1);
        // A freshly written index reports its own age, so `code_search` can say
        // how old the snapshot is instead of presenting it as live state.
        assert_eq!(index_age_secs(&connection), Some(0));
        let (_, _, matches) = search_index(&connection, "spawn child", None, 10, "test").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, "session_pool.rs");

        let unchanged =
            update_index(&mut connection, &root, &root, ".", vec![source.clone()]).unwrap();
        assert_eq!(unchanged.changed_files, 0);

        fs::remove_file(&source).unwrap();
        let removed = update_index(&mut connection, &root, &root, ".", vec![]).unwrap();
        assert_eq!(removed.removed_files, 1);
        let (_, _, matches) = search_index(&connection, "spawn", None, 10, "test").unwrap();
        assert!(matches.is_empty());

        drop(connection);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn ignores_generated_and_dependency_directories() {
        assert!(ignored_directory("node_modules"));
        assert!(ignored_directory("target"));
        assert!(!ignored_directory("src"));
    }
}
