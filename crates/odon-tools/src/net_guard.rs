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
//!
//! # No resolve-then-connect window
//!
//! Validating a hostname and then letting the HTTP client resolve it again is a
//! DNS-rebinding TOCTOU: a name can answer with a public address during the
//! check and with `127.0.0.1` (or `169.254.169.254`) a moment later. So
//! [`validate_url`] does not just return a `Url`; it returns a
//! [`ValidatedTarget`] that keeps the exact [`SocketAddr`]s that were validated.
//! Callers must build their client through [`ValidatedTarget::client`] (or
//! [`pinned_client`]), which pins those addresses with
//! [`reqwest::ClientBuilder::resolve_to_addrs`]. The request then connects to
//! the validated address while the URL — and therefore the TLS SNI and `Host`
//! header — keeps the original hostname, so certificate checks stay intact.
//!
//! Redirects are fail-closed: automatic redirects are only followed while the
//! target host is one of the hosts already validated and pinned for that
//! client (see [`same_host_redirect_policy`]). A redirect to any other host is
//! reported instead of being re-resolved behind the guard's back; callers that
//! need a different host must validate it as a fresh target.

use odon_core::{OdonError, Result};
use std::net::{IpAddr, SocketAddr};

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

/// True for `localhost` and loopback literals.
fn is_loopback_host(host: &str) -> bool {
    match host.parse::<IpAddr>() {
        Ok(ip) => ip.is_loopback(),
        Err(_) => host.trim_end_matches('.').eq_ignore_ascii_case("localhost"),
    }
}

/// A URL that passed the network policy, together with the DNS answers that
/// were checked.
///
/// The addresses are the whole point: they are what the request must connect
/// to, so a later DNS answer cannot swap the destination. Build the HTTP client
/// with [`ValidatedTarget::client`] (or [`pinned_client`]) instead of
/// hand-rolling a `reqwest::Client`.
#[derive(Debug, Clone)]
pub struct ValidatedTarget {
    url: reqwest::Url,
    host: String,
    addrs: Vec<SocketAddr>,
    /// `true` when `addrs` are a DNS answer that must be pinned. Literal IP
    /// hosts are not pinned: the URL already names the address, so no resolver
    /// is involved and there is nothing to race.
    pinned: bool,
    /// The request came from a caller that explicitly allows loopback (the
    /// preview browser). Loopback answers are only acceptable in that scope.
    loopback_scope: bool,
}

impl ValidatedTarget {
    pub fn url(&self) -> &reqwest::Url {
        &self.url
    }

    /// The validated URL as a string, ready for `client.get(..)`.
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// The validated addresses. Requests built from this target connect here.
    pub fn addrs(&self) -> &[SocketAddr] {
        &self.addrs
    }

    /// Whether these addresses are pinned into the client (DNS names) or come
    /// straight from the URL (IP literals).
    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    /// The validated IPs, for callers that open sockets themselves.
    pub fn ip_addrs(&self) -> Vec<IpAddr> {
        let mut ips = Vec::new();
        for addr in &self.addrs {
            if !ips.contains(&addr.ip()) {
                ips.push(addr.ip());
            }
        }
        ips
    }

    /// Replace the validated addresses with a different answer for the same
    /// host — used when a DoH lookup beats a poisoned system resolver — after
    /// re-checking them against the same policy.
    pub fn with_addrs(&self, addrs: Vec<SocketAddr>) -> Result<Self> {
        if addrs.is_empty() {
            return Err(OdonError::Tool {
                tool: "url_guard".to_string(),
                message: format!("Host '{}' did not resolve to any address", self.host),
            });
        }
        check_resolved_addrs(&self.host, self.loopback_scope, &addrs)?;
        Ok(Self {
            url: self.url.clone(),
            host: self.host.clone(),
            addrs,
            pinned: true,
            loopback_scope: self.loopback_scope,
        })
    }

    /// A client pinned to the validated addresses, with fail-closed redirects.
    pub fn client(&self, builder: reqwest::ClientBuilder) -> Result<reqwest::Client> {
        pinned_client(&[self], builder)
    }

    /// Validate `url` and return a client pinned to the addresses that were just
    /// approved, together with the URL to request. Use this instead of building a
    /// bare `reqwest::Client`: an unvalidated client can reach cloud metadata
    /// endpoints and services bound to loopback.
    pub async fn pinned_client_for(
        url: &str,
        builder: reqwest::ClientBuilder,
    ) -> Result<(reqwest::Client, String)> {
        let target = validate_url(url).await?;
        let client = target.client(builder)?;
        Ok((client, target.as_str().to_string()))
    }
}

