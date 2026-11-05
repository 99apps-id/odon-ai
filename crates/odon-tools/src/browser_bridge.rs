// Daemon-facing bridge over the persistent browser sessions.
//
// The desktop panel and the agent tools must drive the same Chromium instance,
// otherwise the panel is decoration: the agent would click in a browser nobody
// can see and the user would drive a second, disconnected one. Every function
// here operates on the store in `browser_session`, so both sides share it.

use crate::browser_session::{
    browser, cdp, create_session, err, evaluate_js, find_persisted_meta, live,
    load_persisted_sessions, persisted, save_meta, validate_session_id,
};
use odon_core::error::Result;
use serde_json::{json, Value};

/// One row per known session, live or merely persisted on disk.
pub async fn list_sessions(workspace_root: &str) -> Result<Value> {
    load_persisted_sessions(workspace_root).await;
    let mut sessions: Vec<Value> = Vec::new();
    {
        let live_map = live().lock().await;
        for (id, handle) in live_map.iter() {
            let live = handle.lock().await;
            sessions.push(json!({
                "session_id": id,
                "url": live.meta.url,
                "port": live.meta.port,
                "pid": live.meta.pid,
                "created_at": live.meta.created_at,
                "live": true
            }));
        }
    }
    // Sessions from a previous daemon lifetime only exist on disk. Without this
    // the panel cannot see or resume them even though `browser_session_list`
    // (the tool) can.
    let persisted_map = persisted().lock().await;
    for (id, meta) in persisted_map.iter() {
        if sessions
            .iter()
            .any(|session| session["session_id"].as_str() == Some(id.as_str()))
        {
            continue;
        }
        sessions.push(json!({
            "session_id": id,
            "url": meta.url,
            "port": meta.port,
            "pid": meta.pid,
            "created_at": meta.created_at,
            "live": false
        }));
    }
    drop(persisted_map);
    sessions.sort_by(|a, b| {
        b["created_at"]
            .as_str()
            .unwrap_or("")
            .cmp(a["created_at"].as_str().unwrap_or(""))
    });
    Ok(json!({ "sessions": sessions, "browser": browser().is_ok() }))
}

/// Create a session for the panel and return its metadata.
pub async fn open_session(workspace_root: &str, url: &str) -> Result<Value> {
    let url = if url.trim().is_empty() {
        "about:blank"
    } else {
        url.trim()
    };
    if !url.starts_with("about:") {
        crate::net_guard::validate_url_allow_loopback(url).await?;
    }
    let meta = create_session(workspace_root, url).await?;
    Ok(json!({
        "session_id": meta.session_id,
        "url": meta.url,
        "created_at": meta.created_at,
        "live": true
    }))
}

async fn require_live(id: &str) -> Result<()> {
    let known = live().lock().await.contains_key(id);
    if known {
        Ok(())
    } else {
        Err(err(format!(
            "session '{id}' is not live; resume or create one first"
        )))
    }
}

