use super::super::records::{
    enqueue, read_record, validate_session_id, validate_tool_id, write_record,
};
use super::super::{models::ProjectName, LocalStore, PROJECT_LABEL_TOOL_ID};
use crate::error::{Result, StoreError};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use tokenscope_core_domain::{payload_hash, TokenBreakdown};
impl LocalStore {
    /// Returns whether a session was manually removed from this device's statistics.
    pub fn is_session_suppressed(&self, tool_id: &str, session_id: &str) -> Result<bool> {
        validate_tool_id(tool_id)?;
        validate_session_id(session_id)?;
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM session_suppressions WHERE tool_id=?1 AND session_id=?2)",
            params![tool_id, session_id],
            |row| row.get(0),
        )?)
    }

    /// Removes a session from local usage and prevents later scans from importing it again.
    /// Original source conversations are never modified by this operation.
    pub fn delete_session(
        &mut self,
        tool_id: &str,
        session_id: &str,
        deleted_at_ms: i64,
    ) -> Result<usize> {
        validate_tool_id(tool_id)?;
        validate_session_id(session_id)?;
        if deleted_at_ms < 0 {
            return Err(StoreError::InvalidInput("session deletion time"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let owner: String = tx
            .query_row(
                "SELECT device_id FROM local_identity WHERE singleton=1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .ok_or(StoreError::NotInitialized)?;
        tx.execute(
            "INSERT OR IGNORE INTO session_suppressions(tool_id,session_id,deleted_at_ms) VALUES(?1,?2,?3)",
            params![tool_id, session_id, deleted_at_ms],
        )?;
        let record_ids = if tool_id == "antigravity" {
            let mut stmt = tx.prepare(
                "SELECT DISTINCT u.record_id FROM usage_records u \
                 JOIN record_sources rs ON rs.record_id=u.record_id \
                 WHERE u.owner_device_id=?1 AND u.tool_id=?2 AND rs.tool_id=?2 \
                   AND rs.source_key=?3 AND u.scope='device_local' AND u.is_deleted=0 \
                 ORDER BY u.record_id",
            )?;
            let rows = stmt.query_map(params![owner, tool_id, session_id], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let mut stmt = tx.prepare(
                "SELECT record_id FROM usage_records \
                 WHERE owner_device_id=?1 AND tool_id=?2 AND session_id=?3 \
                   AND scope='device_local' AND is_deleted=0 ORDER BY record_id",
            )?;
            let rows = stmt.query_map(params![owner, tool_id, session_id], |row| {
                row.get::<_, String>(0)
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        let activity_sources = if tool_id == "antigravity" {
            vec![session_id.to_owned()]
        } else {
            Vec::new()
        };
        let mut deleted = 0usize;
        for record_id in record_ids {
            let mut envelope = read_record(&tx, &record_id)?
                .ok_or(StoreError::DatabaseInconsistent("session record missing"))?;
            if envelope.record.is_deleted {
                continue;
            }
            envelope.record.is_deleted = true;
            envelope.record.usage = TokenBreakdown::default();
            envelope.record.estimated_cost_microusd = None;
            envelope.record.price_version = None;
            let hash = payload_hash(&envelope.record)?;
            let revision = i64::try_from(envelope.record_revision)
                .map_err(|_| StoreError::RevisionOverflow)?
                .checked_add(1)
                .ok_or(StoreError::RevisionOverflow)?;
            write_record(&tx, &envelope.record, revision, &hash, deleted_at_ms)?;
            enqueue(&tx, &record_id, revision, deleted_at_ms)?;
            deleted += 1;
        }
        tx.execute(
            "DELETE FROM session_names WHERE session_id=?1 AND tool_id=?2",
            params![session_id, tool_id],
        )?;
        tx.execute(
            "DELETE FROM empty_source_candidates WHERE tool_id=?1 AND source_key=?2",
            params![tool_id, session_id],
        )?;
        for source_key in activity_sources {
            tx.execute(
                "DELETE FROM antigravity_activity_sources WHERE source_key=?1",
                [&source_key],
            )?;
        }
        tx.commit()?;
        Ok(deleted)
    }

    /// Tombstones sources that remain absent from two complete inventories at least
    /// 30 seconds apart. Partial, unreadable, or untrusted inventories must not call this.
    /// Stores a display-only session label captured from the source transcript.
    /// The label never leaves this device: it is not part of usage records or
    /// the sync outbox.
    pub fn upsert_session_name(
        &mut self,
        session_id: &str,
        tool_id: &str,
        name: &str,
        updated_at_ms: i64,
    ) -> Result<()> {
        let name = name.trim();
        if session_id.is_empty()
            || session_id.len() > 256
            || tool_id.is_empty()
            || tool_id.len() > 64
            || name.is_empty()
            || name.chars().count() > 200
            || name.chars().any(char::is_control)
            || updated_at_ms < 0
        {
            return Err(StoreError::InvalidInput("session name"));
        }
        if self.is_session_suppressed(tool_id, session_id)? {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO session_names(session_id,tool_id,name,updated_at_ms)
             VALUES(?1,?2,?3,?4)
             ON CONFLICT(session_id) DO UPDATE SET
                tool_id=excluded.tool_id,name=excluded.name,updated_at_ms=excluded.updated_at_ms
             WHERE excluded.updated_at_ms >= session_names.updated_at_ms",
            params![session_id, tool_id, name, updated_at_ms],
        )?;
        Ok(())
    }

    /// Clears a display-only title when its source no longer provides one.
    pub fn clear_session_name(&mut self, session_id: &str, tool_id: &str) -> Result<bool> {
        validate_tool_id(tool_id)?;
        validate_session_id(session_id)?;
        let changed = self.conn.execute(
            "DELETE FROM session_names WHERE session_id=?1 AND tool_id=?2",
            params![session_id, tool_id],
        )?;
        Ok(changed > 0)
    }

    /// Caches a project display name separately from canonical usage records.
    pub fn upsert_project_name(
        &mut self,
        project_id: &str,
        name: &str,
        updated_at_ms: i64,
    ) -> Result<()> {
        let name = name.trim();
        if project_id.len() != 68
            || !project_id.starts_with("prj_")
            || !project_id[4..]
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || name.is_empty()
            || name == "."
            || name == ".."
            || name.chars().count() > 120
            || name.chars().any(char::is_control)
            || name.contains('/')
            || name.contains('\\')
            || updated_at_ms < 0
        {
            return Err(StoreError::InvalidInput("project display name"));
        }
        let cache_key = format!("project:{project_id}");
        self.upsert_session_name(&cache_key, PROJECT_LABEL_TOOL_ID, name, updated_at_ms)
    }

    /// Lists cached project labels for device-to-device synchronization.
    pub fn project_names(&self) -> Result<Vec<ProjectName>> {
        let mut stmt = self.conn.prepare(
            "SELECT session_id,name,updated_at_ms FROM session_names WHERE tool_id=?1 AND substr(session_id,1,8)='project:' ORDER BY session_id",
        )?;
        let rows = stmt.query_map([PROJECT_LABEL_TOOL_ID], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut names = Vec::new();
        for row in rows {
            let (cache_key, name, updated_at_ms) = row?;
            let Some(project_id) = cache_key.strip_prefix("project:") else {
                continue;
            };
            if valid_project_name(project_id, &name, updated_at_ms) {
                names.push(ProjectName {
                    project_id: project_id.to_owned(),
                    name,
                    updated_at_ms,
                });
            }
        }
        Ok(names)
    }

    /// Merges labels received from another paired device without touching usage rows.
    pub fn merge_project_names(&mut self, names: &[ProjectName]) -> Result<usize> {
        if names.len() > 20_000 {
            return Err(StoreError::InvalidInput("project name sync limit"));
        }
        for name in names {
            if !valid_project_name(&name.project_id, &name.name, name.updated_at_ms) {
                return Err(StoreError::InvalidInput("project display name"));
            }
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut changed = 0usize;
        for name in names {
            let cache_key = format!("project:{}", name.project_id);
            changed += tx.execute(
                "INSERT INTO session_names(session_id,tool_id,name,updated_at_ms) VALUES(?1,?2,?3,?4) \
                 ON CONFLICT(session_id) DO UPDATE SET tool_id=excluded.tool_id,name=excluded.name,updated_at_ms=excluded.updated_at_ms \
                 WHERE excluded.updated_at_ms >= session_names.updated_at_ms",
                params![cache_key, PROJECT_LABEL_TOOL_ID, name.name, name.updated_at_ms],
            )?;
        }
        tx.commit()?;
        Ok(changed)
    }
}

fn valid_project_name(project_id: &str, name: &str, updated_at_ms: i64) -> bool {
    project_id.len() == 68
        && project_id.starts_with("prj_")
        && project_id[4..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        && !name.trim().is_empty()
        && name.trim() != "."
        && name.trim() != ".."
        && name.chars().count() <= 120
        && !name.chars().any(char::is_control)
        && !name.contains('/')
        && !name.contains('\\')
        && updated_at_ms >= 0
}