/// Pin every target's addresses into `builder`.
fn pin_targets(
    builder: reqwest::ClientBuilder,
    targets: &[&ValidatedTarget],
) -> reqwest::ClientBuilder {
    let mut builder = builder;
    for target in targets {
        if target.pinned && !target.addrs.is_empty() {
            builder = builder.resolve_to_addrs(&target.host, &target.addrs);
        }
    }
    builder
}

/// Build a client that can only reach the validated targets.
///
/// The addresses are pinned with [`reqwest::ClientBuilder::resolve_to_addrs`]
/// so the connection uses the checked answer, while the URL keeps the original
/// hostname for TLS SNI and `Host`. Redirects are restricted to the pinned
/// hosts: anything else is reported instead of being followed.
pub fn pinned_client(
    targets: &[&ValidatedTarget],
    builder: reqwest::ClientBuilder,
) -> Result<reqwest::Client> {
    let hosts: Vec<&str> = targets.iter().map(|target| target.host.as_str()).collect();
    let builder = pin_targets(builder, targets).redirect(same_host_redirect_policy(&hosts));
    build_client(builder)
}

/// Chromium's `--host-resolver-rules` value that pins `target`'s host to the
/// address that was validated.
///
/// A browser is an HTTP client with its own resolver, so handing it a bare
/// hostname after validating that hostname re-opens the resolve-then-connect
/// window this module exists to close. Mapping the name to the validated
/// address closes it without disturbing TLS: the URL and `Host` header keep the
/// name, so certificate checks still run against the real host.
///
/// With `fail_closed`, every other name resolves to nothing, so a redirect hop
/// cannot reach a host that no one validated.
pub fn host_resolver_rules(target: &ValidatedTarget, fail_closed: bool) -> Option<String> {
    if !target.pinned {
        return None;
    }
    let ip = match target.addrs.first()?.ip() {
        IpAddr::V4(v4) => v4.to_string(),
        IpAddr::V6(v6) => format!("[{v6}]"),
    };
    let mut rules = format!("MAP {} {ip}", target.host);
    if fail_closed {
        rules.push_str(",MAP * ~NOTFOUND");
    }
    Some(rules)
}

/// Build a `reqwest::Client`, surfacing builder failures instead of silently
/// falling back to a default client that has none of these guards.
pub fn build_client(builder: reqwest::ClientBuilder) -> Result<reqwest::Client> {
    builder.build().map_err(|e| OdonError::Tool {
        tool: "http_client".to_string(),
        message: format!("Cannot build HTTP client: {e}"),
    })
}

/// Validate a user-supplied URL before any network request.
///
/// Returns the resolved addresses with the URL: callers must connect through
/// them (see [`ValidatedTarget::client`]) so validation and connection cannot
/// disagree about where the host points.
pub async fn validate_url(raw: &str) -> Result<ValidatedTarget> {
    validate_url_inner(raw, false).await
}

/// Validate a URL for the operator's preview browser.
///
/// The browser panel exists to show the app being worked on, which usually
/// lives on loopback, so `localhost`/`127.0.0.1`/`[::1]` are permitted here.
/// This mirrors the rule `/api/browser/read` already applies. Private LAN,
/// link-local, CGNAT and cloud-metadata addresses stay blocked: those reach
/// another machine, which the operator never asked for. Loopback answers are
/// only accepted for loopback *hosts*, so a public name that answers with
/// `127.0.0.1` cannot use this relaxation.
pub async fn validate_url_allow_loopback(raw: &str) -> Result<ValidatedTarget> {
    validate_url_inner(raw, true).await
}

async fn validate_url_inner(raw: &str, allow_loopback: bool) -> Result<ValidatedTarget> {
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

    let loopback_scope = allow_loopback && is_loopback_host(&host);
    if !allow_private_network() && is_blocked_hostname(&host) && !loopback_scope {
        return Err(OdonError::Tool {
            tool: "url_guard".to_string(),
            message: format!(
                "Host '{host}' is private/loopback. Set ODON_ALLOW_PRIVATE_NETWORK=1 to permit local targets."
            ),
        });
    }

    let port = url.port_or_known_default().unwrap_or(443);
    // An IP literal needs no resolver: the URL already names the destination,
    // so there is no answer to pin and no window to rebind.
    let (addrs, pinned) = match host.parse::<IpAddr>() {
        Ok(ip) => (vec![SocketAddr::new(ip, port)], false),
        Err(_) => {
            let addrs = lookup_host(&host, port).await?;
            check_resolved_addrs(&host, loopback_scope, &addrs)?;
            (addrs, true)
        }
    };

    Ok(ValidatedTarget {
        url,
        host,
        addrs,
        pinned,
        loopback_scope,
    })
}

