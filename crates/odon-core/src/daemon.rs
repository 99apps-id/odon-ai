//! Client-side discovery of the local ODON daemon.
//!
//! Every local caller (the `odon` CLI, the daemon-backed tools in
//! `odon-tools`, and anything else that talks to `odond` over HTTP) has to
//! answer the same two questions: *where* is the daemon, and *which token*
//! does it expect. Answering them in one place keeps the three call sites from
//! drifting apart.
//!
//! The daemon enforces a bearer token on every `/api/` request with no
//! fall-open mode: either the operator exported `ODON_API_TOKEN`, or the
//! daemon minted a per-session loopback token and published it to an
//! owner-only file. Both sources are resolved here, in that order, so a client
//! works whether the token is long-lived and operator-managed or ephemeral and
//! written by the daemon at startup.

use std::path::{Path, PathBuf};

/// Environment variable that overrides the daemon base URL.
pub const ENV_DAEMON_URL: &str = "ODON_DAEMON_URL";
/// Environment variable that carries an operator-supplied API token.
pub const ENV_API_TOKEN: &str = "ODON_API_TOKEN";

const DEFAULT_DAEMON_URL: &str = "http://127.0.0.1:4096";

/// Base URL of the daemon, without a trailing slash.
///
/// Falls back to the default loopback port so a locally launched daemon is
/// reachable without any configuration. A blank override is ignored rather
/// than producing an unusable empty base.
pub fn base_url() -> String {
    std::env::var(ENV_DAEMON_URL)
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_DAEMON_URL.to_string())
}

fn odon_state_root() -> PathBuf {
    std::env::var_os("ODON_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Location of the owner-only file holding the per-session loopback token.
///
/// Must match the path `odond` writes at startup, otherwise a loopback client
/// silently sends no credentials and every request fails with 401.
pub fn token_file_path() -> PathBuf {
    odon_state_root().join(".odon").join("api-token")
}

fn normalize_token(raw: Option<String>) -> Option<String> {
    raw.map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

/// The API token a client should present, if any is discoverable.
///
/// `ODON_API_TOKEN` wins so an operator-exposed daemon keeps working even when
/// a stale loopback file exists; otherwise the daemon-published file is used.
pub fn api_token() -> Option<String> {
    resolve_token(std::env::var(ENV_API_TOKEN).ok(), &token_file_path())
}

/// Testable core of [`api_token`]: pick the env token, else the file token.
pub fn resolve_token(env_value: Option<String>, token_file: &Path) -> Option<String> {
    normalize_token(env_value).or_else(|| normalize_token(std::fs::read_to_string(token_file).ok()))
}

/// Actionable message for a `401`, naming the credential sources we tried.
pub fn unauthorized_hint(url: &str) -> String {
    format!(
        "{url} rejected the request with HTTP 401. Set {ENV_API_TOKEN}, or start the daemon on \
         loopback so it publishes its session token to {}.",
        token_file_path().display()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_token_file(name: &str, contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "odon-core-daemon-test-{}-{name}",
            uuid::Uuid::new_v4().simple()
        ));
        std::fs::write(&path, contents).expect("write token file");
        path
    }

    #[test]
    fn env_token_wins_over_the_published_file() {
        let path = temp_token_file("env-wins", "from-file\n");
        let token = resolve_token(Some("  from-env  ".to_string()), &path);
        assert_eq!(token.as_deref(), Some("from-env"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn published_file_is_used_when_the_env_is_absent_or_blank() {
        let path = temp_token_file("file-fallback", "  file-token\n");
        assert_eq!(
            resolve_token(None, &path).as_deref(),
            Some("file-token"),
            "a blank env must fall back to the daemon-published token"
        );
        assert_eq!(
            resolve_token(Some("   ".to_string()), &path).as_deref(),
            Some("file-token"),
            "whitespace is not a token"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_missing_file_yields_no_token_instead_of_an_empty_header() {
        let path = std::env::temp_dir().join("odon-core-daemon-test-absent");
        let _ = std::fs::remove_file(&path);
        assert_eq!(resolve_token(None, &path), None);
    }

    #[test]
    fn token_path_lives_in_the_odont_state_directory() {
        let path = token_file_path();
        assert!(path.ends_with("api-token"), "got {}", path.display());
        assert!(
            path.components().any(|part| part.as_os_str() == ".odon"),
            "got {}",
            path.display()
        );
    }

    #[test]
    fn unauthorized_hint_names_both_credential_sources() {
        let hint = unauthorized_hint("http://127.0.0.1:4096/api/status");
        assert!(hint.contains("401"));
        assert!(hint.contains(ENV_API_TOKEN));
        assert!(hint.contains("api-token"));
    }
}
