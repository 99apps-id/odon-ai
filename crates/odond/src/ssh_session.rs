//! Native SSH sessions for the interactive terminal.
//!
//! Why this exists instead of spawning `ssh.exe` inside a ConPTY: OpenSSH for
//! Windows cannot run an interactive shell under a ConPTY. Verified on this
//! machine — `ssh -V` (no network at all) prints nothing under the ConPTY and
//! returns to the prompt, while the same binary works normally from a real
//! console, and `ssh host "cmd"` (command mode) does work under the ConPTY.
//! Because of that, a PTY wired to `ssh.exe` shows a blank terminal: the child
//! exits before writing a single byte.
//!
//! Driving the protocol in-process with `russh` (the same approach Termigo
//! uses) removes the dependency on a shell console entirely: the browser talks
//! to a real SSH channel, resize becomes a real window-change request, and the
//! private key never leaves the daemon.
//!
//! Host keys are trust-on-first-use with pinning: the SHA-256 fingerprint is
//! recorded on first connect and every later connect must match, so a swapped
//! key is refused instead of silently trusted (the old `ssh.exe` path only had
//! `StrictHostKeyChecking=accept-new` and re-accepted a changed key).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use russh::client::{self, Handle};
use russh::keys::{load_secret_key, HashAlg, PrivateKeyWithHashAlg};
use russh::{ChannelReadHalf, ChannelWriteHalf, Disconnect};
use serde::{Deserialize, Serialize};

/// Handshake budget. A black-holed host must not hang a terminal tab forever.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// Terminal type advertised to the remote shell.
const TERM: &str = "xterm-256color";

/// Everything needed to open one interactive SSH session.
pub struct ConnectOptions {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: PathBuf,
    pub cols: u32,
    pub rows: u32,
    /// `host:port`, the key used in the known-hosts store.
    pub store_key: String,
    /// Path of the JSON known-hosts store.
    pub store_path: PathBuf,
}

impl ConnectOptions {
    /// Options for a saved profile, keeping pinning data under `ODON_HOME`.
    pub fn for_profile(
        host: &str,
        user: &str,
        port: u16,
        key_path: Option<&str>,
        cols: u16,
        rows: u16,
    ) -> Result<Self> {
        let key_path = key_path
            .map(PathBuf::from)
            .ok_or_else(|| anyhow!("profile {user}@{host} has no private key"))?;
        let base = std::env::var_os("ODON_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("."));
        let store_dir = base.join(".odon");
        std::fs::create_dir_all(&store_dir).context("create ODON state directory")?;
        Ok(Self {
            host: host.to_string(),
            port,
            user: user.to_string(),
            key_path,
            cols: cols as u32,
            rows: rows as u32,
            store_key: format!("{host}:{port}"),
            store_path: store_dir.join("known_hosts.json"),
        })
    }
}

/// Outcome of the host-key check, shared with the caller because
/// `check_server_key` cannot attach a message to its own error.
#[derive(Default)]
struct KeyCheck {
    seen: Mutex<Option<String>>,
    rejection: Mutex<Option<String>>,
}

pub struct Verifier {
    expected: Option<String>,
    check: Arc<KeyCheck>,
}

impl client::Handler for Verifier {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &russh::keys::PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        let key = key.public_key();
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        if let Ok(mut slot) = self.check.seen.lock() {
            *slot = Some(fingerprint.clone());
        }
        if let Some(expected) = &self.expected {
            if expected != &fingerprint {
                if let Ok(mut slot) = self.check.rejection.lock() {
                    *slot = Some(format!(
                        "host key mismatch for this host (possible MITM): \
                         expected {expected}, server presented {fingerprint}"
                    ));
                }
                return Ok(false);
            }
        }
        Ok(true)
    }
}

/// One live interactive session: the transport handle plus the shell channel.
pub struct SshSession {
    handle: Handle<Verifier>,
    channel: russh::Channel<russh::client::Msg>,
    /// Fingerprint the server actually presented.
    pub fingerprint: String,
}

impl SshSession {
    pub async fn connect(opts: &ConnectOptions) -> Result<Self> {
        let Authenticated {
            handle,
            fingerprint,
        } = authenticate(opts).await?;
        let channel = handle.channel_open_session().await?;
        channel
            .request_pty(true, TERM, opts.cols, opts.rows, 0, 0, &[])
            .await?;
        channel.request_shell(true).await?;
        Ok(Self {
            handle,
            channel,
            fingerprint,
        })
    }

    /// Split into a read half and a write half.
    ///
    /// The terminal loop needs to wait on remote output and ship keystrokes at
    /// the same time; one `Channel` cannot be borrowed both ways at once, so the
    /// two halves are handed out separately.
    pub fn into_parts(
        self,
    ) -> (
        Handle<Verifier>,
        ChannelReadHalf,
        ChannelWriteHalf<russh::client::Msg>,
        String,
    ) {
        let (read, write) = self.channel.split();
        (self.handle, read, write, self.fingerprint)
    }
}

/// A transport that has completed handshake and public-key auth but has not
/// opened any channel yet.
///
/// The terminal opens a shell channel from it; the SFTP browser opens an
/// `sftp` subsystem channel from its own connection. Sharing the handshake
/// (host-key pinning, key loading, auth) keeps both paths identical instead of
/// duplicating security-relevant code.
pub struct Authenticated {
    pub handle: Handle<Verifier>,
    pub fingerprint: String,
}