async fn lookup_host(host: &str, port: u16) -> Result<Vec<SocketAddr>> {
    let lookup = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| OdonError::Tool {
            tool: "url_guard".to_string(),
            message: format!("DNS resolution failed for '{host}': {e}"),
        })?;
    let addrs: Vec<SocketAddr> = lookup.collect();
    if addrs.is_empty() {
        return Err(OdonError::Tool {
            tool: "url_guard".to_string(),
            message: format!("Host '{host}' did not resolve to any address"),
        });
    }
    Ok(addrs)
}

/// Every resolved address must be public unless the operator opted into
/// private networks, or the caller explicitly scoped this request to loopback
/// and the answer really is loopback.
fn check_resolved_addrs(host: &str, loopback_scope: bool, addrs: &[SocketAddr]) -> Result<()> {
    if allow_private_network() {
        return Ok(());
    }
    for addr in addrs {
        let permitted_loopback = loopback_scope && addr.ip().is_loopback();
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
    Ok(())
}

/// Redirect policy for a client whose connections are pinned to `hosts`.
///
/// Fail-closed: a hop is only followed when its host is one of the hosts that
/// were validated *and* pinned for this client. A redirect to a new host would
/// be re-resolved by the HTTP client with no validation and no pinning, which is
/// exactly the SSRF hole this module exists to close, so it is refused and
/// reported to the caller. Same-host redirects (including a path, port or
/// `http`/`https` change) stay pinned to the validated addresses and are safe.
pub fn same_host_redirect_policy(hosts: &[&str]) -> reqwest::redirect::Policy {
    let allowed: Vec<String> = hosts
        .iter()
        .map(|host| host.trim_end_matches('.').to_ascii_lowercase())
        .collect();
    reqwest::redirect::Policy::custom(move |attempt| {
        if attempt.previous().len() >= 5 {
            return attempt.error("too many redirects");
        }
        match redirect_decision(&allowed, attempt.url()) {
            Ok(()) => attempt.follow(),
            Err(reason) => attempt.error(reason),
        }
    })
}

/// The single decision used for every redirect hop. Kept pure so the rules can
/// be tested without making a network request.
fn redirect_decision(
    pinned_hosts: &[String],
    target: &reqwest::Url,
) -> std::result::Result<(), &'static str> {
    if !matches!(target.scheme(), "http" | "https") {
        return Err("redirect to a non-http(s) scheme blocked");
    }
    let host = target
        .host_str()
        .unwrap_or("")
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if host.is_empty() {
        return Err("redirect without a host blocked");
    }
    // Defense in depth: a metadata host can never be a validated pin, but never
    // let one through even if a future caller builds its host list wrong.
    if is_metadata_host(&host) {
        return Err("redirect to a cloud metadata endpoint blocked");
    }
    if !pinned_hosts.contains(&host) {
        return Err("redirect to an unvalidated host blocked (fail-closed SSRF policy)");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(raw: &str) -> reqwest::Url {
        reqwest::Url::parse(raw).unwrap()
    }

    fn hosts(list: &[&str]) -> Vec<String> {
        list.iter().map(|host| host.to_string()).collect()
    }

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

    #[test]
    fn follows_same_host_redirects_only_when_pinned() {
        let pinned = hosts(&["example.com"]);
        // Same host, different path/port/scheme: still pinned to the validated
        // addresses, so following it cannot reach a new destination.
        assert!(redirect_decision(&pinned, &url("https://example.com/login")).is_ok());
        assert!(redirect_decision(&pinned, &url("https://example.com:8443/api")).is_ok());
        assert!(redirect_decision(&pinned, &url("http://example.com/plain")).is_ok());
        // Host comparison is case-insensitive and tolerates a trailing dot,
        // matching how the URL parser hands the name to the resolver.
        assert!(redirect_decision(&pinned, &url("https://EXAMPLE.com./x")).is_ok());
    }

    #[test]
    fn rejects_cross_host_redirects() {
        let pinned = hosts(&["example.com"]);
        // The classic cross-host bounce is now refused: following it would let
        // reqwest resolve a host that was never validated or pinned.
        for target in [
            "https://www.example.com/",
            "https://evil.example.net/",
            "https://sub.example.com/",
            "http://127.0.0.1:4096/api/status",
            "http://10.0.0.5/admin",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost:3000/",
        ] {
            assert!(
                redirect_decision(&pinned, &url(target)).is_err(),
                "{target} must not be followed"
            );
        }
        // A pinned host list is exact: another validated host is not implied.
        let multi = hosts(&["news.google.com", "hn.algolia.com"]);
        assert!(redirect_decision(&multi, &url("https://hn.algolia.com/api")).is_ok());
        assert!(redirect_decision(&multi, &url("https://example.com/")).is_err());
    }

    #[test]
    fn rejects_redirects_to_blocked_schemes_and_metadata() {
        let pinned = hosts(&["example.com"]);
        assert!(redirect_decision(&pinned, &url("file:///etc/passwd")).is_err());
        assert!(redirect_decision(&pinned, &url("ftp://example.com/x")).is_err());
        // Even a caller that pins a metadata name cannot redirect to it.
        let metadata = hosts(&["metadata.google.internal"]);
        assert!(redirect_decision(&metadata, &url("http://metadata.google.internal/")).is_err());
    }

    #[test]
    fn loopback_redirects_only_survive_for_loopback_clients() {
        if allow_private_network() {
            return;
        }
        // The preview panel serves a dev server on loopback, so a loopback to
        // loopback redirect on the same host must work...
        let preview = hosts(&["127.0.0.1"]);
        assert!(redirect_decision(&preview, &url("http://127.0.0.1:3001/")).is_ok());
        // ...but a public page must never bounce into loopback, and a loopback
        // page must not bounce to a different local name either.
        assert!(
            redirect_decision(&hosts(&["example.com"]), &url("http://127.0.0.1:3001/")).is_err()
        );
        assert!(redirect_decision(&preview, &url("http://localhost:3001/")).is_err());
    }

    #[tokio::test]
    async fn literal_addresses_are_validated_without_a_pin() {
        if allow_private_network() {
            return;
        }
        // A literal IP is its own answer: it is validated, kept as the connect
        // target, and never pinned (no resolver is involved).
        let public = validate_url("http://1.1.1.1:8080/path").await.unwrap();
        assert_eq!(public.host(), "1.1.1.1");
        assert_eq!(public.addrs(), [SocketAddr::from(([1, 1, 1, 1], 8080))]);
        assert!(!public.is_pinned());
        assert_eq!(public.ip_addrs(), [IpAddr::from([1, 1, 1, 1])]);
        // Private literals stay blocked on the public guard.
        assert!(validate_url("http://127.0.0.1/").await.is_err());
        assert!(validate_url("http://10.1.2.3/").await.is_err());
        assert!(validate_url("http://169.254.169.254/").await.is_err());
        assert!(validate_url("http://[::1]/").await.is_err());
    }

    #[tokio::test]
    async fn loopback_scope_allows_loopback_literals_only() {
        if allow_private_network() {
            return;
        }
        let preview = validate_url_allow_loopback("http://127.0.0.1:3000/")
            .await
            .unwrap();
        assert_eq!(preview.addrs(), [SocketAddr::from(([127, 0, 0, 1], 3000))]);
        // The relaxation is loopback-only: LAN and metadata stay blocked.
        assert!(validate_url_allow_loopback("http://10.1.2.3/")
            .await
            .is_err());
        assert!(validate_url_allow_loopback("http://192.168.1.1/")
            .await
            .is_err());
        assert!(validate_url_allow_loopback("http://169.254.169.254/")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn rejected_addresses_cannot_be_swapped_in() {
        if allow_private_network() {
            return;
        }
        let target = validate_url("http://1.1.1.1/").await.unwrap();
        assert!(!target.is_pinned());
        // A DoH answer (or any other source) cannot smuggle in a blocked
        // address for an otherwise-fine host.
        assert!(target
            .with_addrs(vec![SocketAddr::from(([127, 0, 0, 1], 80))])
            .is_err());
        assert!(target.with_addrs(Vec::new()).is_err());
        let swapped = target
            .with_addrs(vec![SocketAddr::from(([1, 0, 0, 1], 80))])
            .unwrap();
        assert!(swapped.is_pinned());
        assert_eq!(swapped.host(), "1.1.1.1");
    }

    #[tokio::test]
    async fn browser_rules_pin_the_validated_address() {
        if allow_private_network() {
            return;
        }
        // A literal IP needs no rule: the browser already names the address.
        let literal = validate_url("http://1.1.1.1/").await.unwrap();
        assert!(host_resolver_rules(&literal, true).is_none());

        // A resolved name is mapped to the exact validated address, and a
        // fail-closed rule set also refuses every other name so a redirect hop
        // cannot reach a host that was never validated.
        let pinned = literal
            .with_addrs(vec![SocketAddr::from(([93, 184, 216, 34], 80))])
            .unwrap();
        let rules = host_resolver_rules(&pinned, true).unwrap();
        assert!(rules.starts_with("MAP 1.1.1.1 93.184.216.34"));
        assert!(rules.contains("MAP * ~NOTFOUND"));
        assert!(!host_resolver_rules(&pinned, false).unwrap().contains('~'));
    }
}
