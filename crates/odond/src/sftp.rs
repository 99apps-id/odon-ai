//! SFTP browsing for the desktop file panel.
//!
//! The interactive terminal drives an SSH shell channel; the file panel needs a
//! second, independent channel: the `sftp` subsystem. Both share the same
//! handshake path ([`crate::ssh_session::authenticate`]) so host-key pinning and
//! public-key auth cannot drift between the two.
//!
//! Connections are cached per profile for a short idle window. Opening SFTP
//! costs a full handshake plus subsystem round-trip, which would make browsing
//! one directory per handshake feel sluggish; the TTL keeps an idle tab from
//! holding a session forever.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use russh::client::Handle;
use russh_sftp::client::SftpSession;
use serde::Serialize;

use crate::ssh_session::{authenticate, ConnectOptions, Verifier};

/// How long an idle cached connection is kept before it is dropped.
const IDLE_TTL: Duration = Duration::from_secs(120);

/// Largest file the editor will open. A file browser that streams a 2 GB log
/// into the UI helps nobody.
pub const MAX_EDIT_BYTES: u64 = 2 * 1024 * 1024;

pub struct Client {
    session: SftpSession,
    /// Keeps the transport alive for as long as the SFTP channel is used.
    _handle: Handle<Verifier>,
}

fn cache() -> &'static tokio::sync::Mutex<HashMap<String, (Arc<Client>, Instant)>> {
    static CACHE: OnceLock<tokio::sync::Mutex<HashMap<String, (Arc<Client>, Instant)>>> =
        OnceLock::new();
    CACHE.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

/// Return a cached SFTP client for `key`, or open one.
pub async fn client(key: &str, opts: &ConnectOptions) -> Result<Arc<Client>> {
    {
        let mut guard = cache().lock().await;
        guard.retain(|_, (_, seen)| seen.elapsed() < IDLE_TTL);
        if let Some((client, seen)) = guard.get_mut(key) {
            *seen = Instant::now();
            return Ok(client.clone());
        }
    }

    // Handshake outside the lock: it takes hundreds of milliseconds and other
    // profiles must not queue behind it.
    let authenticated = authenticate(opts).await?;
    let channel = authenticated
        .handle
        .channel_open_session()
        .await
        .context("open SFTP channel")?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .context("request sftp subsystem (does the server allow SFTP?)")?;
    let session = SftpSession::new(channel.into_stream())
        .await
        .context("start SFTP session")?;

    let client = Arc::new(Client {
        session,
        _handle: authenticated.handle,
    });
    cache()
        .lock()
        .await
        .insert(key.to_string(), (client.clone(), Instant::now()));
    Ok(client)
}

/// Drop every cached connection, e.g. after a profile is edited or removed.
pub async fn forget(key: &str) {
    cache().lock().await.remove(key);
}

/// One directory entry, shaped for the file panel.
#[derive(Debug, Serialize)]
pub struct Entry {
    pub name: String,
    pub path: String,
    /// `dir`, `file` or `link`.
    pub kind: String,
    pub size: u64,
    /// Octal permission string such as `0644`.
    pub mode: String,
    /// Seconds since the Unix epoch, when the server reports it.
    pub mtime: u64,
}

/// Collapse `//`, resolve `.`/`..` lexically and require an absolute result.
///
/// The remote is the user's own server, so this is not a security boundary like
/// the local workspace guard; it keeps paths canonical so the breadcrumb and
/// back/forward history compare equal instead of accumulating `/./` noise.
pub fn normalize_remote(path: &str) -> Result<String> {
    if path.trim().is_empty() {
        bail!("remote path is empty");
    }
    if path.contains('\0') {
        bail!("remote path contains a NUL byte");
    }
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if parts.pop().is_none() && !absolute {
                    bail!("remote path escapes its root: {path}");
                }
            }
            other => parts.push(other),
        }
    }
    if !absolute {
        bail!("remote path must be absolute: {path}");
    }
    Ok(format!("/{}", parts.join("/")))
}

/// Parent directory of a normalised absolute path (`/` for the root).
pub fn parent_remote(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) | None => "/".to_string(),
        Some((head, _)) => head.to_string(),
    }
}

fn kind_of(meta: &russh_sftp::protocol::FileAttributes) -> &'static str {
    use russh_sftp::protocol::FileType;
    match meta.file_type() {
        FileType::Dir => "dir",
        FileType::Symlink => "link",
        _ => "file",
    }
}

fn mode_of(meta: &russh_sftp::protocol::FileAttributes) -> String {
    format!("{:04o}", meta.permissions.unwrap_or(0) & 0o7777)
}

/// Resolve the session's home directory as an absolute remote path.
pub async fn home(client: &Client) -> Result<String> {
    let home = client
        .session
        .canonicalize(".")
        .await
        .context("resolve home directory")?;
    normalize_remote(&home)
}

