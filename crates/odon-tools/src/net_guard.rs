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
        if is_blocked_hostname(&host) {
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
            if is_blocked_ip(addr.ip()) {
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

/// Redirect policy that prevents an allowed public URL from bouncing to an
/// internal host. Redirects must stay on the same host and be http(s).
pub fn redirect_policy(original_host: String) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        let url = attempt.url();
        if !matches!(url.scheme(), "http" | "https") {
            return attempt.error("redirect to a non-http(s) scheme blocked");
        }
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        if host != original_host.to_ascii_lowercase() {
            return attempt.error("cross-host redirect blocked by SSRF policy");
        }
        if !allow_private_network() && is_blocked_hostname(&host) {
            return attempt.error("redirect to a private host blocked by SSRF policy");
        }
        attempt.follow()
    })
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
}
