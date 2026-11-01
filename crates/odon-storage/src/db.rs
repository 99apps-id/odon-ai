use chrono::{DateTime, Utc};
use odon_core::{
    Artifact, ArtifactKind, Message, MessageContent, MessageRole, Session, SessionMode,
    SessionStatus,
};
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
                updated_at TEXT NOT NULL
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
        Ok(())
    }

    // --- Todo Operations (per-session task plan) ---

    pub fn set_todos(&self, session_id: &str, items: &serde_json::Value) -> Result<()> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
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
            let status: SessionStatus =
                serde_json::from_str(&status_str).unwrap_or(SessionStatus::Active);
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
        let conn = self.conn.lock().unwrap();
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

    pub fn get_artifact(&self, id: &str) -> Result<Artifact> {
        let conn = self.conn.lock().unwrap();
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
        } else {
            Err(StorageError::NotFound(format!("Artifact {}", id)))
        }
    }

    pub fn delete_artifact(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count = conn.execute("DELETE FROM artifacts WHERE id = ?1", params![id])?;
        Ok(count > 0)
    }

    pub fn delete_session(&self, id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM messages WHERE session_id = ?1", params![id]);
        let _ = conn.execute("DELETE FROM artifacts WHERE session_id = ?1", params![id]);
        let count = conn.execute("DELETE FROM sessions WHERE id = ?1", params![id])?;
        Ok(count > 0)
    }

    // --- Memory Operations ---

    pub fn set_memory(&self, key: &str, value: &str, category: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
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
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT key, value, category, updated_at FROM memories WHERE key = ?1")?;
        let mut rows = stmt.query(params![key])?;
        if let Some(row) = rows.next()? {
            let updated_at_str: String = row.get(3)?;
            let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&Utc))
                .unwrap_or_else(|_| Utc::now());
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
        let conn = self.conn.lock().unwrap();
        let mut memories = Vec::new();
        if let Some(cat) = category {
            let mut stmt = conn.prepare(
                "SELECT key, value, category, updated_at FROM memories WHERE category = ?1 ORDER BY updated_at DESC"
            )?;
            let rows = stmt.query_map(params![cat], |row| {
                let updated_at_str: String = row.get(3)?;
                let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());
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
                    .unwrap_or_else(|_| Utc::now());
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

    pub fn delete_memory(&self, key: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
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
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = Utc::now().to_rfc3339();
        conn.execute(
            "INSERT INTO provider_configs (provider_id, api_key, base_url, is_default, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(provider_id) DO UPDATE SET
                api_key = excluded.api_key,
                base_url = excluded.base_url,
                is_default = excluded.is_default,
                updated_at = excluded.updated_at",
            params![
                provider_id,
                api_key,
                base_url,
                if is_default { 1 } else { 0 },
                now
            ],
        )?;
        Ok(())
    }

    pub fn get_provider_config(&self, provider_id: &str) -> Result<Option<ProviderConfigRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT provider_id, api_key, base_url, is_default, updated_at
             FROM provider_configs WHERE provider_id = ?1",
        )?;
        let mut rows = stmt.query_map(params![provider_id], |row| {
            let def_int: i32 = row.get(3)?;
            Ok(ProviderConfigRecord {
                provider_id: row.get(0)?,
                api_key: row.get(1)?,
                base_url: row.get(2)?,
                is_default: def_int != 0,
                updated_at: row.get(4)?,
            })
        })?;
        if let Some(r) = rows.next() {
            Ok(Some(r?))
        } else {
            Ok(None)
        }
    }

    pub fn list_provider_configs(&self) -> Result<Vec<ProviderConfigRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT provider_id, api_key, base_url, is_default, updated_at
             FROM provider_configs ORDER BY provider_id ASC",
        )?;
        let rows = stmt.query_map([], |row| {
            let def_int: i32 = row.get(3)?;
            Ok(ProviderConfigRecord {
                provider_id: row.get(0)?,
                api_key: row.get(1)?,
                base_url: row.get(2)?,
                is_default: def_int != 0,
                updated_at: row.get(4)?,
            })
        })?;
        let mut list = Vec::new();
        for r in rows {
            list.push(r?);
        }
        Ok(list)
    }

    pub fn delete_provider_config(&self, provider_id: &str) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
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
    pub updated_at: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemoryItem {
    pub key: String,
    pub value: String,
    pub category: String,
    pub updated_at: DateTime<Utc>,
}
