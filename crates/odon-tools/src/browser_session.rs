use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, path::{Path, PathBuf}, sync::Arc, time::Duration};
use tokio::{process::{Child, Command}, sync::Mutex};

// ── Data ────────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
struct Meta {
    session_id: String,
    profile_dir: String,
    port: u16,
    pid: u32,
    url: String,
    created_at: String,
    last_used_at: String,
}

struct Live {
    meta: Meta,
    child: Child,
}

// Live (running) sessions keyed by session_id.
type LiveStore = Arc<Mutex<HashMap<String, Arc<Mutex<Live>>>>>;
static LIVE: std::sync::OnceLock<LiveStore> = std::sync::OnceLock::new();
fn live() -> &'static LiveStore {
    LIVE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

// Persisted (metadata-only, no running process) sessions keyed by session_id.
type PersistedStore = Arc<Mutex<HashMap<String, Meta>>>;
static PERSISTED: std::sync::OnceLock<PersistedStore> = std::sync::OnceLock::new();
fn persisted() -> &'static PersistedStore {
    PERSISTED.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

// ── Path helpers ────────────────────────────────────────────────────────────

const METADATA_FILE: &str = "session.json";

/// The root directory that holds all browser-session subdirectories.
fn sessions_dir(workspace_root: &str) -> PathBuf {
    Path::new(workspace_root).join(".odon/browser-sessions")
}

/// Directory for a single session: `<sessions_dir>/<session_id>`.
fn session_dir(workspace_root: &str, session_id: &str) -> PathBuf {
    sessions_dir(workspace_root).join(session_id)
}

// ── Persistence helpers ─────────────────────────────────────────────────────

/// Scan the sessions directory and load every readable `session.json` into the
/// in-memory persisted store.  Called once from `store_init()` so that
/// `browser_session_resume` and `browser_session_list` can discover sessions
/// from a previous process lifetime.
async fn load_persisted_sessions(workspace_root: &str) {
    let dir = sessions_dir(workspace_root);
    let mut entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    let mut map = HashMap::new();
    while let Some(Ok(entry)) = entries.next() {
        let path = entry.path().join(METADATA_FILE);
        if let Ok(bytes) = std::fs::read(&path) {
            if let Ok(meta) = serde_json::from_slice::<Meta>(&bytes) {
                map.insert(meta.session_id.clone(), meta);
            }
        }
    }
    *persisted().lock().await = map;
}

/// Persist a `Meta` record to `<session_dir>/session.json`.
fn save_meta(workspace_root: &str, meta: &Meta) -> Result<()> {
    let dir = session_dir(workspace_root, &meta.session_id);
    std::fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
    std::fs::write(dir.join(METADATA_FILE), serde_json::to_vec_pretty(meta).unwrap())
        .map_err(|e| err(e.to_string()))
}

/// Return the persisted `Meta` for `session_id` if present on disk, without
/// touching the in-memory stores.
fn find_persisted_meta(workspace_root: &str, session_id: &str) -> Result<Meta> {
    let path = session_dir(workspace_root, session_id).join(METADATA_FILE);
    let bytes = std::fs::read(&path).map_err(|_| err("session metadata not found"))?;
    serde_json::from_slice(&bytes).map_err(|e| err(e.to_string()))
}

// ── Utilities ───────────────────────────────────────────────────────────────

fn err(msg: impl Into<String>) -> OdonError {
    OdonError::Tool { tool: "browser_session".into(), message: msg.into() }
}

fn sid() -> String {
    format!("session_{}", chrono::Utc::now().timestamp_millis())
}

fn validate_session_id(id: &str) -> Result<&str> {
    if id.is_empty()
        || id.len() > 96
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(err(
            "session_id must contain only ASCII letters, numbers, '_' or '-' and be at most 96 characters",
        ));
    }
    Ok(id)
}

