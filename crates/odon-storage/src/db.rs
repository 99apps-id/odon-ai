use chrono::{DateTime, Utc};
use odon_core::{
    Agent, AgentInboxMessage, AgentSchedule, Artifact, ArtifactKind, ArtifactPayload, Message,
    MessageContent, MessageRole, Session, SessionMode, SessionStatus,
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

            CREATE TABLE IF NOT EXISTS agents (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                role TEXT NOT NULL,
                color TEXT NOT NULL DEFAULT '#557a95',
                system_prompt TEXT NOT NULL DEFAULT '',
                model TEXT NOT NULL DEFAULT 'odon-auto',
                workspace_root TEXT,
                skills_json TEXT NOT NULL DEFAULT '[]',
                schedule_json TEXT NOT NULL DEFAULT '{}',
                enabled INTEGER NOT NULL DEFAULT 1,
                sort_order INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS agent_inbox (
                id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                channel TEXT NOT NULL,
                sender_id TEXT NOT NULL,
                session_id TEXT,
                text TEXT NOT NULL,
                queued_at TEXT NOT NULL,
                released_at TEXT,
                FOREIGN KEY (agent_id) REFERENCES agents (id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_agent_inbox_pending
                ON agent_inbox (agent_id, released_at);
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
                "provider_configs",
                "context_tokens",
                "ALTER TABLE provider_configs ADD COLUMN context_tokens INTEGER",
            ),
            (
                "sessions",
                "metadata",
                "ALTER TABLE sessions ADD COLUMN metadata TEXT",
            ),
            (
                "sessions",
                "agent_id",
                "ALTER TABLE sessions ADD COLUMN agent_id TEXT",
            ),
        ] {
            ensure_column(&conn, table, column, ddl)?;
        }
        // The index lives outside the batch: on a legacy database the column
        // above is added first, so creating the index inside the batch would
        // reference a column that does not exist yet.
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_sessions_agent ON sessions (agent_id)",
            [],
        )?;
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

    /// Removes the session's plan.
    ///
    /// A plan is per-turn working state: once every item is done it is cleared
    /// so the next turn does not inherit a finished list in the harness or the
    /// UI. Paused and stopped turns keep it so they stay resumable.
    pub fn clear_todos(&self, session_id: &str) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "DELETE FROM todos WHERE session_id = ?1",
            params![session_id],
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
            "INSERT INTO sessions (id, title, mode, status, active_model, workspace_root, agent_id, metadata, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                mode = excluded.mode,
                status = excluded.status,
                active_model = excluded.active_model,
                workspace_root = excluded.workspace_root,
                agent_id = excluded.agent_id,
                metadata = excluded.metadata,
                updated_at = excluded.updated_at",
            params![
                session.id,
                session.title,
                mode_str,
                status_str,
                session.active_model,
                session.workspace_root,
                session.agent_id,
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
            "SELECT id, title, mode, status, active_model, workspace_root, agent_id, metadata, created_at, updated_at
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let mode_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let _metadata: Option<String> = row.get(6)?;
            let created_at_str: String = row.get(8)?;
            let updated_at_str: String = row.get(9)?;

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
                agent_id: row.get(6)?,
                metadata: row
                    .get(7)
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
            "SELECT id, title, mode, status, active_model, workspace_root, agent_id, metadata, created_at, updated_at
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let mode_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let active_model: String = row.get(4)?;
            let workspace_root: Option<String> = row.get(5)?;
            let agent_id: Option<String> = row.get(6)?;
            let metadata: Option<String> = row.get(7)?;
            let created_at_str: String = row.get(8)?;
            let updated_at_str: String = row.get(9)?;

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
                agent_id,
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
                    thought_signature: None,
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
            let message = r?;
            // A stored assistant row with neither prose nor a tool call cannot
            // be represented on the wire: strict endpoints answer 400
            // "content or tool_calls must be set" and the whole session stays
            // broken for every later model. Such rows only ever came from a
            // cancelled or reasoning-only step, so they are hidden here too —
            // the UI shows a blank bubble for them otherwise.
            if message.role == MessageRole::Assistant
                && message.content.text.trim().is_empty()
                && message.content.tool_calls.is_empty()
                && message.content.attachments.is_empty()
            {
                continue;
            }
            messages.push(message);
        }
        Ok(messages)
    }

    /// Deletes assistant rows that carry neither prose nor a tool call.
    ///
    /// These rows are unusable on the wire and only exist in stores written
    /// before the write path refused to create them. Returns how many were
    /// removed, so the daemon can report a one-time repair instead of silently
    /// rewriting history.
    pub fn drop_empty_assistant_messages(&self) -> Result<usize> {
        let conn = self.lock_conn()?;
        let mut stmt =
            conn.prepare("SELECT id, content_json FROM messages WHERE role = 'assistant'")?;
        let candidates = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        drop(stmt);

        let empty: Vec<String> = candidates
            .into_iter()
            .filter(|(_, content_json)| {
                match serde_json::from_str::<MessageContent>(content_json) {
                    Ok(content) => {
                        content.text.trim().is_empty()
                            && content.tool_calls.is_empty()
                            && content.attachments.is_empty()
                    }
                    Err(_) => false,
                }
            })
            .map(|(id, _)| id)
            .collect();
        if empty.is_empty() {
            return Ok(0);
        }
        for id in &empty {
            conn.execute("DELETE FROM messages WHERE id = ?1", params![id])?;
        }
        Ok(empty.len())
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
            params![
                uuid::Uuid::new_v4().to_string(),
                key,
                normalize_memory_value(value),
                category,
                now
            ],
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
    /// plumbing (`plan_mode`, `todos`) and `lessons`.
    ///
    /// Lessons are excluded because they get their own harness section; keeping
    /// them here too printed the same five lines twice in every system prompt.
    pub fn list_user_memories(&self, limit: usize) -> Result<Vec<MemoryItem>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT key, value, category, updated_at FROM memories \
             WHERE category NOT IN ('plan_mode', 'todos', 'lessons') \
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

    /// Row counts per category, newest-first ordering ignored. Used by `doctor`
    /// and by tests to prove pruning actually bounded growth.
    pub fn memory_counts(&self) -> Result<Vec<(String, i64)>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT category, COUNT(*) FROM memories GROUP BY category ORDER BY COUNT(*) DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut counts = Vec::new();
        for row in rows {
            counts.push(row?);
        }
        Ok(counts)
    }

    /// Bound memory growth: keep the `per_category_keep` newest rows of each
    /// content category and at most `total_keep` rows overall, evicting the
    /// oldest surplus. Per-session plumbing (`plan_mode`, `todos`) is pruned
    /// separately by session and is left alone here.
    ///
    /// Returns `(evicted_total, evicted_per_category)` for logging.
    pub fn prune_memories(&self, per_category_keep: usize, total_keep: usize) -> Result<usize> {
        if per_category_keep == 0 || total_keep == 0 {
            return Ok(0);
        }
        let conn = self.lock_conn()?;
        let mut evicted = 0usize;

        // Fast path: nothing can be over the per-category ceiling if the whole
        // content table is under it. Keeps the harness build (which prunes) cheap.
        let content_rows: i64 = conn.query_row(
            "SELECT COUNT(*) FROM memories WHERE category NOT IN ('plan_mode', 'todos')",
            [],
            |row| row.get(0),
        )?;
        if content_rows <= per_category_keep as i64 {
            return Ok(0);
        }

        // 1. Per-category ceiling, so one noisy category (say `scratchpad`, or a
        //    provider that fails in many ways) cannot fill the whole table.
        let categories: Vec<String> = {
            let mut stmt = conn.prepare(
                "SELECT category FROM memories \
                 WHERE category NOT IN ('plan_mode', 'todos') \
                 GROUP BY category HAVING COUNT(*) > ?1",
            )?;
            let rows = stmt.query_map(params![per_category_keep as i64], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<std::result::Result<Vec<_>, _>>()?
        };
        for category in categories {
            evicted += conn.execute(
                "DELETE FROM memories WHERE key IN (
                     SELECT key FROM memories WHERE category = ?1
                     ORDER BY updated_at DESC LIMIT -1 OFFSET ?2
                 )",
                params![category, per_category_keep as i64],
            )?;
        }

        // 2. Global ceiling across content rows, oldest first.
        evicted += conn.execute(
            "DELETE FROM memories WHERE key IN (
                 SELECT key FROM memories
                 WHERE category NOT IN ('plan_mode', 'todos')
                 ORDER BY updated_at DESC LIMIT -1 OFFSET ?1
             )",
            params![total_keep as i64],
        )?;

        Ok(evicted)
    }

    // --- Provider Config Operations (BYOK persistence) ---

    #[expect(
        clippy::too_many_arguments,
        reason = "provider config is stored as one row; the context window is part of that row"
    )]
    pub fn save_provider_config(
        &self,
        provider_id: &str,
        api_key: &str,
        base_url: Option<&str>,
        is_default: bool,
        name: Option<&str>,
        models_json: &str,
        context_tokens: Option<i64>,
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
            "INSERT INTO provider_configs (provider_id, api_key, base_url, is_default, name, models_json, context_tokens, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(provider_id) DO UPDATE SET
                api_key = excluded.api_key,
                base_url = excluded.base_url,
                is_default = excluded.is_default,
                name = excluded.name,
                models_json = excluded.models_json,
                context_tokens = excluded.context_tokens,
                updated_at = excluded.updated_at",
            params![
                provider_id,
                api_key,
                base_url,
                if is_default { 1 } else { 0 },
                name,
                models_json,
                context_tokens,
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_provider_config(&self, provider_id: &str) -> Result<Option<ProviderConfigRecord>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT provider_id, api_key, base_url, is_default, name, models_json, context_tokens, updated_at
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
                context_tokens: row.get(6)?,
                updated_at: row.get(7)?,
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
            "SELECT provider_id, api_key, base_url, is_default, name, models_json, context_tokens, updated_at
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
                context_tokens: row.get(6)?,
                updated_at: row.get(7)?,
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

    // --- Agent Operations (persisted office roster) ---

    pub fn save_agent(&self, agent: &Agent) -> Result<()> {
        let conn = self.lock_conn()?;
        let skills_json = serde_json::to_string(&agent.skills)?;
        let schedule_json = serde_json::to_string(&agent.schedule)?;
        conn.execute(
            "INSERT INTO agents (id, name, role, color, system_prompt, model, workspace_root, skills_json, schedule_json, enabled, sort_order, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
             ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                role = excluded.role,
                color = excluded.color,
                system_prompt = excluded.system_prompt,
                model = excluded.model,
                workspace_root = excluded.workspace_root,
                skills_json = excluded.skills_json,
                schedule_json = excluded.schedule_json,
                enabled = excluded.enabled,
                sort_order = excluded.sort_order,
                updated_at = excluded.updated_at",
            params![
                agent.id,
                agent.name,
                agent.role,
                agent.color,
                agent.system_prompt,
                agent.model,
                agent.workspace_root,
                skills_json,
                schedule_json,
                if agent.enabled { 1 } else { 0 },
                agent.sort_order,
                agent.created_at.to_rfc3339(),
                agent.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_agent(&self, id: &str) -> Result<Agent> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, role, color, system_prompt, model, workspace_root, skills_json, schedule_json, enabled, sort_order, created_at, updated_at
             FROM agents WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => agent_from_row(row),
            None => Err(StorageError::NotFound(format!("Agent {}", id))),
        }
    }

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, name, role, color, system_prompt, model, workspace_root, skills_json, schedule_json, enabled, sort_order, created_at, updated_at
             FROM agents ORDER BY sort_order ASC, name ASC",
        )?;
        let mut rows = stmt.query([])?;
        let mut agents = Vec::new();
        while let Some(row) = rows.next()? {
            agents.push(agent_from_row(row)?);
        }
        Ok(agents)
    }

    pub fn delete_agent(&self, id: &str) -> Result<bool> {
        let conn = self.lock_conn()?;
        let count = conn.execute("DELETE FROM agents WHERE id = ?1", params![id])?;
        Ok(count > 0)
    }

    /// Parks a channel message for an off-shift agent.
    pub fn queue_agent_message(&self, message: &AgentInboxMessage) -> Result<()> {
        let conn = self.lock_conn()?;
        conn.execute(
            "INSERT INTO agent_inbox (id, agent_id, channel, sender_id, session_id, text, queued_at, released_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, NULL)",
            params![
                message.id,
                message.agent_id,
                message.channel,
                message.sender_id,
                message.session_id,
                message.text,
                message.queued_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// The still-parked messages for one agent, oldest first.
    pub fn queued_agent_messages(&self, agent_id: &str) -> Result<Vec<AgentInboxMessage>> {
        let conn = self.lock_conn()?;
        let mut stmt = conn.prepare(
            "SELECT id, agent_id, channel, sender_id, session_id, text, queued_at
             FROM agent_inbox
             WHERE agent_id = ?1 AND released_at IS NULL
             ORDER BY queued_at ASC",
        )?;
        let rows = stmt.query_map(params![agent_id], |row| {
            let queued: String = row.get(6)?;
            Ok(AgentInboxMessage {
                id: row.get(0)?,
                agent_id: row.get(1)?,
                channel: row.get(2)?,
                sender_id: row.get(3)?,
                session_id: row.get(4)?,
                text: row.get(5)?,
                queued_at: DateTime::parse_from_rfc3339(&queued)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
            })
        })?;
        rows.collect::<std::result::Result<Vec<_>, _>>()
            .map_err(StorageError::Sqlite)
    }

    /// Marks parked messages as released; returns how many rows moved.
    pub fn mark_agent_inbox_released(&self, ids: &[String]) -> Result<usize> {
        let conn = self.lock_conn()?;
        let now = Utc::now().to_rfc3339();
        let mut released = 0;
        for id in ids {
            released += conn.execute(
                "UPDATE agent_inbox SET released_at = ?2 WHERE id = ?1 AND released_at IS NULL",
                params![id, now],
            )?;
        }
        Ok(released)
    }
}

fn agent_from_row(row: &rusqlite::Row<'_>) -> Result<Agent> {
    let skills_json: String = row.get(7)?;
    let schedule_json: String = row.get(8)?;
    let enabled: i64 = row.get(9)?;
    let created_at_str: String = row.get(11)?;
    let updated_at_str: String = row.get(12)?;
    let parse_time = |value: &str| {
        DateTime::parse_from_rfc3339(value)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now())
    };
    Ok(Agent {
        id: row.get(0)?,
        name: row.get(1)?,
        role: row.get(2)?,
        color: row.get(3)?,
        system_prompt: row.get(4)?,
        model: row.get(5)?,
        workspace_root: row.get(6)?,
        skills: serde_json::from_str(&skills_json).unwrap_or_default(),
        schedule: serde_json::from_str(&schedule_json).unwrap_or_else(|_| AgentSchedule::default()),
        enabled: enabled != 0,
        sort_order: row.get(10)?,
        created_at: parse_time(&created_at_str),
        updated_at: parse_time(&updated_at_str),
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProviderConfigRecord {
    pub provider_id: String,
    pub api_key: String,
    pub base_url: Option<String>,
    pub is_default: bool,
    pub name: Option<String>,
    pub models_json: String,
    /// Context window (tokens) the operator declared for this endpoint. Used
    /// for models that carry no per-model value, so a gateway serving more
    /// than the conservative fallback is not capped by it.
    pub context_tokens: Option<i64>,
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

/// Hard ceiling on one stored memory value.
///
/// Memory is context, not a document store: a value longer than this is a
/// transcript or a tool output pasted in by mistake, and storing it verbatim
/// means every later harness render pays for it.
pub const MAX_MEMORY_VALUE_CHARS: usize = 1_000;
/// Rows kept per memory category. One noisy category (a provider that fails in
/// many ways, a scratchpad) must not be able to fill the table and then push
/// the useful project facts out of the prompt.
pub const MAX_MEMORIES_PER_CATEGORY: usize = 80;
/// Hard ceiling on the whole content table, so the harness query stays cheap on
/// a long-lived install.
pub const MAX_MEMORIES_TOTAL: usize = 400;

/// Normalise a value before it hits the database: collapse newlines (so a stored
/// memory cannot impersonate a harness section) and bound the length, saying so
/// instead of cutting silently.
fn normalize_memory_value(value: &str) -> String {
    let collapsed = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let length = collapsed.chars().count();
    if length <= MAX_MEMORY_VALUE_CHARS {
        return collapsed;
    }
    let kept: String = collapsed.chars().take(MAX_MEMORY_VALUE_CHARS).collect();
    format!(
        "{kept}… [{} more characters truncated on store]",
        length - MAX_MEMORY_VALUE_CHARS
    )
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
    fn agents_round_trip_and_sessions_keep_agent_id() {
        let storage = StorageEngine::new_in_memory().unwrap();
        let mut agent = Agent::new("Coding", "Software Engineering");
        agent.color = "#123456".to_string();
        agent.system_prompt = "Own the code.".to_string();
        agent.skills = vec!["rust".to_string(), "testing".to_string()];
        agent.schedule.days = vec![1, 3, 5];
        storage.save_agent(&agent).unwrap();

        let loaded = storage.get_agent(&agent.id).unwrap();
        assert_eq!(loaded.name, "Coding");
        assert_eq!(loaded.role, "Software Engineering");
        assert_eq!(loaded.skills, vec!["rust", "testing"]);
        assert_eq!(loaded.schedule.days, vec![1, 3, 5]);
        assert!(loaded.enabled);

        let mut session = Session::new("Agent thread", SessionMode::General, "odon-auto");
        session.agent_id = Some(agent.id.clone());
        storage.save_session(&session).unwrap();

        let stored = storage.get_session(&session.id).unwrap();
        assert_eq!(stored.agent_id.as_deref(), Some(agent.id.as_str()));
        let listed = storage.list_sessions().unwrap();
        assert!(listed
            .iter()
            .any(|row| row.agent_id.as_deref() == Some(agent.id.as_str())));

        assert_eq!(storage.list_agents().unwrap().len(), 1);

        // Off-shift inbox: queue two, drain them, and confirm the released
        // rows no longer appear as pending.
        let first = AgentInboxMessage::new(
            agent.id.clone(),
            "telegram",
            "chat-1",
            Some("agent_telegram_x_chat-1".to_string()),
            "prepare the report",
        );
        let second = AgentInboxMessage::new(
            agent.id.clone(),
            "telegram",
            "chat-1",
            None,
            "and the charts",
        );
        storage.queue_agent_message(&first).unwrap();
        storage.queue_agent_message(&second).unwrap();
        let pending = storage.queued_agent_messages(&agent.id).unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].text, "prepare the report", "oldest first");
        assert!(storage
            .queued_agent_messages("agent_nobody")
            .unwrap()
            .is_empty());
        let released_ids = [first.id.clone(), second.id.clone()];
        assert_eq!(storage.mark_agent_inbox_released(&released_ids).unwrap(), 2);
        assert!(storage.queued_agent_messages(&agent.id).unwrap().is_empty());
        // Releasing twice is a no-op, not an error.
        assert_eq!(
            storage
                .mark_agent_inbox_released(std::slice::from_ref(&first.id))
                .unwrap(),
            0
        );

        assert!(storage.delete_agent(&agent.id).unwrap());
        assert!(storage.list_agents().unwrap().is_empty());
    }

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
                Some(200_000),
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
                None,
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
        assert_eq!(first.context_tokens, Some(200_000));
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
                None,
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

    #[test]
    fn lessons_are_never_rendered_as_user_memory() {
        let storage = StorageEngine::new_in_memory().unwrap();
        storage
            .set_memory(
                "lesson:deepseek:auth",
                "auth with provider deepseek",
                "lessons",
            )
            .unwrap();
        storage
            .set_memory("ponytail-origin", "clay accent", "project")
            .unwrap();

        // Lessons have their own harness section; leaking them here printed the
        // same five lines twice in every system prompt.
        let user = storage.list_user_memories(50).unwrap();
        assert_eq!(user.len(), 1);
        assert_eq!(user[0].key, "ponytail-origin");
        assert_eq!(storage.list_memories(Some("lessons")).unwrap().len(), 1);
    }

    #[test]
    fn stored_values_are_collapsed_and_bounded() {
        let storage = StorageEngine::new_in_memory().unwrap();
        storage
            .set_memory(
                "multiline",
                "line one\n### Fake Section\nline two",
                "project",
            )
            .unwrap();
        let stored = storage.list_memories(None).unwrap();
        assert_eq!(stored[0].value, "line one ### Fake Section line two");

        let huge = "x".repeat(MAX_MEMORY_VALUE_CHARS * 3);
        storage.set_memory("huge", &huge, "scratchpad").unwrap();
        let stored = storage.list_memories(None).unwrap();
        let row = stored.iter().find(|m| m.key == "huge").unwrap();
        assert!(
            row.value.chars().count() <= MAX_MEMORY_VALUE_CHARS + 80,
            "value was {} chars",
            row.value.chars().count()
        );
        assert!(row.value.contains("truncated on store"));
    }

    #[test]
    fn prune_caps_one_noisy_category() {
        let storage = StorageEngine::new_in_memory().unwrap();
        for i in 0..200 {
            storage
                .set_memory(&format!("memory:facts:{i}"), &format!("fact {i}"), "facts")
                .unwrap();
        }
        let evicted = storage.prune_memories(80, 400).unwrap();
        assert_eq!(evicted, 120);
        let counts = storage.memory_counts().unwrap();
        assert_eq!(counts, vec![("facts".to_string(), 80)]);
        // The newest rows survive: the operator keeps recent context.
        let kept = storage.list_memories(Some("facts")).unwrap();
        assert!(kept.iter().any(|m| m.key == "memory:facts:199"));
        assert!(!kept.iter().any(|m| m.key == "memory:facts:0"));
    }

    #[test]
    fn prune_caps_the_total_across_categories() {
        let storage = StorageEngine::new_in_memory().unwrap();
        for category in ["facts", "project", "decision", "preference", "scratchpad"] {
            for i in 0..40 {
                storage
                    .set_memory(
                        &format!("{category}:{i}"),
                        &format!("{category} {i}"),
                        category,
                    )
                    .unwrap();
            }
        }
        storage.prune_memories(80, 60).unwrap();
        let total: i64 = storage
            .memory_counts()
            .unwrap()
            .iter()
            .map(|(_, n)| *n)
            .sum();
        assert_eq!(total, 60, "global ceiling must bound the table");
    }

    #[test]
    fn prune_leaves_session_plumbing_alone() {
        let storage = StorageEngine::new_in_memory().unwrap();
        for i in 0..30 {
            storage
                .set_memory(&format!("plan_mode:{i}"), "false", "plan_mode")
                .unwrap();
            storage
                .set_memory(&format!("todos:{i}"), "[]", "todos")
                .unwrap();
        }
        for i in 0..120 {
            storage
                .set_memory(&format!("memory:facts:{i}"), &format!("fact {i}"), "facts")
                .unwrap();
        }
        storage.prune_memories(50, 100).unwrap();
        let counts = storage.memory_counts().unwrap();
        let plumbing: i64 = counts
            .iter()
            .filter(|(category, _)| category == "plan_mode" || category == "todos")
            .map(|(_, n)| *n)
            .sum();
        assert_eq!(plumbing, 60, "per-session state must survive pruning");
    }

    #[test]
    fn prune_is_a_no_op_below_the_ceiling() {
        let storage = StorageEngine::new_in_memory().unwrap();
        for i in 0..10 {
            storage
                .set_memory(&format!("memory:facts:{i}"), &format!("fact {i}"), "facts")
                .unwrap();
        }
        assert_eq!(storage.prune_memories(80, 400).unwrap(), 0);
        assert_eq!(storage.prune_memories(0, 0).unwrap(), 0);
    }

    #[test]
    fn empty_assistant_rows_are_hidden_and_repairable() {
        let storage = StorageEngine::new_in_memory().unwrap();
        let session = Session::new("Poison", SessionMode::Code, "deepseek-flash");
        storage.save_session(&session).unwrap();
        storage
            .save_message(&Message::new_user(&session.id, "hi".to_string()))
            .unwrap();
        // A row the current write path refuses to create, written directly so
        // the repair path has something to find.
        let mut poison = Message::new_assistant(&session.id, String::new(), None, None);
        poison.content.text = "   ".to_string();
        storage.save_message(&poison).unwrap();
        // A tool-call-only step is legitimate and must survive.
        let mut step = Message::new_assistant(&session.id, String::new(), None, None);
        step.content.tool_calls.push(odon_core::ToolCallInfo {
            call_id: "call-1".to_string(),
            tool_name: "fs_read".to_string(),
            arguments: serde_json::json!({}),
            output: None,
            is_error: false,
            requires_approval: false,
            is_approved: None,
        });
        storage.save_message(&step).unwrap();

        let visible = storage.list_messages(&session.id).unwrap();
        assert_eq!(visible.len(), 2, "the empty row must not be replayed");
        assert!(visible.iter().all(|m| m.id != poison.id));
        assert_eq!(storage.drop_empty_assistant_messages().unwrap(), 1);
        assert_eq!(storage.list_messages(&session.id).unwrap().len(), 2);
        assert_eq!(storage.drop_empty_assistant_messages().unwrap(), 0);
    }

    #[test]
    fn clearing_a_plan_leaves_other_sessions_alone() {
        let storage = StorageEngine::new_in_memory().unwrap();
        let done = Session::new("Done", SessionMode::Code, "m");
        let running = Session::new("Running", SessionMode::Code, "m");
        storage.save_session(&done).unwrap();
        storage.save_session(&running).unwrap();
        storage
            .set_todos(
                &done.id,
                &serde_json::json!([{ "title": "ship it", "status": "completed" }]),
            )
            .unwrap();
        storage
            .set_todos(
                &running.id,
                &serde_json::json!([{ "title": "still going", "status": "in_progress" }]),
            )
            .unwrap();

        storage.clear_todos(&done.id).unwrap();

        assert_eq!(storage.get_todos(&done.id).unwrap(), serde_json::json!([]));
        assert_eq!(
            storage
                .get_todos(&running.id)
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1,
            "another session's plan must survive"
        );
    }
}
