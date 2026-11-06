use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
};

// ── Data ────────────────────────────────────────────────────────────────────

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Meta {
    pub(crate) session_id: String,
    pub(crate) profile_dir: String,
    pub(crate) port: u16,
    pub(crate) pid: u32,
    pub(crate) url: String,
    pub(crate) created_at: String,
    pub(crate) last_used_at: String,
    /// Launch with a visible window instead of `--headless`. Remembered so a
    /// resumed profile keeps the same (less automation-detectable) mode.
    #[serde(default)]
    pub(crate) headful: bool,
}

pub(crate) struct Live {
    pub(crate) meta: Meta,
    pub(crate) child: Child,
}

// Live (running) sessions keyed by session_id.
type LiveStore = Arc<Mutex<HashMap<String, Arc<Mutex<Live>>>>>;
static LIVE: std::sync::OnceLock<LiveStore> = std::sync::OnceLock::new();
pub(crate) fn live() -> &'static LiveStore {
    LIVE.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

// Persisted (metadata-only, no running process) sessions keyed by session_id.
type PersistedStore = Arc<Mutex<HashMap<String, Meta>>>;
static PERSISTED: std::sync::OnceLock<PersistedStore> = std::sync::OnceLock::new();
pub(crate) fn persisted() -> &'static PersistedStore {
    PERSISTED.get_or_init(|| Arc::new(Mutex::new(HashMap::new())))
}

// ── Path helpers ────────────────────────────────────────────────────────────

const METADATA_FILE: &str = "session.json";

/// The root directory that holds all browser-session subdirectories.
pub(crate) fn sessions_dir(workspace_root: &str) -> PathBuf {
    Path::new(workspace_root).join(".odon/browser-sessions")
}

/// Directory for a single session: `<sessions_dir>/<session_id>`.
pub(crate) fn session_dir(workspace_root: &str, session_id: &str) -> PathBuf {
    sessions_dir(workspace_root).join(session_id)
}

// ── Persistence helpers ─────────────────────────────────────────────────────

/// Scan the sessions directory and load every readable `session.json` into the
/// in-memory persisted store.  Called once from `store_init()` so that
/// `browser_session_resume` and `browser_session_list` can discover sessions
/// from a previous process lifetime.
pub(crate) async fn load_persisted_sessions(workspace_root: &str) {
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
pub(crate) fn save_meta(workspace_root: &str, meta: &Meta) -> Result<()> {
    let dir = session_dir(workspace_root, &meta.session_id);
    std::fs::create_dir_all(&dir).map_err(|e| err(e.to_string()))?;
    std::fs::write(
        dir.join(METADATA_FILE),
        serde_json::to_vec_pretty(meta).unwrap(),
    )
    .map_err(|e| err(e.to_string()))
}

/// Return the persisted `Meta` for `session_id` if present on disk, without
/// touching the in-memory stores.
pub(crate) fn find_persisted_meta(workspace_root: &str, session_id: &str) -> Result<Meta> {
    let path = session_dir(workspace_root, session_id).join(METADATA_FILE);
    let bytes = std::fs::read(&path).map_err(|_| err("session metadata not found"))?;
    serde_json::from_slice(&bytes).map_err(|e| err(e.to_string()))
}

// ── Utilities ───────────────────────────────────────────────────────────────

pub(crate) fn err(msg: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: "browser_session".into(),
        message: msg.into(),
    }
}

pub(crate) fn sid() -> String {
    // Timestamp-only ids collided when two sessions were created in the same
    // millisecond; the second `insert` replaced the first, dropping its child
    // without killing it and sharing one `--user-data-dir`. Add randomness.
    let unique = uuid::Uuid::new_v4().simple().to_string();
    format!(
        "session_{}_{}",
        chrono::Utc::now().timestamp_millis(),
        &unique[..8]
    )
}

