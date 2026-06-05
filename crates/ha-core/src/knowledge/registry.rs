//! KnowledgeRegistry — the `knowledge_bases` + access-binding tables.
//!
//! Truth source (design D9). Wraps `Arc<SessionDB>` so the tables live inside
//! `sessions.db` next to `projects` / `channel_conversations`, sharing the one
//! SQLite connection. `~/.hope-agent/knowledge/index.db` holds only the
//! rebuildable note/chunk/link cache — never the registry.

use anyhow::Result;
use rusqlite::{params, OptionalExtension};
use std::path::PathBuf;
use std::sync::Arc;

use super::types::{
    CreateKnowledgeBaseInput, KbAccess, KnowledgeBase, KnowledgeBaseMeta, UpdateKnowledgeBaseInput,
};
use crate::session::SessionDB;

/// Knowledge base persistence manager. Wraps `Arc<SessionDB>` to reuse its
/// connection (the tables live in `sessions.db`).
pub struct KnowledgeRegistry {
    session_db: Arc<SessionDB>,
}

impl KnowledgeRegistry {
    pub fn new(session_db: Arc<SessionDB>) -> Self {
        Self { session_db }
    }

    /// Idempotent DDL. Called once at startup from `app_init`.
    pub fn migrate(&self) -> Result<()> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        // FK targets resolve because SessionDB::open set `PRAGMA foreign_keys=ON`.
        // ON DELETE CASCADE on the join tables auto-cleans attach rows when a
        // session / project / KB is deleted; `delete()` also clears them
        // explicitly inside one transaction as a double safeguard.
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS knowledge_bases (
                id          TEXT PRIMARY KEY,
                name        TEXT NOT NULL,
                emoji       TEXT,
                root_dir    TEXT,
                archived    INTEGER NOT NULL DEFAULT 0,
                created_at  INTEGER NOT NULL,
                updated_at  INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_knowledge_bases_archived
                ON knowledge_bases(archived, updated_at DESC);

            CREATE TABLE IF NOT EXISTS session_knowledge_bases (
                session_id TEXT NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
                kb_id      TEXT NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
                access     TEXT NOT NULL DEFAULT 'read' CHECK (access IN ('read','write')),
                created_at INTEGER NOT NULL,
                PRIMARY KEY (session_id, kb_id)
            );
            CREATE INDEX IF NOT EXISTS idx_session_kb_kb
                ON session_knowledge_bases(kb_id);

            CREATE TABLE IF NOT EXISTS project_knowledge_bases (
                project_id TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
                kb_id      TEXT NOT NULL REFERENCES knowledge_bases(id) ON DELETE CASCADE,
                access     TEXT NOT NULL DEFAULT 'read' CHECK (access IN ('read','write')),
                created_at INTEGER NOT NULL,
                PRIMARY KEY (project_id, kb_id)
            );
            CREATE INDEX IF NOT EXISTS idx_project_kb_kb
                ON project_knowledge_bases(kb_id);",
        )?;

