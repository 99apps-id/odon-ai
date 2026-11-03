//! SSRF protection for tools that accept arbitrary URLs.
//!
//! By default only public `http`/`https` destinations are allowed. Private,
//! loopback, link-local, CGNAT and metadata addresses are rejected so an agent
//! (or a malicious model output) cannot reach the daemon itself, cloud
//! metadata, or other services on the host/LAN.
//!
//! Opt-ins:
//! - `ODON_ALLOW_PRIVATE_NETWORK=1` permits private/loopback targets.
//! - `ODON_ALLOWED_HOSTS=example.com,*.internal` restricts requests to a list.
//! - `ODON_BLOCKED_HOSTS=...` adds extra blocked hostnames.

use odon_core::{OdonError, Result};
use std::net::IpAddr;

fn env_flag(name: &str) -> bool {
    std::env::var(name)
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn allow_private_network() -> bool {
    env_flag("ODON_ALLOW_PRIVATE_NETWORK")
}

fn csv_env(name: &str) -> Vec<String> {
    std::env::var(name)
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(|item| item.trim().to_ascii_lowercase())
                .filter(|item| !item.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn host_allowed_by_list(host: &str, list: &[String]) -> bool {
    list.iter().any(|pattern| {
        if let Some(suffix) = pattern.strip_prefix("*.") {
            host.ends_with(&format!(".{suffix}"))
        } else {
            host == pattern
        }
    })
}

fn metadata_addresses() -> &'static [&'static str] {
    &[
        "169.254.169.254",
        "169.254.170.2",
        "fd00:ec2::254",
        "100.100.100.200",
        "metadata.google.internal",
        "metadata.azure.com",
    ]
}

fn is_metadata_host(host: &str) -> bool {
    metadata_addresses()
        .iter()
        .any(|blocked| host.eq_ignore_ascii_case(blocked))
}

pub fn is_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let octets = v4.octets();
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || v4.is_multicast()
                || v4.is_documentation()
                // Carrier-grade NAT 100.64.0.0/10
                || (octets[0] == 100 && (64..=127).contains(&octets[1]))
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
            {
                return true;
            }
            // IPv4-mapped addresses must be classified by the embedded v4.
            if let Some(v4) = v6.to_ipv4_mapped() {
                return is_blocked_ip(IpAddr::V4(v4));
            }
            false
        }
    }
}

fn is_blocked_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty()
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.ends_with(".local")
        || host.ends_with(".internal")
        || host.ends_with(".home.arpa")
        || host == "metadata"
        || is_metadata_host(&host)
    {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_blocked_ip(ip);
    }
    false
}

/// Validate a user-supplied URL before any network request.
pub async fn validate_url(raw: &str) -> Result<reqwest::Url> {
    validate_url_inner(raw, false).await
}

/// Validate a URL for the operator's preview browser.
///
/// The browser panel exists to show the app being worked on, which usually
/// lives on loopback, so `localhost`/`127.0.0.1`/`[::1]` are permitted here.
/// This mirrors the rule `/api/browser/read` already applies. Private LAN,
/// link-local, CGNAT and cloud-metadata addresses stay blocked: those reach
/// another machine, which the operator never asked for.
pub async fn validate_url_allow_loopback(raw: &str) -> Result<reqwest::Url> {
    validate_url_inner(raw, true).await
}

async fn validate_url_inner(raw: &str, allow_loopback: bool) -> Result<reqwest::Url> {
    let url = reqwest::Url::parse(raw.trim()).map_err(|e| OdonError::Tool {
        tool: "url_guard".to_string(),
        message: format!("Invalid URL: {e}"),
    })?;

    match url.scheme() {
        "http" | "https" => {}
        other => {
            return Err(OdonError::Tool {
                tool: "url_guard".to_string(),
                message: format!("Blocked URL scheme '{other}': only http and https are allowed"),
            });
        }
    }

    if url.username() != "" || url.password().is_some() {
        return Err(OdonError::Tool {
            tool: "url_guard".to_string(),
            message: "URLs with embedded credentials are not allowed".to_string(),
        });
    }

    let host = url
        .host_str()
        .ok_or_else(|| OdonError::Tool {
            tool: "url_guard".to_string(),
            message: "URL has no host".to_string(),
        })?
        .to_ascii_lowercase();

    let allowed_hosts = csv_env("ODON_ALLOWED_HOSTS");
    if !allowed_hosts.is_empty() && !host_allowed_by_list(&host, &allowed_hosts) {
        return Err(OdonError::Tool {
            tool: "url_guard".to_string(),
            message: format!("Host '{host}' is not in ODON_ALLOWED_HOSTS"),
        });
    }
    let blocked_hosts = csv_env("ODON_BLOCKED_HOSTS");
    if host_allowed_by_list(&host, &blocked_hosts) {
        return Err(OdonError::Tool {
            tool: "url_guard".to_string(),
            message: format!("Host '{host}' is in ODON_BLOCKED_HOSTS"),
        });
    }

    if is_metadata_host(&host) {
        return Err(OdonError::Tool {
            tool: "url_guard".to_string(),
            message: "Cloud metadata endpoints are always blocked".to_string(),
        });
    }

    if !allow_private_network() {
        let host_is_loopback = match host.parse::<IpAddr>() {
            Ok(ip) => ip.is_loopback(),
            Err(_) => host == "localhost",
        };
        if is_blocked_hostname(&host) && !(allow_loopback && host_is_loopback) {
            return Err(OdonError::Tool {
                tool: "url_guard".to_string(),
                message: format!(
                    "Host '{host}' is private/loopback. Set ODON_ALLOW_PRIVATE_NETWORK=1 to permit local targets."
                ),
            });
        }

        let port = url.port_or_known_default().unwrap_or(443);
        let lookup = tokio::net::lookup_host((host.as_str(), port))
            .await
            .map_err(|e| OdonError::Tool {
                tool: "url_guard".to_string(),
                message: format!("DNS resolution failed for '{host}': {e}"),
            })?;
        let mut checked = false;
        for addr in lookup {
            checked = true;
            let permitted_loopback = allow_loopback && addr.ip().is_loopback();
            if is_blocked_ip(addr.ip()) && !permitted_loopback {
                return Err(OdonError::Tool {
                    tool: "url_guard".to_string(),
                    message: format!(
                        "Host '{host}' resolves to a private/internal address ({})",
                        addr.ip()
                    ),
                });
            }
        }
        if !checked {
            return Err(OdonError::Tool {
                tool: "url_guard".to_string(),
                message: format!("Host '{host}' did not resolve to any address"),
            });
        }
    }

    Ok(url)
}

