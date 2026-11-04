//! Google Maps lead search through the dewa-scraper engine.
//!
//! The engine is a remote service (dewa-scraper relays to it); nothing is scraped
//! locally. Contract, taken from `dewa-scraper/src/server.js` and
//! `dewa-scraper/src/engine.js`:
//!
//!   POST {base}/api/engine/run
//!   body   { "serial": "...", "job": { "keyword": "...", "cities": ["..."], "depth": 2 } }
//!   accept text/event-stream
//!
//! Frames are separated by a blank line and carry `data: <json>`. Lead batches
//! arrive as `{"type":"new_batch","items":[...]}` where an item has `name`,
//! `category`, `city`, `province`, `district`, `phone`, `address`, `rating`,
//! `reviews` and `mapsUrl`.
//!
//! Configuration is explicit and never invented: without an engine URL the tool
//! fails with instructions instead of returning fake rows. The engine serial is
//! optional: the engine serves an unauthenticated trial (capped per hour and per
//! city count) when no serial is sent, and rejects a serial it does not know.
//! Send one only when you own a listed licence.

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use odon_core::{OdonError, Result};
use odon_security::RiskTier;
use serde_json::{json, Value};

use crate::net_guard;
use crate::registry::{Tool, ToolContext};

/// Environment variable holding the engine base URL.
pub const ENGINE_URL_ENV: &str = "ODON_GMAPS_ENGINE_URL";
/// Environment variable holding the engine service serial (optional).
pub const ENGINE_SERIAL_ENV: &str = "ODON_GMAPS_SERIAL";

const TOOL_NAME: &str = "gmaps_search";

fn tool_error(message: impl Into<String>) -> OdonError {
    OdonError::Tool {
        tool: TOOL_NAME.to_string(),
        message: message.into(),
    }
}

#[derive(Debug)]
struct EngineConfig {
    endpoint: String,
    serial: String,
}

fn resolve_config(engine_url: Option<&str>, serial: Option<&str>) -> Result<EngineConfig> {
    let base = engine_url.unwrap_or("").trim();
    // The engine authorizes an empty serial as a capped trial, so a serial is
    // never required here; an unlisted serial would be rejected by the engine.
    let serial = serial.unwrap_or("").trim();
    if base.is_empty() {
        return Err(tool_error(format!(
            "Google Maps engine is not configured. Set {ENGINE_URL_ENV} to the engine base URL \
             (the engine serves POST /api/engine/run). Optionally set {ENGINE_SERIAL_ENV} when you \
             hold a listed licence; without it the engine runs its capped trial. Nothing is \
             emitted without an engine URL."
        )));
    }
    if !(base.starts_with("https://") || base.starts_with("http://")) {
        return Err(tool_error(format!(
            "{ENGINE_URL_ENV} must start with http:// or https://"
        )));
    }
    Ok(EngineConfig {
        endpoint: format!("{}/api/engine/run", base.trim_end_matches('/')),
        serial: serial.to_string(),
    })
}

fn build_job(keyword: &str, cities: &[String], depth: Option<u64>) -> Result<Value> {
    let keyword = keyword.trim();
    if keyword.chars().count() < 2 {
        return Err(tool_error("'keyword' must be at least 2 characters"));
    }
    let mut seen = HashSet::new();
    let cities: Vec<String> = cities
        .iter()
        .map(|city| city.trim().to_string())
        .filter(|city| city.chars().count() >= 2)
        .filter(|city| seen.insert(city.to_lowercase()))
        .collect();
    if cities.is_empty() {
        return Err(tool_error(
            "'cities' needs at least one name of 2 characters or more",
        ));
    }
    let depth = depth.unwrap_or(2).clamp(1, 5);
    Ok(json!({ "keyword": keyword, "cities": cities, "depth": depth }))
}