        Ok(())
    }

    // ── CRUD: knowledge_bases ──────────────────────────────────

    pub fn create(&self, input: CreateKnowledgeBaseInput) -> Result<KnowledgeBase> {
        let trimmed_name = input.name.trim();
        if trimmed_name.is_empty() {
            anyhow::bail!("knowledge base name cannot be empty");
        }
        let name = trimmed_name.to_string();
        let id = uuid::Uuid::new_v4().to_string();
        let now = chrono::Utc::now().timestamp_millis();

        // Validate an external root if given: must exist and be a directory so a
        // bind never silently points at a missing/file path.
        let root_dir = match normalize_optional(input.root_dir.as_deref()) {
            Some(raw) => {
                let p = PathBuf::from(raw);
                let canon = p.canonicalize().map_err(|e| {
                    anyhow::anyhow!("cannot resolve external root '{}': {}", raw, e)
                })?;
                if !canon.is_dir() {
                    anyhow::bail!("external root is not a directory: {}", canon.display());
                }
                Some(canon.to_string_lossy().to_string())
            }
            None => None,
        };

        let emoji = normalize_optional(input.emoji.as_deref()).map(str::to_string);

        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        conn.execute(
            "INSERT INTO knowledge_bases (id, name, emoji, root_dir, archived, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
            params![id, name, emoji, root_dir, now, now],
        )?;

        Ok(KnowledgeBase {
            id,
            name,
            emoji,
            root_dir,
            archived: false,
            created_at: now,
            updated_at: now,
        })
    }

    pub fn get(&self, id: &str) -> Result<Option<KnowledgeBase>> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let row = conn
            .query_row(
                "SELECT id, name, emoji, root_dir, archived, created_at, updated_at
                 FROM knowledge_bases WHERE id = ?1",
                params![id],
                row_to_kb,
            )
            .optional()?;
        Ok(row)
    }

    pub fn update(&self, id: &str, patch: UpdateKnowledgeBaseInput) -> Result<KnowledgeBase> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;

        let now = chrono::Utc::now().timestamp_millis();
        let mut sets: Vec<String> = Vec::new();
        let mut params_vec: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();

        if let Some(name) = &patch.name {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                anyhow::bail!("knowledge base name cannot be empty");
            }
            let idx = params_vec.len() + 1;
            sets.push(format!("name = ?{}", idx));
            params_vec.push(Box::new(trimmed.to_string()));
        }
        if let Some(emoji) = &patch.emoji {
            let idx = params_vec.len() + 1;
            sets.push(format!("emoji = ?{}", idx));
            let normalized = if emoji.trim().is_empty() {
                None
            } else {
                Some(emoji.clone())
            };
            params_vec.push(Box::new(normalized));
        }
        if let Some(archived) = patch.archived {
            let idx = params_vec.len() + 1;
            sets.push(format!("archived = ?{}", idx));
            params_vec.push(Box::new(if archived { 1i64 } else { 0i64 }));
        }

        let idx = params_vec.len() + 1;
        sets.push(format!("updated_at = ?{}", idx));
        params_vec.push(Box::new(now));

        let id_idx = params_vec.len() + 1;
        params_vec.push(Box::new(id.to_string()));

        let sql = format!(
            "UPDATE knowledge_bases SET {} WHERE id = ?{}",
            sets.join(", "),
            id_idx
        );
        let param_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        conn.execute(&sql, param_refs.as_slice())?;

        let kb = conn
            .query_row(
                "SELECT id, name, emoji, root_dir, archived, created_at, updated_at
                 FROM knowledge_bases WHERE id = ?1",
                params![id],
                row_to_kb,
            )
            .optional()?
            .ok_or_else(|| anyhow::anyhow!("knowledge base not found after update: {}", id))?;
        Ok(kb)
    }

    /// Delete a KB row + its attach rows inside a single transaction. The
    /// on-disk index rows + internal notes directory are cross-store and handled
    /// by [`super::delete_kb_cascade`].
    pub fn delete(&self, id: &str) -> Result<()> {
        let mut conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM session_knowledge_bases WHERE kb_id = ?1",
            params![id],
        )?;
        tx.execute(
            "DELETE FROM project_knowledge_bases WHERE kb_id = ?1",
            params![id],
        )?;
        tx.execute("DELETE FROM knowledge_bases WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// Every KB id (including archived). Used by the index reconciler.
    pub fn list_all_ids(&self) -> Result<Vec<String>> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn.prepare("SELECT id FROM knowledge_bases")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// List KBs. `note_count` is filled with 0 here (the index lives in a
    /// separate DB) — the command layer enriches it from the index backend,
    /// mirroring `ProjectMeta::memory_count`.
    pub fn list(&self, include_archived: bool) -> Result<Vec<KnowledgeBaseMeta>> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let where_sql = if include_archived {
            ""
        } else {
            "WHERE archived = 0"
        };
        let sql = format!(
            "SELECT id, name, emoji, root_dir, archived, created_at, updated_at
             FROM knowledge_bases {} ORDER BY updated_at DESC",
            where_sql
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map([], |row| {
            let kb = row_to_kb(row)?;
            let external = kb
                .root_dir
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
            Ok(KnowledgeBaseMeta {
                kb,
                note_count: 0,
                external,
            })
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    // ── Access bindings ────────────────────────────────────────

    pub fn attach_session(&self, session_id: &str, kb_id: &str, access: KbAccess) -> Result<()> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO session_knowledge_bases (session_id, kb_id, access, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id, kb_id) DO UPDATE SET access = excluded.access",
            params![session_id, kb_id, access.as_str(), now],
        )?;
        Ok(())
    }

    pub fn detach_session(&self, session_id: &str, kb_id: &str) -> Result<()> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "DELETE FROM session_knowledge_bases WHERE session_id = ?1 AND kb_id = ?2",
            params![session_id, kb_id],
        )?;
        Ok(())
    }

    pub fn attach_project(&self, project_id: &str, kb_id: &str, access: KbAccess) -> Result<()> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let now = chrono::Utc::now().timestamp_millis();
        conn.execute(
            "INSERT INTO project_knowledge_bases (project_id, kb_id, access, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(project_id, kb_id) DO UPDATE SET access = excluded.access",
            params![project_id, kb_id, access.as_str(), now],
        )?;
        Ok(())
    }

    pub fn detach_project(&self, project_id: &str, kb_id: &str) -> Result<()> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        conn.execute(
            "DELETE FROM project_knowledge_bases WHERE project_id = ?1 AND kb_id = ?2",
            params![project_id, kb_id],
        )?;
        Ok(())
    }

    /// `(kb_id, access)` rows explicitly attached to a session.
    pub fn list_session_attachments(&self, session_id: &str) -> Result<Vec<(String, KbAccess)>> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT kb_id, access FROM session_knowledge_bases WHERE session_id = ?1")?;
        let rows = stmt.query_map(params![session_id], |row| {
            let kb_id: String = row.get(0)?;
            let access: String = row.get(1)?;
            Ok((kb_id, KbAccess::from_str_lenient(&access)))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }

    /// `(kb_id, access)` rows attached to a project.
    pub fn list_project_attachments(&self, project_id: &str) -> Result<Vec<(String, KbAccess)>> {
        let conn = self
            .session_db
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("Lock error: {}", e))?;
        let mut stmt = conn
            .prepare("SELECT kb_id, access FROM project_knowledge_bases WHERE project_id = ?1")?;
        let rows = stmt.query_map(params![project_id], |row| {
            let kb_id: String = row.get(0)?;
            let access: String = row.get(1)?;
            Ok((kb_id, KbAccess::from_str_lenient(&access)))
        })?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r?);
        }
        Ok(out)
    }
}