/// Whether new sessions default to a visible window. Opt in with
/// `ODON_BROWSER_HEADFUL=1`; a per-call `headful` argument overrides it.
pub(crate) fn browser_headful_default() -> bool {
    std::env::var("ODON_BROWSER_HEADFUL")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

/// Base delay (ms) before each human-facing browser action (click/type/select).
/// Opt in with `ODON_BROWSER_ACTION_DELAY_MS` (default 0 = no added delay). A
/// per-call `delay_ms` overrides it.
pub(crate) fn browser_action_delay_default() -> u64 {
    std::env::var("ODON_BROWSER_ACTION_DELAY_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .unwrap_or(0)
        .min(60_000)
}

/// Randomise the delay by roughly ±20% so actions do not run on a fixed,
/// machine-obvious cadence.
fn jittered_delay_ms(base: u64) -> u64 {
    if base == 0 {
        return 0;
    }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.subsec_nanos() as u64)
        .unwrap_or(0);
    let spread = (base / 5).max(1);
    base.saturating_sub(spread) + (nanos % (spread * 2 + 1))
}

async fn human_pause(base: u64) {
    let ms = jittered_delay_ms(base);
    if ms > 0 {
        tokio::time::sleep(Duration::from_millis(ms)).await;
    }
}

fn action_delay(a: &Value) -> u64 {
    a.get("delay_ms")
        .and_then(Value::as_u64)
        .unwrap_or_else(browser_action_delay_default)
        .min(60_000)
}

pub(crate) fn validate_session_id(id: &str) -> Result<&str> {
    if id.is_empty()
        || id.len() > 96
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(err(
            "session_id must contain only ASCII letters, numbers, '_' or '-' and be at most 96 characters",
        ));
    }
    Ok(id)
}