/// Extract the JSON payloads carried by Server-Sent Events frames.
fn parse_sse_frames(body: &str) -> Vec<Value> {
    let normalized = body.replace("\r\n", "\n");
    let mut payloads = Vec::new();
    for frame in normalized.split("\n\n") {
        for line in frame.split('\n') {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("data:") else {
                continue;
            };
            if let Ok(value) = serde_json::from_str::<Value>(rest.trim()) {
                payloads.push(value);
            }
        }
    }
    payloads
}

#[derive(Default)]
struct Collected {
    items: Vec<Value>,
    skipped: u64,
    error: Option<String>,
    finished: bool,
}

fn collect_leads(body: &str) -> Collected {
    let mut out = Collected::default();
    for payload in parse_sse_frames(body) {
        match payload.get("type").and_then(Value::as_str) {
            Some("new_batch") => {
                if let Some(items) = payload.get("items").and_then(Value::as_array) {
                    out.items.extend(items.iter().cloned());
                }
                if let Some(skipped) = payload.get("skipped").and_then(Value::as_u64) {
                    out.skipped += skipped;
                }
            }
            Some("status") => {
                if payload
                    .get("finished")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                {
                    out.finished = true;
                }
                if let Some(error) = payload.get("error").and_then(Value::as_str) {
                    if !error.is_empty() {
                        out.error = Some(error.to_string());
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[cfg(test)]
fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split('/').next()?;
    let host = authority.rsplit('@').next()?.split(':').next()?;
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

pub struct GmapsSearchTool;

#[async_trait]
impl Tool for GmapsSearchTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn description(&self) -> &str {
        "Search Google Maps business leads (name, address, phone, rating) through the configured \
         dewa-scraper engine. Requires ODON_GMAPS_ENGINE_URL; ODON_GMAPS_SERIAL is optional and \
         only needed for a listed licence (without it the engine runs a capped trial). Searches \
         spend engine quota."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "keyword": { "type": "string", "description": "Business category or name, at least 2 characters" },
                "cities": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "One or more city names"
                },
                "depth": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 7,
                    "description": "Result depth, default 2 (the engine clamps to 1-7; higher costs more engine quota)"
                }
            },
            "required": ["keyword", "cities"]
        })
    }

    fn risk_tier(&self) -> RiskTier {
        RiskTier::Tier0ReadOnly
    }

    async fn execute(&self, _ctx: &ToolContext, args: Value) -> Result<Value> {
        let engine_url = std::env::var(ENGINE_URL_ENV).ok();
        let engine_serial = std::env::var(ENGINE_SERIAL_ENV).ok();
        let config = resolve_config(engine_url.as_deref(), engine_serial.as_deref())?;

        let keyword = args
            .get("keyword")
            .and_then(Value::as_str)
            .ok_or_else(|| tool_error("Missing 'keyword' parameter"))?;
        let cities: Vec<String> = args
            .get("cities")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        let depth = args.get("depth").and_then(Value::as_u64);
        let job = build_job(keyword, &cities, depth)?;

        let target = net_guard::validate_url(&config.endpoint).await?;
        let client = target.client(reqwest::Client::builder().timeout(Duration::from_secs(120)))?;

        let body = json!({ "serial": config.serial, "job": job });
        let response = client
            .post(&config.endpoint)
            .header("Accept", "text/event-stream")
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|error| tool_error(format!("Engine request failed: {error}")))?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|error| tool_error(format!("Cannot read engine stream: {error}")))?;
        if !status.is_success() {
            let detail = text.trim();
            let detail = if detail.chars().count() > 400 {
                detail.chars().take(400).collect::<String>()
            } else {
                detail.to_string()
            };
            return Err(tool_error(format!(
                "Engine rejected the search ({status}): {detail}"
            )));
        }

        let collected = collect_leads(&text);
        if let Some(error) = &collected.error {
            return Err(tool_error(format!("Engine reported: {error}")));
        }

        Ok(json!({
            "engine_host": target.host(),
            "keyword": job["keyword"],
            "cities": job["cities"],
            "depth": job["depth"],
            "count": collected.items.len(),
            "skipped": collected.skipped,
            "finished": collected.finished,
            "items": collected.items
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_requires_url_but_not_serial() {
        let missing_url = resolve_config(None, None).unwrap_err().to_string();
        assert!(missing_url.contains(ENGINE_URL_ENV), "{missing_url}");
        // A serial is optional: the engine authorizes an empty serial as a trial.
        let trial = resolve_config(Some("https://engine.test"), None).unwrap();
        assert_eq!(trial.serial, "");
    }

    #[test]
    fn config_builds_run_endpoint_and_rejects_bad_scheme() {
        let config = resolve_config(Some("https://engine.test/"), Some(" serial ")).unwrap();
        assert_eq!(config.endpoint, "https://engine.test/api/engine/run");
        assert_eq!(config.serial, "serial");
        assert!(resolve_config(Some("engine.test"), Some("s")).is_err());
        assert!(resolve_config(Some("engine.test"), None).is_err());
    }

    #[test]
    fn job_trims_dedupes_and_clamps() {
        let job = build_job(
            "  kopi  ",
            &[
                " Batam ".into(),
                "batam".into(),
                "a".into(),
                "Jakarta".into(),
            ],
            Some(9),
        )
        .unwrap();
        assert_eq!(job["keyword"], "kopi");
        assert_eq!(job["cities"], json!(["Batam", "Jakarta"]));
        assert_eq!(job["depth"], 5);
        assert_eq!(
            build_job("kopi", &["Batam".into()], Some(0)).unwrap()["depth"],
            1
        );
        assert_eq!(
            build_job("kopi", &["Batam".into()], None).unwrap()["depth"],
            2
        );
    }

    #[test]
    fn job_rejects_short_keyword_and_empty_cities() {
        assert!(build_job("a", &["Batam".into()], None).is_err());
        assert!(build_job("kopi", &[], None).is_err());
        assert!(build_job("kopi", &["a".into()], None).is_err());
    }

    #[test]
    fn parses_engine_frames_and_collects_leads() {
        let body = concat!(
            "data: {\"type\":\"connected\"}\n\n",
            "data: {\"type\":\"log\",\"text\":\"keruk\"}\n\n",
            "data: {\"type\":\"new_batch\",\"items\":[{\"name\":\"Kopi A\",\"phone\":\"0812\",\"address\":\"Jl. Satu\",\"mapsUrl\":\"https://maps/x\"}],\"skipped\":3}\n\n",
            "broken frame without data\n\n",
            "data: {\"type\":\"new_batch\",\"items\":[{\"name\":\"Kopi B\"}]}\n\n",
            "data: {\"type\":\"status\",\"running\":false,\"finished\":true}\n\n"
        );
        let collected = collect_leads(body);
        assert_eq!(collected.items.len(), 2);
        assert_eq!(collected.items[0]["name"], "Kopi A");
        assert_eq!(collected.items[0]["phone"], "0812");
        assert_eq!(collected.skipped, 3);
        assert!(collected.finished);
        assert!(collected.error.is_none());
    }

    #[test]
    fn tolerates_crlf_and_reports_engine_error() {
        let body = "data: {\"type\":\"status\",\"error\":\"kuota habis\"}\r\n\r\n";
        let collected = collect_leads(body);
        assert_eq!(collected.error.as_deref(), Some("kuota habis"));
        assert!(collected.items.is_empty());
        assert!(!collected.finished);
    }

    #[test]
    fn ignores_malformed_json_frames() {
        assert!(parse_sse_frames("data: {not json}\n\ndata: {\"ok\":true}\n\n").len() == 1);
    }

    #[test]
    fn extracts_host_from_endpoint() {
        assert_eq!(
            host_of("https://engine.test/api/engine/run").as_deref(),
            Some("engine.test")
        );
        assert_eq!(
            host_of("http://127.0.0.1:8080/x").as_deref(),
            Some("127.0.0.1")
        );
        assert_eq!(host_of("not-a-url"), None);
    }
}
