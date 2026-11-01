use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use chrono::{DateTime, Utc};
use odon_core::{OdonError, Result};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    pub provider: String,
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub account_id: Option<String>,
}

pub struct OAuthHandler;

impl OAuthHandler {
    pub fn generate_pkce() -> Result<(String, String)> {
        let mut buf = [0u8; 32];
        SystemRandom::new()
            .fill(&mut buf)
            .map_err(|_| OdonError::Auth("Failed to generate secure random bytes".to_string()))?;

        let verifier = URL_SAFE_NO_PAD.encode(buf);
        let hash = Sha256::digest(verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hash);

        Ok((verifier, challenge))
    }

    /// Spawns an ephemeral TCP listener on 127.0.0.1 to capture the redirect code
    pub async fn wait_for_redirect_code(port: u16) -> Result<String> {
        let addr: SocketAddr = format!("127.0.0.1:{}", port)
            .parse()
            .map_err(|e| OdonError::Auth(format!("Invalid address: {}", e)))?;

        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| OdonError::Auth(format!("Failed to bind loopback OAuth port: {}", e)))?;

        let (mut socket, _) = listener
            .accept()
            .await
            .map_err(|e| OdonError::Auth(format!("Failed to accept connection: {}", e)))?;

        let mut buf = [0u8; 2048];
        let n = socket
            .read(&mut buf)
            .await
            .map_err(|e| OdonError::Auth(format!("Failed to read HTTP request: {}", e)))?;

        let req = String::from_utf8_lossy(&buf[..n]);
        let first_line = req.lines().next().unwrap_or_default();

        // Extract code from "GET /?code=... HTTP/1.1" or "GET /callback?code=..."
        let mut auth_code = None;
        if let Some(path) = first_line.split_whitespace().nth(1) {
            if let Some(query) = path.split('?').nth(1) {
                for pair in query.split('&') {
                    let mut parts = pair.split('=');
                    if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
                        if k == "code" {
                            auth_code = Some(v.to_string());
                            break;
                        }
                    }
                }
            }
        }

        // Return a clean HTML success response
        let resp_body = "<!DOCTYPE html><html><body style='font-family: sans-serif; text-align: center; padding: 50px;'><h2>ODON OAuth Successful!</h2><p>You can close this tab and return to ODON.</p></body></html>";
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            resp_body.len(),
            resp_body
        );

        let _ = socket.write_all(response.as_bytes()).await;

        auth_code.ok_or_else(|| OdonError::Auth("No authorization code found in redirect".to_string()))
    }
}