pub(crate) fn browser() -> Result<PathBuf> {
    let mut p = Vec::new();
    if cfg!(windows) {
        for variable in ["PROGRAMFILES", "PROGRAMFILES(X86)"] {
            if let Ok(x) = std::env::var(variable) {
                p.push(PathBuf::from(x.clone()).join("Google/Chrome/Application/chrome.exe"));
                p.push(PathBuf::from(x).join("Microsoft/Edge/Application/msedge.exe"));
            }
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

pub(crate) async fn port() -> Result<u16> {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|e| err(e.to_string()))?;
    Ok(l.local_addr().map_err(|e| err(e.to_string()))?.port())
}

pub(crate) async fn cdp(id: &str, method: &str, params: Value) -> Result<Value> {
    let live = live()
        .lock()
        .await
        .get(id)
        .cloned()
        .ok_or_else(|| err(format!("session '{id}' is not live; resume it first")))?;
    let p = live.lock().await.meta.port;
    // A session that was just spawned has not published its page target yet,
    // so wait briefly instead of failing the panel and the agent with
    // "no active browser tab" during startup.
    let deadline_target = tokio::time::Instant::now() + Duration::from_secs(10);
    let u = loop {
        let mut found = None;
        if let Ok(r) = reqwest::get(format!("http://127.0.0.1:{p}/json")).await {
            if let Ok(ts) = r.json::<Vec<Value>>().await {
                found = ts
                    .iter()
                    .filter(|x| {
                        x.get("type")
                            .and_then(Value::as_str)
                            .map(|t| t == "page")
                            .unwrap_or(true)
                    })
                    .find_map(|x| x.get("webSocketDebuggerUrl").and_then(Value::as_str))
                    .map(str::to_string);
            }
        }
        if let Some(found) = found {
            break found;
        }
        if tokio::time::Instant::now() >= deadline_target {
            return Err(err("no active browser tab"));
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
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
    // CDP pushes unsolicited events on the same socket, so the first frame is
    // not necessarily our reply. Wait for the frame whose id matches ours and
    // surface Chrome's own error text instead of a generic fallback.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    loop {
        let left = deadline.saturating_duration_since(tokio::time::Instant::now());
        if left.is_zero() {
            return Err(err(format!("CDP timeout waiting for {method}")));
        }
        let m = tokio::time::timeout(left, ws.next())
            .await
            .map_err(|_| err(format!("CDP timeout waiting for {method}")))?
            .ok_or_else(|| err("CDP closed"))?
            .map_err(|e| err(e.to_string()))?;
        let text = match m {
            tokio_tungstenite::tungstenite::Message::Text(x) => x,
            tokio_tungstenite::tungstenite::Message::Close(_) => return Err(err("CDP closed")),
            _ => continue,
        };
        let frame: Value = serde_json::from_str(&text).map_err(|e| err(e.to_string()))?;
        if frame.get("id").and_then(Value::as_i64) != Some(1) {
            continue;
        }
        if let Some(e) = frame.get("error") {
            let msg = e
                .get("message")
                .and_then(Value::as_str)
                .map(str::to_string)
                .unwrap_or_else(|| e.to_string());
            let data = e
                .get("data")
                .and_then(Value::as_str)
                .map(|d| format!(": {d}"))
                .unwrap_or_default();
            return Err(err(format!("{method} failed: {msg}{data}")));
        }
        return Ok(frame);
    }
}

pub(crate) fn schema(props: Value, req: Value) -> Value {
    json!({ "type": "object", "properties": props, "required": req })
}

pub(crate) async fn evaluate_js(id: &str, expression: &str) -> Result<Value> {
    let response = cdp(
        id,
        "Runtime.evaluate",
        json!({ "expression": expression, "returnByValue": true, "awaitPromise": false }),
    )
    .await?;
    if let Some(exception) = response.pointer("/result/exceptionDetails") {
        let text = exception
            .pointer("/exception/description")
            .or_else(|| exception.pointer("/text"))
            .and_then(Value::as_str)
            .unwrap_or("browser evaluation failed");
        return Err(err(text.to_string()));
    }
    Ok(response
        .pointer("/result/result")
        .cloned()
        .unwrap_or(Value::Null))
}

fn visible_expression(selector: &str) -> String {
    let encoded = serde_json::to_string(selector).unwrap_or_else(|_| "\"\"".to_string());
    format!(
        "(()=>{{const e=document.querySelector({encoded});if(!e)return false;const r=e.getBoundingClientRect();const style=getComputedStyle(e);return r.width>0&&r.height>0&&style.visibility!=='hidden'&&style.display!=='none'}})()",
        encoded = encoded
    )
}

async fn wait_visible(id: &str, selector: &str, timeout_ms: u64) -> Result<bool> {
    let expression = visible_expression(selector);
    let started = std::time::Instant::now();
    while started.elapsed().as_millis() < timeout_ms as u128 {
        let value = evaluate_js(id, &expression).await?;
        if value.get("value").and_then(Value::as_bool).unwrap_or(false) {
            return Ok(true);
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    Ok(false)
}

// ── Tools ───────────────────────────────────────────────────────────────────

pub struct BrowserSessionCreateTool;
#[async_trait]
impl Tool for BrowserSessionCreateTool {
    fn name(&self) -> &str {
        "browser_session_create"
    }
    fn description(&self) -> &str {
        "Create or reuse a persistent Chromium profile session for reusable cookies and localStorage. Pass the same `name` on later runs to keep the login instead of signing in again. Set `headful` to show the browser window (less likely to be flagged as automation)."
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({
                "name": { "type": "string", "description": "Stable profile name (e.g. 'instagram'); the same name reuses the saved cookies/login" },
                "url": { "type": "string" },
                "headful": { "type": "boolean", "description": "Show the browser window instead of headless (default from ODON_BROWSER_HEADFUL)" }
            }),
            json!([]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    fn requirements(&self) -> crate::registry::ToolRequirements {
        crate::registry::ToolRequirements {
            any_binaries: vec![
                "chrome".to_string(),
                "msedge".to_string(),
                "google-chrome".to_string(),
                "chromium".to_string(),
                "microsoft-edge".to_string(),
            ],
            ..Default::default()
        }
    }
    async fn execute(&self, ctx: &ToolContext, a: Value) -> Result<Value> {
        let url = a
            .get("url")
            .and_then(Value::as_str)
            .unwrap_or("about:blank");
        validate_session_url(url).await?;
        let name = a.get("name").and_then(Value::as_str);
        let headful = a
            .get("headful")
            .and_then(Value::as_bool)
            .unwrap_or_else(browser_headful_default);
        let m = create_session_named(&ctx.workspace_root, name, url, headful).await?;
        Ok(json!(m))
    }
}

/// Sessions carry a URL into a full Chromium network stack. Only allow
/// `about:blank` or URLs that survive the same SSRF guard used elsewhere.
async fn validate_session_url(url: &str) -> Result<()> {
    if url == "about:blank" {
        return Ok(());
    }
    // The panel previews the app under development, so loopback is allowed;
    // LAN and metadata targets stay blocked by the shared guard.
    crate::net_guard::validate_url_allow_loopback(url)
        .await
        .map_err(|e| err(e.to_string()))?;
    Ok(())
}

/// Launch Chromium and wait until its DevTools endpoint actually answers.
///
/// `port()` only reserves a free port and releases it again, so two sessions
/// created at the same time can be handed the same number; the loser then
/// fails to bind and dies silently. Waiting for `/json/version` and retrying
/// turns that race into a slow-but-correct start instead of a dead session
/// that the panel shows as blank.
pub(crate) async fn spawn_browser(
    root: &std::path::Path,
    url: &str,
    headful: bool,
) -> Result<(Child, u16)> {
    let exe = browser()?;
    let mut last = String::from("browser did not start");
    for _ in 0..3 {
        let p = port().await?;
        let mut c = Command::new(&exe);
        c.args([
            "--disable-gpu",
            "--hide-scrollbars",
            "--window-size=1280,800",
            "--no-first-run",
            "--no-default-browser-check",
            "--remote-debugging-address=127.0.0.1",
            &format!("--remote-debugging-port={p}"),
            &format!("--user-data-dir={}", root.display()),
        ]);
        if headful {
            // A visible window is far less likely to be flagged as an automated
            // client, and this also clears the `navigator.webdriver` signal.
            c.arg("--disable-blink-features=AutomationControlled");
        } else {
            c.arg("--headless=new");
        }
        // Pin the validated address for the landing page. Chromium has its own
        // resolver, so an un-pinned launch would re-resolve the hostname after
        // the guard already approved a different answer.
        if let Ok(target) = crate::net_guard::validate_url_allow_loopback(url).await {
            if let Some(rules) = crate::net_guard::host_resolver_rules(&target, false) {
                c.arg(format!("--host-resolver-rules={rules}"));
            }
        }
        c.arg(url);
        // A dropped `Child` (timeout, error) must not leave Chromium running.
        c.kill_on_drop(true);
        let mut child = match c.spawn() {
            Ok(child) => child,
            Err(e) => {
                last = e.to_string();
                continue;
            }
        };
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        loop {
            if let Ok(Some(_)) = child.try_wait() {
                last = format!("browser exited during startup (port {p})");
                break;
            }
            // A DevTools endpoint that accepts the socket but never answers
            // would otherwise hang startup forever, so the deadline below could
            // never fire. Bound each probe.
            let reachable = tokio::time::timeout(
                Duration::from_secs(2),
                reqwest::get(format!("http://127.0.0.1:{p}/json/version")),
            )
            .await
            .map(|result| result.is_ok())
            .unwrap_or(false);
            if reachable {
                return Ok((child, p));
            }
            if tokio::time::Instant::now() >= deadline {
                last = format!("browser did not expose its debugging port ({p})");
                let _ = child.kill().await;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Err(err(format!("could not start a browser session: {last}")))
}

/// Derive a stable session id from a user-supplied profile name so the same
/// name always maps to the same Chromium profile directory (kept cookies /
/// logins). Falls back to a random id when the name is unusable.
fn profile_session_id(name: &str) -> String {
    let mut out = String::new();
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch.to_ascii_lowercase());
        } else if (ch.is_whitespace() || matches!(ch, '.' | '/')) && !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    let out: String = out.chars().take(80).collect();
    if out.is_empty() {
        sid()
    } else {
        format!("profile-{out}")
    }
}

/// Create (or reuse) a persistent session. A `name` maps to a stable profile
/// so an agent can log in once and reuse the cookies on later runs instead of
/// logging in from scratch every time.
pub(crate) async fn create_session_named(
    workspace_root: &str,
    name: Option<&str>,
    url: &str,
    headful: bool,
) -> Result<Meta> {
    let Some(name) = name.map(str::trim).filter(|value| !value.is_empty()) else {
        return create_session_with_headful(workspace_root, url, headful).await;
    };
    let id = profile_session_id(name);

    // Already running: hand back the same live browser.
    if let Some(existing) = live().lock().await.get(&id).cloned() {
        let mut live = existing.lock().await;
        live.meta.last_used_at = chrono::Utc::now().to_rfc3339();
        return Ok(live.meta.clone());
    }

    // Persisted from an earlier run: relaunch with the saved profile directory
    // so the login state is still there, then open the requested URL.
    if let Ok(meta) = find_persisted_meta(workspace_root, &id) {
        validate_session_url(url).await?;
        let (mut child, p) =
            spawn_browser(std::path::Path::new(&meta.profile_dir), url, headful).await?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut updated = meta.clone();
        updated.port = p;
        updated.pid = child.id().unwrap_or(0);
        updated.url = url.into();
        updated.last_used_at = now;
        updated.headful = headful;
        if let Err(error) = save_meta(workspace_root, &updated) {
            let _ = child.kill().await;
            return Err(error);
        }
        live().lock().await.insert(
            id.clone(),
            Arc::new(Mutex::new(Live {
                meta: updated.clone(),
                child,
            })),
        );
        return Ok(updated);
    }

    // Fresh named profile.
    let root = session_dir(workspace_root, &id);
    std::fs::create_dir_all(&root).map_err(|e| err(e.to_string()))?;
    let (mut child, p) = spawn_browser(&root, url, headful).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let m = Meta {
        session_id: id.clone(),
        profile_dir: root.display().to_string(),
        port: p,
        pid: child.id().unwrap_or(0),
        url: url.into(),
        created_at: now.clone(),
        last_used_at: now,
        headful,
    };
    if let Err(error) = save_meta(workspace_root, &m) {
        let _ = child.kill().await;
        return Err(error);
    }
    live().lock().await.insert(
        id.clone(),
        Arc::new(Mutex::new(Live {
            meta: m.clone(),
            child,
        })),
    );
    Ok(m)
}

/// Spawn a Chromium session and register it in the shared live store.
///
/// Shared by the `browser_session_create` tool and the daemon bridge so the
/// panel and the agent drive the exact same browser.
pub(crate) async fn create_session(workspace_root: &str, url: &str) -> Result<Meta> {
    create_session_with_headful(workspace_root, url, browser_headful_default()).await
}

pub(crate) async fn create_session_with_headful(
    workspace_root: &str,
    url: &str,
    headful: bool,
) -> Result<Meta> {
    let id = sid();
    let root = session_dir(workspace_root, &id);
    std::fs::create_dir_all(&root).map_err(|e| err(e.to_string()))?;
    let (mut child, p) = spawn_browser(&root, url, headful).await?;
    let now = chrono::Utc::now().to_rfc3339();
    let m = Meta {
        session_id: id.clone(),
        profile_dir: root.display().to_string(),
        port: p,
        pid: child.id().unwrap_or(0),
        url: url.into(),
        created_at: now.clone(),
        last_used_at: now,
        headful,
    };
    // Persist metadata so the session survives a process restart. If this
    // fails, kill the browser we just spawned instead of leaking it.
    if let Err(error) = save_meta(workspace_root, &m) {
        let _ = child.kill().await;
        return Err(error);
    }
    let replaced = live().lock().await.insert(
        id.clone(),
        Arc::new(Mutex::new(Live {
            meta: m.clone(),
            child,
        })),
    );
    // On the (now unlikely) id collision, terminate the displaced browser so it
    // does not keep running unseen.
    if let Some(old) = replaced {
        let mut old = old.lock().await;
        let _ = old.child.kill().await;
    }
    Ok(m)
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
        validate_session_url(&m.url).await?;
        let (child, p) =
            spawn_browser(std::path::Path::new(&m.profile_dir), &m.url, m.headful).await?;
        let now = chrono::Utc::now().to_rfc3339();
        let mut n = m.clone();
        n.port = p;
        n.pid = child.id().unwrap_or(0);
        n.last_used_at = now;
        // Overwrite the metadata file so the new port/pid is durable.
        save_meta(&ctx.workspace_root, &n)?;
        live().lock().await.insert(
            id.into(),
            Arc::new(Mutex::new(Live {
                meta: n.clone(),
                child,
            })),
        );
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
                "timeout_ms": { "type": "integer", "description": "Wait for document readyState (default 10000, max 60000)" }
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
        let timeout_ms = a
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(10_000)
            .clamp(1_000, 60_000);
        let navigation = cdp(id, "Page.navigate", json!({ "url": u })).await?;
        let started = std::time::Instant::now();
        let mut ready = false;
        while started.elapsed().as_millis() < timeout_ms as u128 {
            let state = evaluate_js(id, "document.readyState").await?;
            if state.get("value").and_then(Value::as_str) == Some("complete") {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        let current = evaluate_js(id, "location.href").await?;
        Ok(json!({
            "session_id": id,
            "url": current.get("value").and_then(Value::as_str).unwrap_or(u),
            "frame_id": navigation.pointer("/result/frameId"),
            "ready": ready,
            "timeout": !ready,
            "duration_ms": started.elapsed().as_millis()
        }))
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
                "timeout_ms": { "type": "integer", "description": "Wait for the element to be visible before clicking (default 5000, max 30000)" },
                "delay_ms": { "type": "integer", "description": "Human-like pause before clicking, in ms (default from ODON_BROWSER_ACTION_DELAY_MS, max 60000)" }
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
        let timeout_ms = a
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(5_000)
            .clamp(100, 30_000);
        let visible = wait_visible(id, s, timeout_ms).await?;
        if !visible {
            return Err(err(format!(
                "selector '{s}' did not become visible within {timeout_ms} ms"
            )));
        }
        // A short, jittered pause before the click reads far less like a bot
        // than clicking the instant the element appears.
        let delay = action_delay(&a);
        human_pause(delay).await;
        let encoded = serde_json::to_string(s).unwrap();
        let js = format!(
            "(()=>{{const e=document.querySelector({encoded});if(!e)throw new Error('selector not found');if(e instanceof HTMLButtonElement||e instanceof HTMLInputElement)e.disabled&&(()=>{{throw new Error('element is disabled')}})();e.scrollIntoView({{block:'center',inline:'nearest'}});e.click();return true}})()",
            encoded = encoded
        );
        let value = evaluate_js(id, &js).await?;
        Ok(json!({
            "session_id": id,
            "selector": s,
            "clicked": value.get("value").and_then(Value::as_bool).unwrap_or(false),
            "waited_ms": timeout_ms,
            "delay_ms": delay
        }))
    }
}

pub struct BrowserSessionTextTool;
#[async_trait]
impl Tool for BrowserSessionTextTool {
    fn name(&self) -> &str {
        "browser_session_text"
    }
    fn description(&self) -> &str {
        "Read visible page text and links from a live persistent browser session"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" } }),
            json!(["session_id"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("session_id is required"))?;
        let js = "(()=>{const text=document.body?document.body.innerText:'';const links=[...document.links].slice(0,100).map(a=>({text:(a.innerText||'').trim().slice(0,200),href:a.href})).filter(x=>x.href);return JSON.stringify({title:document.title,url:location.href,text:text.slice(0,50000),links})})()";
        let value = cdp(
            id,
            "Runtime.evaluate",
            json!({ "expression": js, "returnByValue": true }),
        )
        .await?;
        let raw = value
            .pointer("/result/result/value")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let parsed: Value = serde_json::from_str(raw)
            .map_err(|e| err(format!("failed to parse page data: {e}")))?;
        Ok(parsed)
    }
}

pub struct BrowserSessionTypeTool;
#[async_trait]
impl Tool for BrowserSessionTypeTool {
    fn name(&self) -> &str {
        "browser_session_type"
    }
    fn description(&self) -> &str {
        "Set the value of an input, textarea, or contenteditable element in a live browser session"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" }, "selector": { "type": "string" }, "text": { "type": "string" }, "delay_ms": { "type": "integer", "description": "Human-like pause before typing, in ms (default from ODON_BROWSER_ACTION_DELAY_MS)" } }),
            json!(["session_id", "selector", "text"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("session_id is required"))?;
        let selector = args
            .get("selector")
            .and_then(Value::as_str)
            .ok_or_else(|| err("selector is required"))?;
        let text = args.get("text").and_then(Value::as_str).unwrap_or("");
        let delay = action_delay(&args);
        human_pause(delay).await;
        let js = format!(
            "(()=>{{const e=document.querySelector({});if(!e)throw new Error('selector not found');if(e instanceof HTMLInputElement||e instanceof HTMLTextAreaElement){{e.focus();e.value={};e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));}}else{{e.focus();e.textContent={};e.dispatchEvent(new InputEvent('input',{{bubbles:true}}));}}return true}})()",
            serde_json::to_string(selector).unwrap(),
            serde_json::to_string(text).unwrap(),
            serde_json::to_string(text).unwrap()
        );
        let _ = cdp(
            id,
            "Runtime.evaluate",
            json!({ "expression": js, "returnByValue": true }),
        )
        .await?;
        Ok(json!({
            "session_id": id,
            "selector": selector,
            "typed": text.chars().count(),
            "delay_ms": delay
        }))
    }
}

pub struct BrowserSessionWaitTool;
#[async_trait]
impl Tool for BrowserSessionWaitTool {
    fn name(&self) -> &str {
        "browser_session_wait"
    }
    fn description(&self) -> &str {
        "Wait until an element exists and is visible in a live browser session"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" }, "selector": { "type": "string" }, "timeout_ms": { "type": "integer", "description": "Maximum wait time (default 5000, max 30000)" } }),
            json!(["session_id", "selector"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("session_id is required"))?;
        let selector = args
            .get("selector")
            .and_then(Value::as_str)
            .ok_or_else(|| err("selector is required"))?;
        let timeout_ms = args
            .get("timeout_ms")
            .and_then(Value::as_u64)
            .unwrap_or(5_000)
            .clamp(100, 30_000);
        let js = format!(
            "(()=>{{const e=document.querySelector({});if(!e)return false;const r=e.getBoundingClientRect();const style=getComputedStyle(e);return r.width>0&&r.height>0&&style.visibility!=='hidden'&&style.display!=='none'}})()",
            serde_json::to_string(selector).unwrap()
        );
        let started = std::time::Instant::now();
        while started.elapsed().as_millis() < timeout_ms as u128 {
            let value = cdp(
                id,
                "Runtime.evaluate",
                json!({ "expression": js, "returnByValue": true }),
            )
            .await?;
            if value
                .pointer("/result/result/value")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                return Ok(json!({
                    "session_id": id,
                    "selector": selector,
                    "visible": true,
                    "elapsed_ms": started.elapsed().as_millis()
                }));
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        Ok(json!({
            "session_id": id,
            "selector": selector,
            "visible": false,
            "elapsed_ms": started.elapsed().as_millis(),
            "timeout": true
        }))
    }
}

pub struct BrowserSessionScreenshotTool;
#[async_trait]
impl Tool for BrowserSessionScreenshotTool {
    fn name(&self) -> &str {
        "browser_session_screenshot"
    }
    fn description(&self) -> &str {
        "Capture a PNG screenshot from a live browser session and save it in the workspace"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" }, "filename": { "type": "string", "description": "Workspace-relative PNG path (default generated/browser/<session>.png)" } }),
            json!(["session_id"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("session_id is required"))?;
        let fallback = format!("generated/browser/{id}.png");
        let requested = args
            .get("filename")
            .and_then(Value::as_str)
            .unwrap_or(&fallback);
        if !requested.to_ascii_lowercase().ends_with(".png") {
            return Err(err("filename must end with .png"));
        }
        let path = crate::workspace_path::resolve_write(&ctx.workspace_root, requested)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| err(e.to_string()))?;
        }
        let value = cdp(
            id,
            "Page.captureScreenshot",
            json!({ "format": "png", "captureBeyondViewport": true }),
        )
        .await?;
        let data = value
            .pointer("/result/data")
            .and_then(Value::as_str)
            .ok_or_else(|| err("browser did not return screenshot data"))?;
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| err(format!("invalid screenshot payload: {e}")))?;
        std::fs::write(&path, &bytes).map_err(|e| err(e.to_string()))?;
        let relative = path
            .strip_prefix(std::path::Path::new(&ctx.workspace_root))
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        Ok(json!({
            "session_id": id,
            "path": relative,
            "absolute_path": path,
            "bytes": bytes.len(),
            "status": "saved"
        }))
    }
}

pub struct BrowserSessionSelectTool;
#[async_trait]
impl Tool for BrowserSessionSelectTool {
    fn name(&self) -> &str {
        "browser_session_select"
    }
    fn description(&self) -> &str {
        "Select an option in a select element within a live browser session"
    }
    fn parameters_schema(&self) -> Value {
        schema(
            json!({ "session_id": { "type": "string" }, "selector": { "type": "string" }, "value": { "type": "string" }, "delay_ms": { "type": "integer", "description": "Human-like pause before selecting, in ms (default from ODON_BROWSER_ACTION_DELAY_MS)" } }),
            json!(["session_id", "selector", "value"]),
        )
    }
    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier1WorkspaceMutating
    }
    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let id = args
            .get("session_id")
            .and_then(Value::as_str)
            .ok_or_else(|| err("session_id is required"))?;
        let selector = args
            .get("selector")
            .and_then(Value::as_str)
            .ok_or_else(|| err("selector is required"))?;
        let value = args
            .get("value")
            .and_then(Value::as_str)
            .ok_or_else(|| err("value is required"))?;
        let delay = action_delay(&args);
        human_pause(delay).await;
        let js = format!(
            "(()=>{{const e=document.querySelector({});if(!e)throw new Error('selector not found');if(!(e instanceof HTMLSelectElement))throw new Error('selector is not a select element');e.value={};e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return e.value}})()",
            serde_json::to_string(selector).unwrap(),
            serde_json::to_string(value).unwrap()
        );
        let result = cdp(
            id,
            "Runtime.evaluate",
            json!({ "expression": js, "returnByValue": true }),
        )
        .await?;
        let selected = result
            .pointer("/result/result/value")
            .and_then(Value::as_str)
            .unwrap_or(value);
        Ok(json!({
            "session_id": id,
            "selector": selector,
            "value": selected,
            "delay_ms": delay,
            "status": "selected"
        }))
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

/// Count live and persisted browser sessions so capability status reflects the
/// actual browser session runtime rather than a hardcoded false value.
pub async fn session_status_counts(workspace_root: &str) -> (usize, usize) {
    let persisted_count = {
        let map = persisted().lock().await;
        map.len()
    };
    let live_count = {
        let map = live().lock().await;
        map.len()
    };
    if persisted_count > 0 || live_count > 0 {
        (live_count, persisted_count)
    } else {
        // Cover cases where startup loading has not completed yet.
        let dir = sessions_dir(workspace_root);
        let count = std::fs::read_dir(&dir)
            .map(|entries| {
                entries
                    .filter_map(|entry| entry.ok())
                    .filter(|entry| entry.path().join(METADATA_FILE).is_file())
                    .count()
            })
            .unwrap_or(0);
        (live_count, count)
    }
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
            headful: false,
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
        rt.block_on(load_persisted_sessions(
            "/nonexistent/path/that/does/not/exist",
        ));
        // PERSISTED store should be empty.
        let map = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(persisted().lock());
        assert!(map.is_empty());
    }
}
