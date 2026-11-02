use crate::registry::{Tool, ToolContext};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use odon_core::error::{OdonError, Result};
use odon_security::RiskTier;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, path::{Path, PathBuf}, sync::Arc, time::Duration};
use tokio::{process::{Child, Command}, sync::Mutex};

#[derive(Clone, Serialize, Deserialize)] struct Meta { session_id: String, profile_dir: String, port: u16, pid: u32, url: String, created_at: String, last_used_at: String }
struct Live { meta: Meta, child: Child }
type Store = Arc<Mutex<HashMap<String, Arc<Mutex<Live>>>>>;
static STORE: std::sync::OnceLock<Store> = std::sync::OnceLock::new();
fn store() -> &'static Store { STORE.get_or_init(|| Arc::new(Mutex::new(HashMap::new()))) }
fn err(msg: impl Into<String>) -> OdonError { OdonError::Tool { tool: "browser_session".into(), message: msg.into() } }
fn sid() -> String { format!("session_{}", chrono::Utc::now().timestamp_millis()) }
fn validate_session_id(id: &str) -> Result<&str> {
    if id.is_empty() || id.len() > 96 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
        return Err(err("session_id must contain only ASCII letters, numbers, '_' or '-' and be at most 96 characters"));
    }
    Ok(id)
}
fn browser() -> Result<PathBuf> { let mut p=Vec::new(); if cfg!(windows) { if let Ok(x)=std::env::var("PROGRAMFILES") { p.push(PathBuf::from(x.clone()).join("Google/Chrome/Application/chrome.exe")); p.push(PathBuf::from(x).join("Microsoft/Edge/Application/msedge.exe")); } if let Ok(x)=std::env::var("LOCALAPPDATA") { p.push(PathBuf::from(x.clone()).join("Google/Chrome/Application/chrome.exe")); p.push(PathBuf::from(x).join("Microsoft/Edge/Application/msedge.exe")); } } else { p.extend([PathBuf::from("/usr/bin/google-chrome"),PathBuf::from("/usr/bin/chromium"),PathBuf::from("/usr/bin/microsoft-edge")]); } p.into_iter().find(|x| x.is_file()).ok_or_else(|| err("Chrome/Chromium/Edge executable not found")) }
async fn port() -> Result<u16> { let l=tokio::net::TcpListener::bind("127.0.0.1:0").await.map_err(|e|err(e.to_string()))?; Ok(l.local_addr().map_err(|e|err(e.to_string()))?.port()) }
async fn cdp(id:&str, method:&str, params:Value)->Result<Value>{ let live=store().lock().await.get(id).cloned().ok_or_else(||err(format!("session '{id}' is not live; resume it first")))?; let p=live.lock().await.meta.port; let ts:Vec<Value>=reqwest::get(format!("http://127.0.0.1:{p}/json")).await.map_err(|e|err(e.to_string()))?.json().await.map_err(|e|err(e.to_string()))?; let u=ts.iter().find_map(|x|x.get("webSocketDebuggerUrl").and_then(Value::as_str)).ok_or_else(||err("no active browser tab"))?; let (mut ws,_)=tokio_tungstenite::connect_async(u).await.map_err(|e|err(e.to_string()))?; ws.send(tokio_tungstenite::tungstenite::Message::Text(json!({"id":1,"method":method,"params":params}).to_string().into())).await.map_err(|e|err(e.to_string()))?; let m=tokio::time::timeout(Duration::from_secs(20),ws.next()).await.map_err(|_|err("CDP timeout"))?.ok_or_else(||err("CDP closed"))?.map_err(|e|err(e.to_string()))?; match m { tokio_tungstenite::tungstenite::Message::Text(x)=>serde_json::from_str(&x).map_err(|e|err(e.to_string())), _=>Err(err("invalid CDP response")) } }
fn schema(props:Value, req:Value)->Value{json!({"type":"object","properties":props,"required":req})}

pub struct BrowserSessionCreateTool;
#[async_trait] impl Tool for BrowserSessionCreateTool { fn name(&self)->&str{"browser_session_create"} fn description(&self)->&str{"Create a persistent Chromium profile session for reusable cookies and localStorage"} fn parameters_schema(&self)->Value{schema(json!({"name":{"type":"string"},"url":{"type":"string"}}),json!([]))} fn risk_tier(&self)->RiskTier{RiskTier::Tier1WorkspaceMutating} async fn execute(&self,ctx:&ToolContext,a:Value)->Result<Value>{ let id=sid(); let root=Path::new(&ctx.workspace_root).join(".odon/browser-sessions").join(&id); std::fs::create_dir_all(&root).map_err(|e|err(e.to_string()))?; let p=port().await?; let url=a.get("url").and_then(Value::as_str).unwrap_or("about:blank"); let mut c=Command::new(browser()?); c.args(["--headless=new","--disable-gpu","--no-first-run","--no-default-browser-check",&format!("--remote-debugging-port={p}"),&format!("--user-data-dir={}",root.display()),url]); let child=c.spawn().map_err(|e|err(e.to_string()))?; let now=chrono::Utc::now().to_rfc3339(); let m=Meta{session_id:id.clone(),profile_dir:root.display().to_string(),port:p,pid:child.id().unwrap_or(0),url:url.into(),created_at:now.clone(),last_used_at:now}; std::fs::write(root.join("session.json"),serde_json::to_vec_pretty(&m).unwrap()).map_err(|e|err(e.to_string()))?; store().lock().await.insert(id.clone(),Arc::new(Mutex::new(Live{meta:m.clone(),child}))); Ok(json!(m)) } }