/// True for `localhost` and loopback literals, matching `validate_url_inner`.
fn is_loopback_host(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host.trim_end_matches('.').eq_ignore_ascii_case("localhost"),
    }
}

/// Redirect policy that keeps SSRF protection while allowing normal web
/// redirects.
///
/// The previous version required every hop to stay on the original host, so the
/// ordinary `example.com` -> `www.example.com` bounce was reported as
/// "error following redirect for url" and most real sites became unreadable.
/// Matching the initial-URL rule instead: block cloud metadata, the operator's
/// host lists, non-http(s) schemes, and private addresses (loopback only
/// survives when the request itself started on loopback, which is the preview
/// panel's dev-server case). A public hostname therefore always stays reachable.
pub fn redirect_policy(original_host: String) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        match redirect_decision(&original_host, attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(reason) => attempt.error(reason),
        }
    })
}

/// The single decision used for every redirect hop. Kept pure so the rules can
/// be tested without making a network request.
fn redirect_decision(original_host: &str, target: &reqwest::Url) -> std::result::Result<(), &'static str> {
    if !matches!(target.scheme(), "http" | "https") {
        return Err("redirect to a non-http(s) scheme blocked");
    }
    let host = target.host_str().unwrap_or("").to_ascii_lowercase();
    if host.is_empty() {
        return Err("redirect without a host blocked");
    }
    if is_metadata_host(&host) {
        return Err("redirect to a cloud metadata endpoint blocked");
    }
    let allowed_hosts = csv_env("ODON_ALLOWED_HOSTS");
    if !allowed_hosts.is_empty() && !host_allowed_by_list(&host, &allowed_hosts) {
        return Err("redirect host is not in ODON_ALLOWED_HOSTS");
    }
    let blocked_hosts = csv_env("ODON_BLOCKED_HOSTS");
    if host_allowed_by_list(&host, &blocked_hosts) {
        return Err("redirect host is in ODON_BLOCKED_HOSTS");
    }
    if !allow_private_network()
        && is_blocked_hostname(&host)
        && !(is_loopback_host(original_host) && is_loopback_host(&host))
    {
        return Err("redirect to a private host blocked by SSRF policy");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_private_and_public_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
        ] {
            assert!(is_blocked_ip(ip.parse().unwrap()), "should block {ip}");
        }
        for ip in ["1.1.1.1", "8.8.8.8", "2606:4700:4700::1111"] {
            assert!(!is_blocked_ip(ip.parse().unwrap()), "should allow {ip}");
        }
    }

    #[test]
    fn blocks_localhost_and_metadata_names() {
        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("foo.local"));
        assert!(is_blocked_hostname("metadata.google.internal"));
        assert!(!is_blocked_hostname("github.com"));
    }

    #[tokio::test]
    async fn rejects_non_http_schemes() {
        assert!(validate_url("file:///etc/passwd").await.is_err());
        assert!(validate_url("gopher://example.com").await.is_err());
    }

    fn url(raw: &str) -> reqwest::Url {
        reqwest::Url::parse(raw).unwrap()
    }

    #[test]
    fn allows_ordinary_public_cross_host_redirects() {
        // The regression: google.com redirects to www.google.com, and most sites
        // bounce host this way. Requiring the same host broke normal browsing.
        assert!(redirect_decision("google.com", &url("https://www.google.com/")).is_ok());
        assert!(redirect_decision("example.com", &url("https://example.com/id")).is_ok());
        assert!(redirect_decision("httpbin.org", &url("https://httpbin.org/get")).is_ok());
        // A path-only redirect keeps the host and must stay allowed.
        assert!(redirect_decision("github.com", &url("https://github.com/login")).is_ok());
    }

    #[test]
    fn still_blocks_ssrf_redirect_targets() {
        if allow_private_network() {
            return; // operator opted out of the private-network guard
        }
        for target in [
            "http://127.0.0.1:4096/api/status",
            "http://10.0.0.5/admin",
            "http://192.168.1.1/",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost:3000/",
            "http://metadata.google.internal/",
        ] {
            assert!(
                redirect_decision("example.com", &url(target)).is_err(),
                "{target} must stay blocked"
            );
        }
        assert!(redirect_decision("example.com", &url("file:///etc/passwd")).is_err());
        assert!(redirect_decision("example.com", &url("ftp://example.com/x")).is_err());
    }

    #[test]
    fn loopback_redirect_only_survives_from_loopback() {
        if allow_private_network() {
            return;
        }
        // The preview panel serves a dev server on loopback, so a loopback to
        // loopback redirect must work.
        assert!(redirect_decision("127.0.0.1", &url("http://127.0.0.1:3001/")).is_ok());
        // A public page must never be able to bounce into loopback.
        assert!(redirect_decision("example.com", &url("http://127.0.0.1:3001/")).is_err());
    }
}
