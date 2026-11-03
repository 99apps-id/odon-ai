use chrono::{DateTime, Utc};
use odon_core::{
    Artifact, ArtifactKind, ArtifactPayload, Message, MessageContent, MessageRole, Session,
    SessionMode, SessionStatus,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum StorageError {
    #[error("Database error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Entity not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, StorageError>;

#[derive(Clone)]
pub struct StorageEngine {
    conn: Arc<Mutex<Connection>>,
    _db_path: PathBuf,
}

impl StorageEngine {
    fn apply_pragmas(conn: &Connection) -> Result<()> {
        conn.execute_batch(
            "PRAGMA foreign_keys = ON;
             PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;",
        )?;
        Ok(())
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| StorageError::NotFound("storage lock poisoned".to_string()))
    }

    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::apply_pragmas(&conn)?;
        let engine = Self {
            conn: Arc::new(Mutex::new(conn)),
            _db_path: PathBuf::from(":memory:"),
        };
        engine.init_schema()?;
        Ok(engine)
    }

    pub fn new_at_path(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(&path)?;
        Self::apply_pragmas(&conn)?;
        let engine = Self {
            conn: Arc::new(Mutex::new(conn)),
            _db_path: path,
        };
        engine.init_schema()?;
        Ok(engine)
    }

    pub fn default_storage() -> Result<Self> {
        // `ODON_HOME` keeps the database next to the rest of the daemon state
        // (OAuth tokens, automations, MCP configs), which is what tests and
        // portable installs expect. Without it, keep the historical location.
        let base_dir = match std::env::var_os("ODON_HOME") {
            Some(home) => PathBuf::from(home).join(".odon"),
            None => dirs::data_local_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("odon"),
        };
        std::fs::create_dir_all(&base_dir)?;
        Self::new_at_path(base_dir.join("odon_state.db"))
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                active_model TEXT NOT NULL,
                workspace_root TEXT,
                metadata TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content_json TEXT NOT NULL,
                model TEXT,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS artifacts (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                title TEXT NOT NULL,
                kind TEXT NOT NULL,
                summary TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS artifact_versions (
                artifact_id TEXT NOT NULL,
                version INTEGER NOT NULL,
                title TEXT NOT NULL,
                kind TEXT NOT NULL,
                summary TEXT NOT NULL,
                payload_json TEXT NOT NULL,
                created_at TEXT NOT NULL,
                PRIMARY KEY (artifact_id, version),
                FOREIGN KEY (artifact_id) REFERENCES artifacts (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS audit_logs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                risk_tier INTEGER NOT NULL,
                description TEXT NOT NULL,
                approved INTEGER NOT NULL,
                success INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                timestamp TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages (session_id);
            CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts (session_id);

            CREATE TABLE IF NOT EXISTS memories (
                id TEXT PRIMARY KEY,
                key TEXT NOT NULL UNIQUE,
                value TEXT NOT NULL,
                category TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS todos (
                session_id TEXT PRIMARY KEY,
                items_json TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE CASCADE
            );

            CREATE TABLE IF NOT EXISTS provider_configs (
                provider_id TEXT PRIMARY KEY,
                api_key TEXT NOT NULL,
                base_url TEXT,
                is_default INTEGER DEFAULT 0,
                updated_at TEXT NOT NULL
            );
            ",
        )?;
        // Older databases predate columns such as `sessions.metadata` and
        // multi-endpoint provider metadata. `CREATE TABLE IF NOT EXISTS` leaves
        // an existing table untouched, so those columns must be added here;
        // SQLite has no IF NOT EXISTS for ADD COLUMN, so only add a column when
        // it is actually missing instead of swallowing every migration error.
        for (table, column, ddl) in [
            (
                "provider_configs",
                "name",
                "ALTER TABLE provider_configs ADD COLUMN name TEXT",
            ),
            (
                "provider_configs",
                "models_json",
                "ALTER TABLE provider_configs ADD COLUMN models_json TEXT NOT NULL DEFAULT '[]'",
            ),
            (
                "sessions",
                "metadata",
                "ALTER TABLE sessions ADD COLUMN metadata TEXT",
            ),
        ] {
            ensure_column(&conn, table, column, ddl)?;
        }
        // Best-effort cleanup for databases created before cascades existed.
        let _ = conn.execute(
            "DELETE FROM artifact_versions WHERE artifact_id NOT IN (SELECT id FROM artifacts)",
            [],
        );
        let _ = conn.execute(
            "DELETE FROM audit_logs WHERE session_id NOT IN (SELECT id FROM sessions)",
            [],
        );
        let _ = conn.execute(
            "DELETE FROM todos WHERE session_id NOT IN (SELECT id FROM sessions)",
            [],
        );
        Ok(())
    }

    // --- Todo Operations (per-session task plan) ---

    pub fn set_todos(&self, session_id: &str, items: &serde_json::Value) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO todos (session_id, items_json, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(session_id) DO UPDATE SET
                items_json = excluded.items_json,
                updated_at = excluded.updated_at",
            params![session_id, items.to_string(), Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn get_todos(&self, session_id: &str) -> Result<serde_json::Value> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare("SELECT items_json FROM todos WHERE session_id = ?1")?;
        let mut rows = stmt.query(params![session_id])?;
        if let Some(row) = rows.next()? {
            let raw: String = row.get(0)?;
            Ok(serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::json!([])))
        } else {
            Ok(serde_json::json!([]))
        }
    }

    // --- Session Operations ---

    pub fn save_session(&self, session: &Session) -> Result<()> {
        let conn = self.lock_conn()?;
        let mode_str = serde_json::to_string(&session.mode)?;
        let status_str = serde_json::to_string(&session.status)?;
        let metadata_json = match &session.metadata {
            Some(value) => Some(serde_json::to_string(value)?),
            None => None,
        };
        conn.execute(
            "INSERT INTO sessions (id, title, mode, status, active_model, workspace_root, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                mode = excluded.mode,
                status = excluded.status,
                active_model = excluded.active_model,
                workspace_root = excluded.workspace_root,
                metadata = excluded.metadata,
                updated_at = excluded.updated_at",
            params![
                session.id,
                session.title,
                mode_str,
                status_str,
                session.active_model,
                session.workspace_root,
                metadata_json,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, mode, status, active_model, workspace_root, metadata, created_at, updated_at
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let mode_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let _metadata: Option<String> = row.get(6)?;
            let created_at_str: String = row.get(7)?;
            let updated_at_str: String = row.get(8)?;

            let mode: SessionMode = serde_json::from_str(&mode_str)?;
            let status: SessionStatus = serde_json::from_str(&status_str)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });

            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                mode,
                status,
                active_model: row.get(4)?,
                workspace_root: row.get(5)?,
                metadata: row
                    .get(6)
                    .ok()
                    .and_then(|v: Option<String>| v.and_then(|s| serde_json::from_str(&s).ok())),
                created_at,
                updated_at,
            })
        } else {
            Err(StorageError::NotFound(format!("Session {}", id)))
        }
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, title, mode, status, active_model, workspace_root, metadata, created_at, updated_at
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let mode_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let active_model: String = row.get(4)?;
            let workspace_root: Option<String> = row.get(5)?;
            let metadata: Option<String> = row.get(6)?;
            let created_at_str: String = row.get(7)?;
            let updated_at_str: String = row.get(8)?;

            let mode: SessionMode = serde_json::from_str(&mode_str).unwrap_or(SessionMode::General);
            let status: SessionStatus =
                serde_json::from_str(&status_str).unwrap_or(SessionStatus::Active);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });

            Ok(Session {
                id,
                title,
                mode,
                status,
                active_model,
                workspace_root,
                metadata: metadata.and_then(|v| serde_json::from_str(&v).ok()),
                created_at,
                updated_at,
            })
        })?;

        let mut sessions = Vec::new();
        for r in rows {
            sessions.push(r?);
        }
        Ok(sessions)
    }

    // --- Message Operations ---

    pub fn save_message(&self, msg: &Message) -> Result<()> {
        let conn = self.lock_conn()?;
        let role_str = match msg.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        };
        let content_json = serde_json::to_string(&msg.content)?;
        conn.execute(
            "INSERT INTO messages (id, session_id, role, content_json, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                session_id = excluded.session_id,
                role = excluded.role,
                content_json = excluded.content_json,
                model = excluded.model",
            params![
                msg.id,
                msg.session_id,
                role_str,
                content_json,
                msg.model,
                msg.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn list_messages(&self, session_id: &str) -> Result<Vec<Message>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content_json, model, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC, rowid ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let role_str: String = row.get(2)?;
            let content_json: String = row.get(3)?;
            let model: Option<String> = row.get(4)?;
            let created_at_str: String = row.get(5)?;

            let role = match role_str.trim().trim_matches('"').to_lowercase().as_str() {
                "assistant" => MessageRole::Assistant,
                "system" => MessageRole::System,
                "tool" => MessageRole::Tool,
                _ => MessageRole::User,
            };
            let content: MessageContent =
                serde_json::from_str(&content_json).unwrap_or(MessageContent {
                    text: String::new(),
                    thought: None,
                    tool_calls: Vec::new(),
                    artifact_ids: Vec::new(),
                    action_chips: Vec::new(),
                    attachments: Vec::new(),
                });
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });

            Ok(Message {
                id,
                session_id,
                role,
                content,
                model,
                created_at,
            })
        })?;

        let mut messages = Vec::new();
        for r in rows {
            messages.push(r?);
        }
        Ok(messages)
    }

    // --- Artifact Operations ---

    pub fn save_artifact(&self, artifact: &Artifact) -> Result<()> {
        let conn = self.lock_conn()?;
        let kind_str = serde_json::to_string(&artifact.kind)?;
        let payload_json = serde_json::to_string(&artifact.payload)?;

        // `.ok()` here would also turn a genuine SQL failure into "no existing
        // row", silently treating the artifact as new; only absence is expected.
        let existing: Option<(String, String, String, String)> = conn
            .query_row(
                "SELECT title, kind, summary, payload_json FROM artifacts WHERE id = ?1",
                params![artifact.id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;

        conn.execute(
            "INSERT INTO artifacts (id, session_id, title, kind, summary, payload_json, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                kind = excluded.kind,
                summary = excluded.summary,
                payload_json = excluded.payload_json",
            params![
                artifact.id,
                artifact.session_id,
                artifact.title,
                kind_str,
                artifact.summary,
                payload_json,
                artifact.created_at.to_rfc3339(),
            ],
        )?;

        // Persist immutable versions whenever an artifact is created or actually
        // changes. This also backfills version 1 for pre-existing artifacts.
        let changed = match &existing {
            None => true,
            Some((old_title, old_kind, old_summary, old_payload)) => {
                old_title != &artifact.title
                    || old_kind != &kind_str
                    || old_summary != &artifact.summary
                    || old_payload != &payload_json
            }
        };
        if changed {
            let next_version: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version), 0) + 1 FROM artifact_versions WHERE artifact_id = ?1",
                params![artifact.id],
                |row| row.get(0),
            )?;
            conn.execute(
                "INSERT INTO artifact_versions
                    (artifact_id, version, title, kind, summary, payload_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    artifact.id,
                    next_version,
                    artifact.title,
                    kind_str,
                    artifact.summary,
                    payload_json,
                    Utc::now().to_rfc3339(),
                ],
            )?;
        }
        Ok(())
    }

    fn artifact_version_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ArtifactVersion> {
        let kind_str: String = row.get(3)?;
        let payload_json: String = row.get(5)?;
        let created_at_str: String = row.get(6)?;
        let kind: ArtifactKind = serde_json::from_str(&kind_str).unwrap_or(ArtifactKind::Document);
        let payload = serde_json::from_str(&payload_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                0,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })?;
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|value| value.with_timezone(&Utc))
            .unwrap_or_else(|_| {
                tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                Utc::now()
            });
        Ok(ArtifactVersion {
            artifact_id: row.get(0)?,
            version: row.get(1)?,
            title: row.get(2)?,
            kind,
            summary: row.get(4)?,
            payload,
            created_at,
        })
    }

    pub fn list_artifact_versions(&self, artifact_id: &str) -> Result<Vec<ArtifactVersion>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT artifact_id, version, title, kind, summary, payload_json, created_at
             FROM artifact_versions WHERE artifact_id = ?1 ORDER BY version DESC",
        )?;
        let rows = stmt.query_map(params![artifact_id], Self::artifact_version_from_row)?;
        let mut versions = Vec::new();
        for row in rows {
            versions.push(row?);
        }
        Ok(versions)
    }

    pub fn get_artifact_version(&self, artifact_id: &str, version: i64) -> Result<ArtifactVersion> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT artifact_id, version, title, kind, summary, payload_json, created_at
             FROM artifact_versions WHERE artifact_id = ?1 AND version = ?2",
        )?;
        let mut rows = stmt.query(params![artifact_id, version])?;
        if let Some(row) = rows.next()? {
            Ok(Self::artifact_version_from_row(row)?)
        } else {
            Err(StorageError::NotFound(format!(
                "Artifact {artifact_id} version {version}"
            )))
        }
    }

    pub fn restore_artifact_version(&self, artifact_id: &str, version: i64) -> Result<Artifact> {
        let old = self.get_artifact_version(artifact_id, version)?;
        let mut artifact = self.get_artifact(artifact_id)?;
        artifact.title = old.title;
        artifact.kind = old.kind;
        artifact.summary = old.summary;
        artifact.payload = old.payload;
        artifact.created_at = Utc::now();
        self.save_artifact(&artifact)?;
        Ok(artifact)
    }

    pub fn list_artifacts(&self, session_id: &str) -> Result<Vec<Artifact>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, title, kind, summary, payload_json, created_at
             FROM artifacts WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let title: String = row.get(2)?;
            let kind_str: String = row.get(3)?;
            let summary: String = row.get(4)?;
            let payload_json: String = row.get(5)?;
            let created_at_str: String = row.get(6)?;

            let kind: ArtifactKind =
                serde_json::from_str(&kind_str).unwrap_or(ArtifactKind::Document);
            let payload = serde_json::from_str(&payload_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });

            Ok(Artifact {
                id,
                session_id,
                title,
                kind,
                summary,
                payload,
                created_at,
            })
        })?;

        let mut artifacts = Vec::new();
        for r in rows {
            artifacts.push(r?);
        }
        Ok(artifacts)
    }

    pub fn get_artifact(&self, id: &str) -> Result<Artifact> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, session_id, title, kind, summary, payload_json, created_at
             FROM artifacts WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let title: String = row.get(2)?;
            let kind_str: String = row.get(3)?;
            let summary: String = row.get(4)?;
            let payload_json: String = row.get(5)?;
            let created_at_str: String = row.get(6)?;

            let kind: ArtifactKind =
                serde_json::from_str(&kind_str).unwrap_or(ArtifactKind::Document);
            let payload = serde_json::from_str(&payload_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });

            Ok(Artifact {
                id,
                session_id,
                title,
                kind,
                summary,
                payload,
                created_at,
            })
        } else {
            Err(StorageError::NotFound(format!("Artifact {}", id)))
        }
    }

    pub fn delete_artifact(&self, id: &str) -> Result<bool> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM artifact_versions WHERE artifact_id = ?1",
            params![id],
        )?;
        let count = tx.execute("DELETE FROM artifacts WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(count > 0)
    }

    pub fn delete_session(&self, id: &str) -> Result<bool> {
        let mut conn = self.lock_conn()?;
        let tx = conn.transaction()?;
        tx.execute("DELETE FROM messages WHERE session_id = ?1", params![id])?;
        tx.execute(
            "DELETE FROM artifact_versions WHERE artifact_id IN (SELECT id FROM artifacts WHERE session_id = ?1)",
            params![id],
        )?;
        tx.execute("DELETE FROM artifacts WHERE session_id = ?1", params![id])?;
        tx.execute("DELETE FROM audit_logs WHERE session_id = ?1", params![id])?;
        tx.execute("DELETE FROM todos WHERE session_id = ?1", params![id])?;
        let count = tx.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(count > 0)
    }

    // --- Memory Operations ---

    pub fn set_memory(&self, key: &str, value: &str, category: &str) -> Result<()> {
        let conn = self.lock_conn()?;
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO memories (id, key, value, category, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(key) DO UPDATE SET
                value = excluded.value,
                category = excluded.category,
                updated_at = excluded.updated_at",
            params![uuid::Uuid::new_v4().to_string(), key, value, category, now],
        )?;
        Ok(())
    }

    pub fn get_memory(&self, key: &str) -> Result<Option<MemoryItem>> {
        let conn = self.lock_conn()?;
        let mut stmt =
            conn.prepare("SELECT key, value, category, updated_at FROM memories WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let updated_at_str: String = row.get(3)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });
            Ok(Some(MemoryItem {
                key: row.get(0)?,
                value: row.get(1)?,
                category: row.get(2)?,
                updated_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_memories(&self, category: Option<&str>) -> Result<Vec<MemoryItem>> {
        let conn = self.lock_conn()?;
        let mut memories = Vec::new();
        if let Some(cat) = category {
            let mut stmt = conn.prepare(
                "SELECT key, value, category, updated_at FROM memories WHERE category = ?1 ORDER BY updated_at DESC"
            )?;
            let rows = stmt.query_map(params![cat], |row| {
                let updated_at_str: String = row.get(3)?;
                let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| {
                        tracing::warn!(
                            "corrupt timestamp in odon_state.db; substituting current time"
                        );
                        Utc::now()
                    });
                Ok(MemoryItem {
                    key: row.get(0)?,
                    value: row.get(1)?,
                    category: row.get(2)?,
                    updated_at,
                })
            })?;
            for r in rows {
                memories.push(r?);
            }
        } else {
            let mut stmt = conn.prepare(
                "SELECT key, value, category, updated_at FROM memories ORDER BY updated_at DESC",
            )?;
            let rows = stmt.query_map([], |row| {
                let updated_at_str: String = row.get(3)?;
                let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| {
                        tracing::warn!(
                            "corrupt timestamp in odon_state.db; substituting current time"
                        );
                        Utc::now()
                    });
                Ok(MemoryItem {
                    key: row.get(0)?,
                    value: row.get(1)?,
                    category: row.get(2)?,
                    updated_at,
                })
            })?;
            for r in rows {
                memories.push(r?);
            }
        }
        Ok(memories)
    }

    /// Memories worth showing to a model: everything except per-session harness
    /// plumbing (`plan_mode`, `todos`). Filtering happens in SQL on purpose: those
    /// rows are rewritten every turn, so trimming by recency before filtering would
    /// let plumbing crowd out the user's own memories.
    pub fn list_user_memories(&self, limit: usize) -> Result<Vec<MemoryItem>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, value, category, updated_at FROM memories \
             WHERE category NOT IN ('plan_mode', 'todos') \
             ORDER BY updated_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| {
            let updated_at_str: String = row.get(3)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| {
                    tracing::warn!("corrupt timestamp in odon_state.db; substituting current time");
                    Utc::now()
                });
            Ok(MemoryItem {
                key: row.get(0)?,
                value: row.get(1)?,
                category: row.get(2)?,
                updated_at,
            })
        })?;
        let mut memories = Vec::new();
        for r in rows {
            memories.push(r?);
        }
        Ok(memories)
    }

    pub fn delete_memory(&self, key: &str) -> Result<bool> {
        let conn = self.lock_conn()?;
        let count = conn.execute("DELETE FROM memories WHERE key = ?1", params![key])?;
        Ok(count > 0)
    }

    // --- Provider Config Operations (BYOK persistence) ---

    pub fn save_provider_config(
        &self,
        provider_id: &str,
        api_key: &str,
        base_url: Option<&str>,
        is_default: bool,
        name: Option<&str>,
        models_json: &str,
    ) -> Result<()> {
        let conn = self.lock_conn()?;
        let now = Utc::now().to_rfc3339();
        if is_default {
            conn.execute(
                "UPDATE provider_configs SET is_default = 0 WHERE provider_id != ?1",
                params![provider_id],
            )?;
        }
        conn.execute(
            "INSERT INTO provider_configs (provider_id, api_key, base_url, is_default, name, models_json, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(provider_id) DO UPDATE SET
                api_key = excluded.api_key,
                base_url = excluded.base_url,
                is_default = excluded.is_default,
                name = excluded.name,
                models_json = excluded.models_json,
                updated_at = excluded.updated_at",
            params![
                provider_id,
                api_key,
                base_url,
                if is_default { 1 } else { 0 },
                name,
                models_json,
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_provider_config(&self, provider_id: &str) -> Result<Option<ProviderConfigRecord>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT provider_id, api_key, base_url, is_default, name, models_json, updated_at
             FROM provider_configs WHERE provider_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![provider_id], |row| {
            let def_int: i32 = row.get(3)?;
            Ok(ProviderConfigRecord {
                provider_id: row.get(0)?,
                api_key: row.get(1)?,
                base_url: row.get(2)?,
                is_default: def_int != 0,
                name: row.get(4)?,
                models_json: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    pub fn list_provider_configs(&self) -> Result<Vec<ProviderConfigRecord>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT provider_id, api_key, base_url, is_default, name, models_json, updated_at
             FROM provider_configs ORDER BY provider_id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let def_int: i32 = row.get(3)?;
            Ok(ProviderConfigRecord {
                provider_id: row.get(0)?,
                api_key: row.get(1)?,
                base_url: row.get(2)?,
                is_default: def_int != 0,
                name: row.get(4)?,
                models_json: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn delete_provider_config(&self, provider_id: &str) -> Result<bool> {
        let conn = self.lock_conn()?;
        let count = conn.execute(
            "DELETE FROM provider_configs WHERE provider_id = ?1",
            params![provider_id],
        )?;
        Ok(count > 0)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfigRecord {
    pub provider_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub name: Option<String>,
    pub models_json: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ArtifactVersion {
    pub artifact_id: String,
    pub version: i64,
    pub title: String,
    pub kind: ArtifactKind,
    pub summary: String,
    pub payload: ArtifactPayload,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryItem {
    pub key: String,
    pub value: String,
    pub category: String,
    pub updated_at: DateTime<Utc>,
}

/// Adds a column to an existing table when the migration has not run yet.
///
/// `table` and `column` are compile-time literals from the migration list
/// above, never user input, so interpolating them into the pragma is safe.
fn ensure_column(conn: &rusqlite::Connection, table: &str, column: &str, ddl: &str) -> Result<()> {
    let mut stmt = conn.prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))?;
    let exists = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .any(|name| name.as_deref() == Ok(column));
    if !exists {
        conn.execute(ddl, [])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_sessions_table_gains_metadata_column() {
        // Reproduces an existing install whose `sessions` table was created
        // before `metadata` existed: opening it must migrate, not fail with
        // "table sessions has no column named metadata".
        let path = std::env::temp_dir().join(format!(
            "odon-legacy-{}-{}.db",
            std::process::id(),
            Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE sessions (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    mode TEXT NOT NULL,
                    status TEXT NOT NULL,
                    active_model TEXT NOT NULL,
                    workspace_root TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );",
            )
            .unwrap();
        }

        let storage = StorageEngine::new_at_path(&path).unwrap();
        let mut session = Session::new("Legacy row", SessionMode::General, "odon-auto");
        session.metadata = Some(serde_json::json!({ "source": "legacy-migration" }));
        storage.save_session(&session).unwrap();

        let loaded = storage.get_session(&session.id).unwrap();
        assert_eq!(
            loaded.metadata,
            Some(serde_json::json!({ "source": "legacy-migration" }))
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn artifact_versions_are_created_and_restorable() {
        let storage = StorageEngine::new_in_memory().unwrap();
        let session = Session::new("Version test", SessionMode::General, "odon-auto");
        storage.save_session(&session).unwrap();
        let mut artifact = Artifact::new_document(&session.id, "Spec", "PRD", "# v1");
        storage.save_artifact(&artifact).unwrap();

        artifact.payload = odon_core::ArtifactPayload::Document {
            doc_type: "PRD".to_string(),
            markdown: "# v2".to_string(),
            size_bytes: 4,
        };
        storage.save_artifact(&artifact).unwrap();

        let versions = storage.list_artifact_versions(&artifact.id).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].version, 2);
        assert_eq!(versions[1].version, 1);

        let restored = storage.restore_artifact_version(&artifact.id, 1).unwrap();
        assert_eq!(
            restored.payload,
            odon_core::ArtifactPayload::Document {
                doc_type: "PRD".to_string(),
                markdown: "# v1".to_string(),
                size_bytes: 4,
            }
        );
        let versions = storage.list_artifact_versions(&artifact.id).unwrap();
        assert_eq!(versions.len(), 3);
    }

    #[test]
    fn multiple_openai_compatible_endpoints_roundtrip() {
        let storage = StorageEngine::new_in_memory().unwrap();
        storage
            .save_provider_config(
                "compat-first",
                "sk-first",
                Some("https://first.example/v1"),
                true,
                Some("First Gateway"),
                r#"["model-a","model-b"]"#,
            )
            .unwrap();
        storage
            .save_provider_config(
                "compat-second",
                "sk-second",
                Some("https://second.example/v1"),
                false,
                Some("Second Gateway"),
                r#"["model-c"]"#,
            )
            .unwrap();

        let configs = storage.list_provider_configs().unwrap();
        assert_eq!(configs.len(), 2);
        let first = configs
            .iter()
            .find(|c| c.provider_id == "compat-first")
            .unwrap();
        assert_eq!(first.name.as_deref(), Some("First Gateway"));
        assert_eq!(first.models_json, r#"["model-a","model-b"]"#);
        assert!(first.is_default);
        assert!(
            !configs
                .iter()
                .find(|c| c.provider_id == "compat-second")
                .unwrap()
                .is_default
        );

        storage
            .save_provider_config(
                "compat-second",
                "sk-second",
                Some("https://second.example/v1"),
                true,
                Some("Second Gateway"),
                r#"["model-c"]"#,
            )
            .unwrap();
        let configs = storage.list_provider_configs().unwrap();
        assert!(
            !configs
                .iter()
                .find(|c| c.provider_id == "compat-first")
                .unwrap()
                .is_default
        );
        assert!(
            configs
                .iter()
                .find(|c| c.provider_id == "compat-second")
                .unwrap()
                .is_default
        );
    }

    #[test]
    fn user_memories_exclude_per_session_plumbing() {
        let storage = StorageEngine::new_in_memory().unwrap();

        // Per-session plumbing written by the harness tools.
        storage
            .set_memory("plan_mode:s1", "false", "plan_mode")
            .unwrap();
        storage
            .set_memory("todos:s1", r#"[{"status":"pending"}]"#, "todos")
            .unwrap();
        storage
            .set_memory("memory:facts:abc", "prefers tabs", "facts")
            .unwrap();
        storage
            .set_memory("ponytail-origin", "clay accent", "project")
            .unwrap();

        let all = storage.list_memories(None).unwrap();
        assert!(
            all.iter().any(|m| m.category == "plan_mode"),
            "plumbing rows are still readable for debugging"
        );

        let user = storage.list_user_memories(10).unwrap();
        let mut keys: Vec<&str> = user.iter().map(|m| m.key.as_str()).collect();
        keys.sort();
        assert_eq!(keys, vec!["memory:facts:abc", "ponytail-origin"]);
        assert!(
            user.iter()
                .all(|m| m.category != "plan_mode" && m.category != "todos"),
            "session plumbing must never reach the system prompt"
        );
    }

    #[test]
    fn user_memories_are_not_starved_by_plumbing_rows() {
        let storage = StorageEngine::new_in_memory().unwrap();

        // More plumbing rows than the prompt limit, all written after the
        // real memory so a naive ORDER BY created_at DESC LIMIT would drop it.
        storage
            .set_memory("memory:facts:real", "keep me", "facts")
            .unwrap();
        for i in 0..25 {
            storage
                .set_memory(&format!("plan_mode:{i}"), "false", "plan_mode")
                .unwrap();
        }

        let user = storage.list_user_memories(10).unwrap();
        assert_eq!(user.len(), 1);
        assert_eq!(user[0].key, "memory:facts:real");
    }
}