pub struct BrowserSessionListTool;
#[async_trait] impl Tool for BrowserSessionListTool { fn name(&self)->&str{"browser_session_list"} fn description(&self)->&str{"List live persistent browser sessions"} fn parameters_schema(&self)->Value{schema(json!({}),json!([]))} fn risk_tier(&self)->RiskTier{RiskTier::Tier0ReadOnly} async fn execute(&self,_:&ToolContext,_:Value)->Result<Value>{ let s=store().lock().await; let mut v=Vec::new(); for x in s.values(){v.push(x.lock().await.meta.clone());} Ok(json!(v)) } }

pub struct BrowserSessionResumeTool;
#[async_trait] impl Tool for BrowserSessionResumeTool { fn name(&self)->&str{"browser_session_resume"} fn description(&self)->&str{"Resume a persistent browser profile by session id"} fn parameters_schema(&self)->Value{schema(json!({"session_id":{"type":"string"}}),json!(["session_id"]))} fn risk_tier(&self)->RiskTier{RiskTier::Tier1WorkspaceMutating} async fn execute(&self,ctx:&ToolContext,a:Value)->Result<Value>{ let id=validate_session_id(a.get("session_id").and_then(Value::as_str).ok_or_else(||err("session_id is required"))?)?; let path=Path::new(&ctx.workspace_root).join(".odon/browser-sessions").join(id); let b=std::fs::read(path.join("session.json")).map_err(|_|err("session metadata not found"))?; let m:Meta=serde_json::from_slice(&b).map_err(|e|err(e.to_string()))?; let p=port().await?; let mut c=Command::new(browser()?); c.args(["--headless=new","--disable-gpu","--no-first-run",&format!("--remote-debugging-port={p}"),&format!("--user-data-dir={}",m.profile_dir),"about:blank"]); let child=c.spawn().map_err(|e|err(e.to_string()))?; let mut n=m.clone(); n.port=p; n.pid=child.id().unwrap_or(0); n.last_used_at=chrono::Utc::now().to_rfc3339(); store().lock().await.insert(id.into(),Arc::new(Mutex::new(Live{meta:n.clone(),child}))); Ok(json!(n)) } }

pub struct BrowserSessionCloseTool;
#[async_trait] impl Tool for BrowserSessionCloseTool { fn name(&self)->&str{"browser_session_close"} fn description(&self)->&str{"Close a browser session while retaining its persistent profile"} fn parameters_schema(&self)->Value{schema(json!({"session_id":{"type":"string"}}),json!(["session_id"]))} fn risk_tier(&self)->RiskTier{RiskTier::Tier1WorkspaceMutating} async fn execute(&self,_:&ToolContext,a:Value)->Result<Value>{ let id=validate_session_id(a.get("session_id").and_then(Value::as_str).ok_or_else(||err("session_id is required"))?)?; if let Some(x)=store().lock().await.remove(id){let mut l=x.lock().await; let _=l.child.kill().await; return Ok(json!({"session_id":id,"closed":true,"profile_retained":true}));} Err(err("session is not live")) } }

pub struct BrowserSessionNavigateTool;
#[async_trait] impl Tool for BrowserSessionNavigateTool { fn name(&self)->&str{"browser_session_navigate"} fn description(&self)->&str{"Navigate a persistent browser session without losing login state"} fn parameters_schema(&self)->Value{schema(json!({"session_id":{"type":"string"},"url":{"type":"string"}}),json!(["session_id","url"]))} fn risk_tier(&self)->RiskTier{RiskTier::Tier1WorkspaceMutating} async fn execute(&self,_:&ToolContext,a:Value)->Result<Value>{let id=validate_session_id(a.get("session_id").and_then(Value::as_str).ok_or_else(||err("session_id is required"))?)?;let u=a.get("url").and_then(Value::as_str).ok_or_else(||err("url is required"))?;crate::net_guard::validate_url(u).await?;Ok(cdp(id,"Page.navigate",json!({"url":u})).await?)} }

#[cfg(test)]
mod tests {
    use super::validate_session_id;

    #[test]
    fn session_ids_reject_path_traversal_and_control_chars() {
        assert!(validate_session_id("safe_session-1").is_ok());
        assert!(validate_session_id("../escape").is_err());
        assert!(validate_session_id("nested/name").is_err());
        assert!(validate_session_id("").is_err());
        assert!(validate_session_id(&"a".repeat(97)).is_err());
    }
}

pub struct BrowserSessionClickTool;
#[async_trait] impl Tool for BrowserSessionClickTool { fn name(&self)->&str{"browser_session_click"} fn description(&self)->&str{"Click an element inside a persistent browser session"} fn parameters_schema(&self)->Value{schema(json!({"session_id":{"type":"string"},"selector":{"type":"string"}}),json!(["session_id","selector"]))} fn risk_tier(&self)->RiskTier{RiskTier::Tier1WorkspaceMutating} async fn execute(&self,_:&ToolContext,a:Value)->Result<Value>{let id=validate_session_id(a.get("session_id").and_then(Value::as_str).ok_or_else(||err("session_id is required"))?)?;let s=a.get("selector").and_then(Value::as_str).ok_or_else(||err("selector is required"))?;let js=format!("(()=>{{const e=document.querySelector({});if(!e)throw new Error('selector not found');e.click();return true}})()",serde_json::to_string(s).unwrap());Ok(cdp(id,"Runtime.evaluate",json!({"expression":js,"returnByValue":true})).await?)} }