/// Navigate an existing session and wait for the document to settle.
pub async fn navigate(workspace_root: &str, id: &str, url: &str, timeout_ms: u64) -> Result<Value> {
    let id = validate_session_id(id)?;
    crate::net_guard::validate_url_allow_loopback(url).await?;
    require_live(id).await?;
    cdp(id, "Page.navigate", json!({ "url": url })).await?;
    let started = std::time::Instant::now();
    let timeout_ms = timeout_ms.clamp(1_000, 30_000);
    let mut ready = false;
    while started.elapsed().as_millis() < timeout_ms as u128 {
        let state = evaluate_js(id, "document.readyState").await?;
        if state.get("value").and_then(Value::as_str) == Some("complete") {
            ready = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    }
    let current = evaluate_js(id, "location.href").await?;
    let title = evaluate_js(id, "document.title").await?;
    let final_url = current
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or(url)
        .to_string();
    touch(workspace_root, id, &final_url).await;
    Ok(json!({
        "session_id": id,
        "url": final_url,
        "title": title.get("value").and_then(Value::as_str).unwrap_or(""),
        "ready": ready
    }))
}

/// Keep the persisted URL in sync so a resumed session reopens where it was.
async fn touch(workspace_root: &str, id: &str, url: &str) {
    let mut map = live().lock().await;
    if let Some(handle) = map.get_mut(id) {
        let mut guard = handle.lock().await;
        guard.meta.url = url.to_string();
        guard.meta.last_used_at = chrono::Utc::now().to_rfc3339();
        let meta = guard.meta.clone();
        drop(guard);
        let _ = save_meta(workspace_root, &meta);
    }
}

/// Visible page state: url, title, text and links the agent can reason about.
pub async fn page_state(id: &str) -> Result<Value> {
    let id = validate_session_id(id)?;
    require_live(id).await?;
    let url = evaluate_js(id, "location.href").await?;
    let title = evaluate_js(id, "document.title").await?;
    let text = evaluate_js(
        id,
        "document.body ? document.body.innerText.slice(0, 20000) : ''",
    )
    .await?;
    let links = evaluate_js(
        id,
        "(Array.from(document.querySelectorAll('a[href]')).slice(0,200).map(a=>a.href))",
    )
    .await?;
    Ok(json!({
        "session_id": id,
        "url": url.get("value").and_then(Value::as_str).unwrap_or(""),
        "title": title.get("value").and_then(Value::as_str).unwrap_or(""),
        "text": text.get("value").and_then(Value::as_str).unwrap_or(""),
        "links": links.get("value").cloned().unwrap_or(Value::Array(vec![]))
    }))
}

/// Base64 PNG of the current viewport, for the panel's live view.
pub async fn screenshot(id: &str) -> Result<String> {
    let id = validate_session_id(id)?;
    require_live(id).await?;
    let shot = cdp(
        id,
        "Page.captureScreenshot",
        json!({ "format": "png", "captureBeyondViewport": false }),
    )
    .await?;
    shot.pointer("/result/data")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
        .ok_or_else(|| err("browser did not return a screenshot"))
}

/// Click at viewport coordinates (CSS pixels, not device pixels).
pub async fn click(id: &str, x: f64, y: f64) -> Result<()> {
    let id = validate_session_id(id)?;
    require_live(id).await?;
    for kind in ["mousePressed", "mouseReleased"] {
        cdp(
            id,
            "Input.dispatchMouseEvent",
            json!({
                "type": kind,
                "x": x,
                "y": y,
                "button": "left",
                "clickCount": 1
            }),
        )
        .await?;
    }
    Ok(())
}

/// Type text into whatever currently holds focus.
pub async fn type_text(id: &str, text: &str) -> Result<()> {
    let id = validate_session_id(id)?;
    require_live(id).await?;
    cdp(id, "Input.insertText", json!({ "text": text })).await?;
    Ok(())
}

/// Close a session and drop its process.
pub async fn close(id: &str) -> Result<()> {
    let id = validate_session_id(id)?;
    let handle = live().lock().await.remove(id);
    match handle {
        Some(h) => {
            let mut guard = h.lock().await;
            let _ = guard.child.kill().await;
            Ok(())
        }
        None => Ok(()),
    }
}

/// Persisted metadata for a session that is no longer live.
pub fn persisted_meta(workspace_root: &str, id: &str) -> Result<Value> {
    let id = validate_session_id(id)?;
    let meta = find_persisted_meta(workspace_root, id)?;
    Ok(json!({
        "session_id": meta.session_id,
        "url": meta.url,
        "created_at": meta.created_at,
        "live": false
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser_session::create_session;

    fn workspace() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("odon-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// The panel and the agent must drive one browser, not two. `create_session`
    /// is exactly what `browser_session_create` calls, so a session it makes
    /// must be visible through the bridge the panel reads.
    ///
    /// Opt-in: launching a real browser is slow and noisy, so it only runs with
    /// `ODON_BROWSER_TEST=1` rather than on every `cargo test`.
    #[tokio::test]
    async fn bridge_lists_the_session_created_for_the_agent() {
        if std::env::var("ODON_BROWSER_TEST").ok().as_deref() != Some("1") || browser().is_err() {
            return; // Not requested, or no Chromium on this machine.
        }
        let root = workspace();
        let root_s = root.display().to_string();
        let meta = create_session(&root_s, "about:blank").await.unwrap();

        let listed = list_sessions(&root_s).await.unwrap();
        let ids: Vec<String> = listed["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|s| s["session_id"].as_str().map(str::to_string))
            .collect();
        assert!(
            ids.contains(&meta.session_id),
            "bridge did not expose the agent's session: {ids:?}"
        );

        // A real screenshot round-trip proves the bridge talks to the same CDP tab.
        let started = std::time::Instant::now();
        let mut png = String::new();
        while started.elapsed() < std::time::Duration::from_secs(8) {
            if let Ok(data) = screenshot(&meta.session_id).await {
                if !data.is_empty() {
                    png = data;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
        }
        assert!(
            png.starts_with("iVBORw0KGgo"),
            "expected PNG data, got {} bytes",
            png.len()
        );

        close(&meta.session_id).await.unwrap();
        let after = list_sessions(&root_s).await.unwrap();
        let still_live = after["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s["session_id"].as_str() == Some(meta.session_id.as_str()));
        assert!(!still_live, "closed session still listed as live");
    }

    /// The panel shows a live frame, so a screenshot of a *rendered* page must
    /// come back as a real PNG. An empty payload or a stalled CDP read would
    /// leave the panel blank, which this proves cannot happen silently.
    #[tokio::test]
    async fn screenshot_of_a_rendered_page_is_a_png() {
        if std::env::var("ODON_BROWSER_TEST").ok().as_deref() != Some("1") || browser().is_err() {
            return;
        }
        let root = workspace();
        let root_s = root.display().to_string();
        let page = "data:text/html,<h1 style='font:48px sans-serif'>odon live</h1>";
        let meta = create_session(&root_s, page).await.unwrap();

        let started = std::time::Instant::now();
        let mut png = String::new();
        while started.elapsed() < std::time::Duration::from_secs(10) {
            if let Ok(data) = screenshot(&meta.session_id).await {
                if !data.is_empty() {
                    png = data;
                    break;
                }
            }
            tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        }
        assert!(
            png.starts_with("iVBORw0KGgo"),
            "expected PNG data, got {} bytes",
            png.len()
        );

        // The agent must also be able to read the same page it screenshots.
        let text = page_state(&meta.session_id).await.unwrap();
        assert!(
            text["text"].as_str().unwrap_or("").contains("odon live"),
            "agent could not read the rendered page: {text}"
        );

        close(&meta.session_id).await.unwrap();
    }

    /// Unknown ids must be rejected before any CDP call is attempted.
    #[tokio::test]
    async fn unknown_session_is_rejected() {
        let root = workspace();
        let err = screenshot("session_does_not_exist").await.unwrap_err();
        assert!(
            err.to_string().contains("not live"),
            "unexpected error: {err}"
        );
        let bad = navigate(
            &root.display().to_string(),
            "../etc",
            "https://example.com",
            5_000,
        )
        .await
        .unwrap_err();
        assert!(
            bad.to_string().contains("session_id"),
            "unexpected error: {bad}"
        );
    }
}