pub async fn list(client: &Client, path: &str) -> Result<Vec<Entry>> {
    let dir = normalize_remote(path)?;
    let mut out = Vec::new();
    let entries = client
        .session
        .read_dir(dir.clone())
        .await
        .with_context(|| format!("list {dir}"))?;
    for entry in entries {
        let name = entry.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let meta = entry.metadata();
        let child = if dir == "/" {
            format!("/{name}")
        } else {
            format!("{dir}/{name}")
        };
        out.push(Entry {
            path: child,
            kind: kind_of(&meta).to_string(),
            size: meta.size.unwrap_or(0),
            mode: mode_of(&meta),
            mtime: meta.mtime.map(u64::from).unwrap_or(0),
            name,
        });
    }
    // Directories first, then names, so the panel is stable between reloads.
    out.sort_by(|a, b| {
        let rank = |e: &Entry| if e.kind == "dir" { 0 } else { 1 };
        rank(a)
            .cmp(&rank(b))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(out)
}

/// Read a text file for the editor, refusing oversized payloads.
pub async fn read_text(client: &Client, path: &str) -> Result<(String, bool)> {
    let file = normalize_remote(path)?;
    let meta = client
        .session
        .metadata(file.clone())
        .await
        .with_context(|| format!("stat {file}"))?;
    if kind_of(&meta) == "dir" {
        bail!("{file} is a directory");
    }
    let size = meta.size.unwrap_or(0);
    if size > MAX_EDIT_BYTES {
        bail!(
            "{file} is {size} bytes; the editor opens files up to {MAX_EDIT_BYTES} bytes"
        );
    }
    let bytes = client
        .session
        .read(file.clone())
        .await
        .with_context(|| format!("read {file}"))?;
    let truncated = bytes.len() as u64 > MAX_EDIT_BYTES;
    let slice = if truncated {
        &bytes[..MAX_EDIT_BYTES as usize]
    } else {
        &bytes[..]
    };
    Ok((String::from_utf8_lossy(slice).to_string(), truncated))
}

pub async fn write_text(client: &Client, path: &str, content: &str) -> Result<()> {
    let file = normalize_remote(path)?;
    // `SftpSession::write` opens with `WRITE` only: it refuses to create a new
    // file and leaves trailing bytes when the new content is shorter. Create
    // and truncate explicitly so saving behaves like an editor.
    use russh_sftp::protocol::OpenFlags;
    use tokio::io::AsyncWriteExt;
    let flags = OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE;
    let mut handle = client
        .session
        .open_with_flags(file.clone(), flags)
        .await
        .with_context(|| format!("open {file} for writing"))?;
    handle
        .write_all(content.as_bytes())
        .await
        .with_context(|| format!("write {file}"))?;
    handle.shutdown().await.ok();
    Ok(())
}

pub async fn create_dir(client: &Client, path: &str) -> Result<String> {
    let dir = normalize_remote(path)?;
    client
        .session
        .create_dir(dir.clone())
        .await
        .with_context(|| format!("create directory {dir}"))?;
    Ok(dir)
}

pub async fn rename(client: &Client, from: &str, to: &str) -> Result<String> {
    let from = normalize_remote(from)?;
    let to = normalize_remote(to)?;
    client
        .session
        .rename(from.clone(), to.clone())
        .await
        .with_context(|| format!("rename {from} to {to}"))?;
    Ok(to)
}

/// Remove a file, or a directory only when empty (never recursive by accident).
pub async fn delete(client: &Client, path: &str, is_dir: bool) -> Result<String> {
    let target = normalize_remote(path)?;
    if target == "/" {
        bail!("refusing to delete the remote root");
    }
    let result = if is_dir {
        client.session.remove_dir(target.clone()).await
    } else {
        client.session.remove_file(target.clone()).await
    };
    result.with_context(|| {
        if is_dir {
            format!("remove directory {target} (only empty directories can be removed)")
        } else {
            format!("remove file {target}")
        }
    })?;
    Ok(target)
}

/// Combine `dir` and `name` into an absolute remote path.
pub fn join_remote(dir: &str, name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(anyhow!("name is empty"));
    }
    if name.contains('/') {
        bail!("name must not contain a slash: {name}");
    }
    let dir = normalize_remote(dir)?;
    Ok(if dir == "/" {
        format!("/{name}")
    } else {
        format!("{dir}/{name}")
    })
}

/// Flatten an error together with its causes.
///
/// `to_string()` on a `anyhow::Error` prints only the outermost context
/// ("write /home/admin/x"), which hides the SFTP status that actually says why
/// ("permission denied"). `{:#}` keeps the chain readable in the panel.
pub fn error_text(err: &anyhow::Error) -> String {
    format!("{err:#}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_redundant_separators_and_dots() {
        assert_eq!(normalize_remote("/var//log/./nginx").unwrap(), "/var/log/nginx");
        assert_eq!(normalize_remote("/").unwrap(), "/");
    }

    #[test]
    fn resolves_dot_dot_lexically() {
        assert_eq!(normalize_remote("/home/admin/../ubuntu").unwrap(), "/home/ubuntu");
        assert_eq!(normalize_remote("/../../etc").unwrap(), "/etc");
    }

    #[test]
    fn rejects_relative_paths() {
        assert!(normalize_remote("etc/passwd").is_err());
        assert!(normalize_remote("   ").is_err());
        assert!(normalize_remote("/etc/\0x").is_err());
    }

    #[test]
    fn parent_of_root_is_root() {
        assert_eq!(parent_remote("/"), "/");
        assert_eq!(parent_remote("/home"), "/");
        assert_eq!(parent_remote("/home/admin/"), "/home");
    }

    #[test]
    fn joins_names_without_double_slashes() {
        assert_eq!(join_remote("/", "etc").unwrap(), "/etc");
        assert_eq!(join_remote("/home/admin", "a.txt").unwrap(), "/home/admin/a.txt");
        assert!(join_remote("/home", "a/b").is_err());
        assert!(join_remote("/home", "  ").is_err());
    }
}