fn browser() -> Result<PathBuf> {
    let mut p = Vec::new();
    if cfg!(windows) {
        if let Ok(x) = std::env::var("PROGRAMFILES") {
            p.push(PathBuf::from(x.clone()).join("Google/Chrome/Application/chrome.exe"));
            p.push(PathBuf::from(x).join("Microsoft/Edge/Application/msedge.exe"));
        }
        if let Ok(x) = std::env::var("LOCALAPPDATA") {
            p.push(PathBuf::from(x.clone()).join("Google/Chrome/Application/chrome.exe"));
            p.push(PathBuf::from(x).join("Microsoft/Edge/Application/msedge.exe"));
        }
    } else {
        p.extend([
            PathBuf::from("/usr/bin/google-chrome"),
            PathBuf::from("/usr/bin/chromium"),
            PathBuf::from("/usr/bin/microsoft-edge"),
        ]);
    }
    p.into_iter()
        .find(|x| x.is_file())
        .ok_or_else(|| err("Chrome/Chromium/Edge executable not found"))
}

async fn port() -> Result<u16> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| err(e.to_string()))?;
    Ok(l.local_addr().map_err(|e| err(e.to_string()))?.port())
}

async fn cdp(id: &str, method: &str, params: Value) -> Result<Value> {
    let live = live().lock().await.get(id).cloned().ok_or_else(|| {
        err(format!("session '{id}' is not live; resume it first"))
    })?;
    let p = live.lock().await.meta.port;
    let ts: Vec<Value> = reqwest::get(format!("http://127.0.0.1:{p}/json"))
        .await
        .map_err(|e| err(e.to_string()))?
        .json()
        .await
        .map_err(|e| err(e.to_string()))?;
    let u = ts
        .iter()
        .find_map(|x| x.get("webSocketDebuggerUrl").and_then(Value::as_str))
        .ok_or_else(|| err("no active browser tab"))?;
    let (mut ws, _) = tokio_tungstenite::connect_async(u)
        .await
        .map_err(|e| err(e.to_string()))?;
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        json!({ "id": 1, "method": method, "params": params })
            .to_string()
            .into(),
    ))
    .await
    .map_err(|e| err(e.to_string()))?;
    let m = tokio::time::timeout(Duration::from_secs(20), ws.next())
        .await
        .map_err(|_| err("CDP timeout"))?
        .ok_or_else(|| err("CDP closed"))?
        .map_err(|e| err(e.to_string()))?;
    match m {
        tokio_tungstenite::tungstenite::Message::Text(x) => {
            serde_json::from_str(&x).map_err(|e| err(e.to_string()))
        }
        _ => Err(err("invalid CDP response")),
    }
}

fn schema(props: Value, req: Value) -> Value {
    json!({ "type": "object", "properties": props, "required": req })
}

// ── Tools ───────────────────────────────────────────────────────────────────