// ── Row helpers ─────────────────────────────────────────────────

fn row_to_kb(row: &rusqlite::Row) -> rusqlite::Result<KnowledgeBase> {
    Ok(KnowledgeBase {
        id: row.get(0)?,
        name: row.get(1)?,
        emoji: row.get::<_, Option<String>>(2).unwrap_or(None),
        root_dir: row.get::<_, Option<String>>(3).unwrap_or(None),
        archived: row.get::<_, i64>(4).unwrap_or(0) != 0,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn normalize_optional(value: Option<&str>) -> Option<&str> {
    match value {
        Some(v) if !v.trim().is_empty() => Some(v),
        _ => None,
    }
}

/// Resolve a KB's notes directory + whether it is an external (read-only) root.
///
/// Internal KBs (NULL `root_dir`) materialize the default
/// `~/.hope-agent/knowledge/{id}/notes/` lazily (mirrors project workspace), so
/// the path is never written into the DB and `HA_DATA_DIR` stays relocatable.
/// External KBs return their bound path as-is (canonicalized at create time).
pub fn resolve_kb_dir(kb_id: &str) -> Result<(PathBuf, bool)> {
    let db =
        crate::get_knowledge_db().ok_or_else(|| anyhow::anyhow!("knowledge db not initialized"))?;
    let kb = db
        .get(kb_id)?
        .ok_or_else(|| anyhow::anyhow!("knowledge base not found: {}", kb_id))?;
    if let Some(root) = kb.root_dir.filter(|s| !s.trim().is_empty()) {
        Ok((PathBuf::from(root), true))
    } else {
        let dir = crate::paths::knowledge_kb_notes_dir(kb_id)?;
        let path = crate::util::ensure_dir_canonical(&dir)?;
        Ok((PathBuf::from(path), false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn registry() -> (tempfile::TempDir, KnowledgeRegistry) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("sessions.db");
        let session_db = Arc::new(SessionDB::open(&db_path).unwrap());
        // `project_knowledge_bases` FKs `projects` — created by ProjectDB::migrate,
        // which runs before the registry in production (app_init).
        crate::project::ProjectDB::new(session_db.clone())
            .migrate()
            .unwrap();
        let reg = KnowledgeRegistry::new(session_db);
        reg.migrate().unwrap();
        (dir, reg)
    }

    #[test]
    fn create_get_update_delete_roundtrip() {
        let (_d, reg) = registry();
        let kb = reg
            .create(CreateKnowledgeBaseInput {
                name: "  Work  ".into(),
                emoji: Some("📚".into()),
                root_dir: None,
            })
            .unwrap();
        assert_eq!(kb.name, "Work");
        assert!(!kb.is_external());

        let fetched = reg.get(&kb.id).unwrap().unwrap();
        assert_eq!(fetched.emoji.as_deref(), Some("📚"));

        let updated = reg
            .update(
                &kb.id,
                UpdateKnowledgeBaseInput {
                    name: Some("Personal".into()),
                    archived: Some(true),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Personal");
        assert!(updated.archived);

        assert_eq!(reg.list(false).unwrap().len(), 0); // archived hidden
        assert_eq!(reg.list(true).unwrap().len(), 1);

        reg.delete(&kb.id).unwrap();
        assert!(reg.get(&kb.id).unwrap().is_none());
    }

    #[test]
    fn empty_name_rejected() {
        let (_d, reg) = registry();
        assert!(reg
            .create(CreateKnowledgeBaseInput {
                name: "   ".into(),
                emoji: None,
                root_dir: None,
            })
            .is_err());
    }
}
