use tracing::{info, warn};

use crate::oauth::{OAuthProvider, OAuthService, OAuthToken};

#[cfg(target_os = "windows")]
pub fn read_credential(target: &str) -> Option<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    #[repr(C)]
    #[allow(clippy::upper_case_acronyms)]
    struct CREDENTIALW {
        flags: u32,
        r#type: u32,
        target_name: *mut u16,
        comment: *mut u16,
        last_written: [u32; 2],
        credential_blob_size: u32,
        credential_blob: *mut u8,
        persist: u32,
        attribute_count: u32,
        attributes: *mut std::ffi::c_void,
        target_alias: *mut u16,
        user_name: *mut u16,
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn CredReadW(
            target_name: *const u16,
            r#type: u32,
            flags: u32,
            credential: *mut *mut CREDENTIALW,
        ) -> i32;
        fn CredFree(credential: *mut std::ffi::c_void);
    }

    let wide: Vec<u16> = OsStr::new(target)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(wide.as_ptr(), 1, 0, &mut ptr) != 0 && !ptr.is_null() {
            let cred = &*ptr;
            let size = cred.credential_blob_size as usize;
            let bytes = std::slice::from_raw_parts(cred.credential_blob, size);

            // Windows Credential Manager strings from keyring / Termigo are usually UTF-16
            let result = if size >= 2 && size.is_multiple_of(2) {
                let u16_slice: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|c| u16::from_ne_bytes([c[0], c[1]]))
                    .collect();
                String::from_utf16(&u16_slice)
                    .ok()
                    .map(|s| s.trim_matches(['\0', '\r', '\n', ' '].as_ref()).to_string())
                    .or_else(|| {
                        String::from_utf8(bytes.to_vec())
                            .ok()
                            .map(|s| s.trim_matches(['\0', '\r', '\n', ' '].as_ref()).to_string())
                    })
            } else {
                String::from_utf8(bytes.to_vec())
                    .ok()
                    .map(|s| s.trim_matches(['\0', '\r', '\n', ' '].as_ref()).to_string())
            };

            CredFree(ptr as *mut std::ffi::c_void);
            return result;
        }
    }
    None
}

#[cfg(not(target_os = "windows"))]
pub fn read_credential(_target: &str) -> Option<String> {
    None
}

/// Try to discover an existing authenticated GitHub Copilot session on the host
pub async fn discover_github_copilot() -> Option<OAuthToken> {
    // 1. Try Windows Credential Manager target
    let raw_json = read_credential("github-copilot-token.termigo-ai");
    let refresh_token = if let Some(text) = raw_json {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&text) {
            val.get("refresh_token")
                .and_then(|r| r.as_str())
                .map(str::to_string)
        } else {
            None
        }
    } else {
        None
    };

    let ghu_token = refresh_token.or_else(|| {
        std::env::var("COPILOT_REFRESH_TOKEN")
            .ok()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
    })?;

    let svc = OAuthService::new().ok()?;
    match svc.refresh(OAuthProvider::GithubCopilot, &ghu_token).await {
        Ok(token) => {
            info!("Successfully minted GitHub Copilot session from host credentials");
            Some(token)
        }
        Err(e) => {
            warn!("Failed to refresh GitHub Copilot token from host: {}", e);
            None
        }
    }
}

/// Try to discover an existing authenticated Google Antigravity session on the host
pub async fn discover_antigravity() -> Option<OAuthToken> {
    let mut refresh_token: Option<String> = None;

    // Check chunked credentials in Windows Credential Manager
    if let (Some(c0), Some(c1)) = (
        read_credential("antigravity-token__termigo_chunk_0.termigo-ai"),
        read_credential("antigravity-token__termigo_chunk_1.termigo-ai"),
    ) {
        let combined = format!("{}{}", c0, c1);
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&combined) {
            refresh_token = val
                .get("refresh_token")
                .and_then(|r| r.as_str())
                .map(str::to_string);
        }
    }

    if refresh_token.is_none() {
        if let Some(single) = read_credential("antigravity-token.termigo-ai") {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&single) {
                refresh_token = val
                    .get("refresh_token")
                    .and_then(|r| r.as_str())
                    .map(str::to_string);
            }
        }
    }

    // Check ~/.gemini/oauth_creds.json
    if refresh_token.is_none() {
        if let Some(home) = dirs::home_dir() {
            let path = home.join(".gemini").join("oauth_creds.json");
            if path.exists() {
                if let Ok(data) = std::fs::read_to_string(&path) {
                    if let Ok(val) = serde_json::from_str::<serde_json::Value>(&data) {
                        refresh_token = val
                            .get("refresh_token")
                            .and_then(|r| r.as_str())
                            .map(str::to_string);
                    }
                }
            }
        }
    }

    let rf = refresh_token.or_else(|| std::env::var("ANTIGRAVITY_REFRESH_TOKEN").ok())?;
    let svc = OAuthService::new().ok()?;
    match svc.refresh(OAuthProvider::Antigravity, &rf).await {
        Ok(token) => {
            info!("Successfully refreshed Antigravity Google token from host credentials");
            Some(token)
        }
        Err(e) => {
            warn!("Failed to refresh Antigravity Google token: {}", e);
            None
        }
    }
}

#[derive(Debug, Clone)]
pub struct CodexHostAuth {
    pub access_token: String,
    pub account_id: Option<String>,
}

/// Read the ChatGPT/Codex CLI login. The Codex CLI stores the session in
/// `$CODEX_HOME/auth.json` (or `~/.codex/auth.json`).
pub fn discover_codex() -> Option<CodexHostAuth> {
    let home = std::env::var_os("CODEX_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::home_dir().map(|home| home.join(".codex")))?;
    let text = std::fs::read_to_string(home.join("auth.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(&text).ok()?;
    let tokens = value.get("tokens")?;
    let access_token = tokens.get("access_token")?.as_str()?.trim().to_string();
    if access_token.is_empty() {
        return None;
    }
    let account_id = tokens
        .get("account_id")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Some(CodexHostAuth {
        access_token,
        account_id,
    })
}