pub struct BrowserSessionCreateTool;
#[async_trait]
impl Tool for BrowserSessionCreateTool {
    fn name(&self) -> &str {
        "browser_session_create"
    }
    fn description(&self) -> &str {
        "Create a persistent Chromium profile session for reusable cookies and localStorage"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "name": { "type": "string" }, "url": { "type": "string" } }),
            json!([]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, a: Value) -> Result<Value> {
        let id = sid();
        let root = session_dir(&ctx.workspace_root, &id);
        std::fs::create_dir_all(&root).map_err(|e| err(e.to_string()))?;
        let p = port().await?;
        let url = a.get("url").and_then(Value::as_str).unwrap_or("about:blank");
        let mut c = Command::new(browser()?);
        c.args([
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            &format!("--remote-debugging-port={p}"),
            &format!("--user-data-dir={}", root.display()),
            url,
        ]);
        let child = c.spawn().map_err(|e| err(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        let m = Meta {
            session_id: id.clone(),
            profile_dir: root.display().to_string(),
            port: p,
            pid: child.id().unwrap_or(0),
            url: url.into(),
            created_at: now.clone(),
            last_used_at: now,
        };
        // Persist metadata so the session survives a process restart.
        save_meta(&ctx.workspace_root, &m)?;
        live()
            .lock()
            .await
            .insert(id.clone(), Arc::new(Mutex::new(Live { meta: m.clone(), child })));
        Ok(json!(m))
    }
}

pub struct BrowserSessionListTool;
#[async_trait]
impl Tool for BrowserSessionListTool {
    fn name(&self) -> &str {
        "browser_session_list"
    }
    fn description(&self) -> &str {
        "List live persistent browser sessions"
    }
    fn parameters_schema(&self) -> Value {
        schema(json!({}), json!([]))
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _: &ToolContext, _: Value) -> Result<Value> {
        // Collect live sessions.
        let mut v: Vec<Value> = Vec::new();
        for x in live().lock().await.values() {
            v.push(json!(x.lock().await.meta.clone()));
        }
        // Also include persisted sessions whose browser process is no longer
        // running (e.g. after a process restart), so callers can see what is
        // available to resume.
        for meta in persisted().lock().await.values() {
            if !live().lock().await.contains_key(&meta.session_id) {
                v.push(json!(meta.clone()));
            }
        }
        Ok(json!(v))
    }
}

pub struct BrowserSessionResumeTool;
#[async_trait]
impl Tool for BrowserSessionResumeTool {
    fn name(&self) -> &str {
        "browser_session_resume"
    }
    fn description(&self) -> &str {
        "Resume a persistent browser profile by session id"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" } }),
            json!(["session_id"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, a: Value) -> Result<Value> {
        let id = validate_session_id(
            a.get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| err("session_id is required"))?,
        )?;

        // If the session is already live in this process, hand it back directly.
        if let Some(x) = live().lock().await.get(id).cloned() {
            let mut l = x.lock().await;
            l.meta.last_used_at = chrono::Utc::now().to_rfc3339();
            return Ok(json!(l.meta.clone()));
        }

        // Otherwise load persisted metadata from disk and spawn a fresh browser
        // that reuses the saved profile directory.
        let m = find_persisted_meta(&ctx.workspace_root, id)?;
        let p = port().await?;
        let mut c = Command::new(browser()?);
        c.args([
            "--headless=new",
            "--disable-gpu",
            "--no-first-run",
            "--no-default-browser-check",
            &format!("--remote-debugging-port={p}"),
            &format!("--user-data-dir={}", m.profile_dir),
            &m.url,
        ]);
        let child = c.spawn().map_err(|e| err(e.to_string()))?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut n = m.clone();
        n.port = p;
        n.pid = child.id().unwrap_or(0);
        n.last_used_at = now;
        // Overwrite the metadata file so the new port/pid is durable.
        save_meta(&ctx.workspace_root, &n)?;
        live()
            .lock()
            .await
            .insert(id.into(), Arc::new(Mutex::new(Live { meta: n.clone(), child })));
        Ok(json!(n))
    }
}

pub struct BrowserSessionCloseTool;
#[async_trait]
impl Tool for BrowserSessionCloseTool {
    fn name(&self) -> &str {
        "browser_session_close"
    }
    fn description(&self) -> &str {
        "Close a persistent browser session"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" } }),
            json!(["session_id"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _: &ToolContext, a: Value) -> Result<Value> {
        let id = validate_session_id(
            a.get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| err("session_id is required"))?,
        )?;
        if let Some(x) = live().lock().await.remove(id) {
            let mut l = x.lock().await;
            let _ = l.child.kill().await;
            return Ok(json!({ "session_id": id, "closed": true, "profile_retained": true }));
        }
        Err(err("session is not live"))
    }
}

pub struct BrowserSessionNavigateTool;
#[async_trait]
impl Tool for BrowserSessionNavigateTool {
    fn name(&self) -> &str {
        "browser_session_navigate"
    }
    fn description(&self) -> &str {
        "Navigate a persistent browser session without losing login state"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({
                "session_id": { "type": "string" },
                "url": { "type": "string" },
            }),
            json!(["session_id", "url"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _: &ToolContext, a: Value) -> Result<Value> {
        let id = validate_session_id(
            a.get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| err("session_id is required"))?,
        )?;
        let u = a
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| err("url is required"))?;
        crate::net_guard::validate_url(u).await?;
        Ok(cdp(id, "Page.navigate", json!({ "url": u })).await?)
    }
}

pub struct BrowserSessionClickTool;
#[async_trait]
impl Tool for BrowserSessionClickTool {
    fn name(&self) -> &str {
        "browser_session_click"
    }
    fn description(&self) -> &str {
        "Click an element inside a persistent browser session"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({
                "session_id": { "type": "string" },
                "selector": { "type": "string" },
            }),
            json!(["session_id", "selector"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _: &ToolContext, a: Value) -> Result<Value> {
        let id = validate_session_id(
            a.get("session_id")
                .and_then(Value::as_str)
                .ok_or_else(|| err("session_id is required"))?,
        )?;
        let s = a
            .get("selector")
            .and_then(Value::as_str)
            .ok_or_else(|| err("selector is required"))?;
        let js = format!(
            "(()=>{{const e=document.querySelector({});if(!e)throw new Error('selector not found');e.click();return true}})()",
            serde_json::to_string(s).unwrap()
        );
        Ok(cdp(id, "Runtime.evaluate", json!({ "expression": js, "returnByValue": true }))
            .await?)
    }
}

// ── Bootstrap ───────────────────────────────────────────────────────────────

/// Load all persisted session metadata from disk into the in-memory store.
/// Call once at application startup so `resume`/`list` work after a restart.
pub fn store_init(workspace_root: &str) {
    // Fire-and-forget: load is best-effort and non-blocking.
    let root = workspace_root.to_string();
    tokio::spawn(async move {
        load_persisted_sessions(&root).await;
    });
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // session_id validation
    // -------------------------------------------------------------------------

    #[test]
    fn session_ids_reject_path_traversal_and_control_chars() {
        assert!(validate_session_id("safe_session-1").is_ok());
        assert!(validate_session_id("../escape").is_err());
        assert!(validate_session_id("nested/name").is_err());
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id(&"a".repeat(97)).is_err());
    }

    #[test]
    fn session_ids_reject_special_characters() {
        assert!(validate_session_id("has spaces").is_err());
        assert!(validate_session_id("has.dot").is_err());
        assert!(validate_session_id("has@at").is_err());
        assert!(validate_session_id("has#hash").is_err());
    }

    // -------------------------------------------------------------------------
    // Metadata roundtrip
    // -------------------------------------------------------------------------

    #[test]
    fn metadata_roundtrip_preserves_all_fields() {
        let dir = std::env::temp_dir().join("odon-browser-session-test");
        let _ = std::fs::remove_dir_all(&dir); // clean slate
        std::fs::create_dir_all(&dir).unwrap();

        let original = Meta {
            session_id: "session_test123".into(),
            profile_dir: "/tmp/chrome-profile".into(),
            port: 9222,
            pid: 12345,
            url: "https://example.com".into(),
            created_at: "2025-01-01T00:00:00Z".into(),
            last_used_at: "2025-01-02T00:00:00Z".into(),
        };

        // Serialise to disk.
        let path = dir.join("session.json");
        std::fs::write(&path, serde_json::to_vec_pretty(&original).unwrap()).unwrap();

        // Deserialise and compare.
        let bytes = std::fs::read(&path).unwrap();
        let loaded: Meta = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(loaded.session_id, original.session_id);
        assert_eq!(loaded.profile_dir, original.profile_dir);
        assert_eq!(loaded.port, original.port);
        assert_eq!(loaded.pid, original.pid);
        assert_eq!(loaded.url, original.url);
        assert_eq!(loaded.created_at, original.created_at);
        assert_eq!(loaded.last_used_at, original.last_used_at);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_persisted_sessions_handles_missing_directory() {
        // Should not panic when the sessions directory does not exist yet.
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(load_persisted_sessions("/nonexistent/path/that/does/not/exist"));
        // PERSISTED store should be empty.
        let map = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(persisted().lock());
        assert!(map.is_empty());
    }
}
