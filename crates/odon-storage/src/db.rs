use chrono::{DateTime, Utc};
use odon_core::{Artifact, ArtifactKind, Message, MessageContent, MessageRole, Session, SessionMode, SessionStatus};
use rusqlite::{params, Connection};
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
    pub fn new_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
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
        let engine = Self {
            conn: Arc::new(Mutex::new(conn)),
            _db_path: path,
        };
        engine.init_schema()?;
        Ok(engine)
    }

    pub fn default_storage() -> Result<Self> {
        let base_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("odon");
        std::fs::create_dir_all(&base_dir)?;
        Self::new_at_path(base_dir.join("odon_state.db"))
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "
            CREATE TABLE IF NOT EXISTS sessions (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                mode TEXT NOT NULL,
                status TEXT NOT NULL,
                active_model TEXT NOT NULL,
                workspace_root TEXT,
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

            CREATE TABLE IF NOT EXISTS audit_logs (
                id TEXT PRIMARY KEY,
                session_id TEXT NOT NULL,
                tool_name TEXT NOT NULL,
                risk_tier INTEGER NOT NULL,
                description TEXT NOT NULL,
                approved INTEGER NOT NULL,
                success INTEGER NOT NULL,
                duration_ms INTEGER NOT NULL,
                timestamp TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_messages_session ON messages (session_id);
            CREATE INDEX IF NOT EXISTS idx_artifacts_session ON artifacts (session_id);
            ",
        )?;
        Ok(())
    }

    // --- Session Operations ---

    pub fn save_session(&self, session: &Session) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let mode_str = serde_json::to_string(&session.mode)?;
        let status_str = serde_json::to_string(&session.status)?;
        conn.execute(
            "INSERT INTO sessions (id, title, mode, status, active_model, workspace_root, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                title = excluded.title,
                mode = excluded.mode,
                status = excluded.status,
                active_model = excluded.active_model,
                workspace_root = excluded.workspace_root,
                updated_at = excluded.updated_at",
            params![
                session.id,
                session.title,
                mode_str,
                status_str,
                session.active_model,
                session.workspace_root,
                session.created_at.to_rfc3339(),
                session.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn get_session(&self, id: &str) -> Result<Session> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, mode, status, active_model, workspace_root, created_at, updated_at
             FROM sessions WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            let mode_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let created_at_str: String = row.get(6)?;
            let updated_at_str: String = row.get(7)?;

            let mode: SessionMode = serde_json::from_str(&mode_str)?;
            let status: SessionStatus = serde_json::from_str(&status_str)?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Session {
                id: row.get(0)?,
                title: row.get(1)?,
                mode,
                status,
                active_model: row.get(4)?,
                workspace_root: row.get(5)?,
                created_at,
                updated_at,
            })
        } else {
            Err(StorageError::NotFound(format!("Session {}", id)))
        }
    }

    pub fn list_sessions(&self) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, title, mode, status, active_model, workspace_root, created_at, updated_at
             FROM sessions ORDER BY updated_at DESC",
        )?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let title: String = row.get(1)?;
            let mode_str: String = row.get(2)?;
            let status_str: String = row.get(3)?;
            let active_model: String = row.get(4)?;
            let workspace_root: Option<String> = row.get(5)?;
            let created_at_str: String = row.get(6)?;
            let updated_at_str: String = row.get(7)?;

            let mode: SessionMode = serde_json::from_str(&mode_str).unwrap_or(SessionMode::General);
            let status: SessionStatus = serde_json::from_str(&status_str).unwrap_or(SessionStatus::Active);
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

            Ok(Session {
                id,
                title,
                mode,
                status,
                active_model,
                workspace_root,
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
        let conn = self.conn.lock().unwrap();
        let role_str = serde_json::to_string(&msg.role)?;
        let content_json = serde_json::to_string(&msg.content)?;
        conn.execute(
            "INSERT INTO messages (id, session_id, role, content_json, model, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, session_id, role, content_json, model, created_at
             FROM messages WHERE session_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![session_id], |row| {
            let id: String = row.get(0)?;
            let session_id: String = row.get(1)?;
            let role_str: String = row.get(2)?;
            let content_json: String = row.get(3)?;
            let model: Option<String> = row.get(4)?;
            let created_at_str: String = row.get(5)?;

            let role: MessageRole = serde_json::from_str(&role_str).unwrap_or(MessageRole::User);
            let content: MessageContent = serde_json::from_str(&content_json).unwrap_or(MessageContent {
                text: String::new(),
                thought: None,
                tool_calls: Vec::new(),
                artifact_ids: Vec::new(),
                action_chips: Vec::new(),
            });
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

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
        let conn = self.conn.lock().unwrap();
        let kind_str = serde_json::to_string(&artifact.kind)?;
        let payload_json = serde_json::to_string(&artifact.payload)?;
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
        Ok(())
    }

    pub fn list_artifacts(&self, session_id: &str) -> Result<Vec<Artifact>> {
        let conn = self.conn.lock().unwrap();
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

            let kind: ArtifactKind = serde_json::from_str(&kind_str).unwrap_or(ArtifactKind::Document);
            let payload = serde_json::from_str(&payload_json).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
            })?;
            let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());

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
}