pub async fn authenticate(opts: &ConnectOptions) -> Result<Authenticated> {
    let expected = load_known_host(&opts.store_path, &opts.store_key);
    let check = Arc::new(KeyCheck::default());
    let config = Arc::new(client::Config {
        // An interactive shell can sit idle for hours; the default
        // activity timeout would drop the connection.
        inactivity_timeout: None,
        ..<_>::default()
    });
    let handler = Verifier {
        expected: expected.clone(),
        check: check.clone(),
    };

    let addr = (opts.host.as_str(), opts.port);
    let connect = tokio::time::timeout(CONNECT_TIMEOUT, client::connect(config, addr, handler));
    let mut handle = match connect.await {
        Err(_) => bail!(
            "timed out after {}s connecting to {}:{}",
            CONNECT_TIMEOUT.as_secs(),
            opts.host,
            opts.port
        ),
        Ok(Err(e)) => {
            // A refused host key surfaces here as a generic handshake
            // error; report the real reason when we have one.
            if let Some(reason) = check.rejection.lock().ok().and_then(|s| s.clone()) {
                bail!("{reason}");
            }
            return Err(anyhow!(e))
                .with_context(|| format!("connecting to {}:{}", opts.host, opts.port));
        }
        Ok(Ok(h)) => h,
    };

    let fingerprint = check
        .seen
        .lock()
        .ok()
        .and_then(|s| s.clone())
        .unwrap_or_else(|| "unknown".to_string());

    let key_pair = load_secret_key(&opts.key_path, None).with_context(|| {
        format!(
            "reading private key {} (is it a valid OpenSSH/ PEM key?)",
            opts.key_path.display()
        )
    })?;
    // The connect timeout above only bounds TCP+KEX. A server that stalls the
    // authentication phase would otherwise hang the terminal tab forever.
    let auth = tokio::time::timeout(CONNECT_TIMEOUT, async {
        let hash_alg = handle.best_supported_rsa_hash().await?.flatten();
        handle
            .authenticate_publickey(
                opts.user.clone(),
                PrivateKeyWithHashAlg::new(Arc::new(key_pair), hash_alg),
            )
            .await
    })
    .await
    .map_err(|_| {
        anyhow!(
            "timed out after {}s authenticating to {}:{}",
            CONNECT_TIMEOUT.as_secs(),
            opts.host,
            opts.port
        )
    })??;
    if !auth.success() {
        let _ = handle.disconnect(Disconnect::ByApplication, "", "en").await;
        bail!(
            "authentication failed for {}@{} — the server did not accept this key",
            opts.user,
            opts.host
        );
    }

    // Pin the fingerprint on first use, and remember a rotation only after
    // the caller has seen the mismatch path above.
    if expected.is_none() {
        let _ = save_known_host(&opts.store_path, &opts.store_key, &fingerprint);
    }

    Ok(Authenticated {
        handle,
        fingerprint,
    })
}

/// Text to show in the terminal for a failed session, so the user gets the
/// real cause instead of a blank pane.
pub fn describe_error(err: &anyhow::Error) -> String {
    let mut text = format!("SSH connection failed: {err}\r\n");
    for cause in err.chain().skip(1) {
        text.push_str(&format!("  caused by: {cause}\r\n"));
    }
    text
}

#[derive(Serialize, Deserialize, Default)]
struct KnownHosts {
    #[serde(default)]
    hosts: BTreeMap<String, String>,
}

fn read_known_hosts(path: &Path) -> KnownHosts {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn load_known_host(path: &Path, store_key: &str) -> Option<String> {
    read_known_hosts(path).hosts.get(store_key).cloned()
}

fn save_known_host(path: &Path, store_key: &str, fingerprint: &str) -> Result<()> {
    let mut known = read_known_hosts(path);
    known
        .hosts
        .insert(store_key.to_string(), fingerprint.to_string());
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let raw = serde_json::to_string_pretty(&known)?;
    // Write to a temp file and rename, so a crash mid-write cannot leave a
    // half-written store (which reads as empty and would re-TOFU the host).
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, raw)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("odon-ssh-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir.join("known-hosts.json")
    }

    #[test]
    fn known_host_round_trips_and_is_pinned() {
        let path = temp_store("round-trip");
        let _ = std::fs::remove_file(&path);

        assert!(load_known_host(&path, "example:22").is_none());

        save_known_host(&path, "example:22", "SHA256:abc").expect("save");
        assert_eq!(
            load_known_host(&path, "example:22").as_deref(),
            Some("SHA256:abc")
        );

        // A different host is independent, and re-pinning one host must not
        // drop the other.
        save_known_host(&path, "other:2222", "SHA256:def").expect("save other");
        assert_eq!(
            load_known_host(&path, "example:22").as_deref(),
            Some("SHA256:abc")
        );
        assert_eq!(
            load_known_host(&path, "other:2222").as_deref(),
            Some("SHA256:def")
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn corrupt_store_falls_back_to_empty_instead_of_failing() {
        let path = temp_store("corrupt");
        std::fs::write(&path, "{not json").expect("write");
        // Reading must not panic and must not trust a half-written file.
        assert!(load_known_host(&path, "example:22").is_none());
        // A save repairs the file so the next read works.
        save_known_host(&path, "example:22", "SHA256:xyz").expect("save");
        assert_eq!(
            load_known_host(&path, "example:22").as_deref(),
            Some("SHA256:xyz")
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn describe_error_includes_context_chain() {
        let err = anyhow!("handshake failed").context("connecting to 10.0.0.1:22");
        let text = describe_error(&err);
        assert!(text.contains("connecting to 10.0.0.1:22"), "{text}");
        assert!(text.contains("handshake failed"), "{text}");
    }
}
