use super::super::models::AntigravityActivitySummary;
use super::super::LocalStore;
use crate::error::{pos_u64, Result, StoreError};
impl LocalStore {
    /// Read only known Antigravity activity, independent of usage/outbox.
    pub fn antigravity_activity_summary(&self) -> Result<AntigravityActivitySummary> {
        let (known, unknown, conversations): (i64,i64,i64) = self.conn.query_row(
            "SELECT COALESCE(SUM(known_turns),0),COALESCE(SUM(unknown_turns),0),COUNT(*) FROM antigravity_activity_sources",
            [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let mut stmt = self.conn.prepare("SELECT model_id,SUM(response_count) FROM antigravity_model_activity GROUP BY model_id ORDER BY model_id LIMIT 513")?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        if rows.len() > 512 {
            return Err(StoreError::InvalidInput("activity summary too many models"));
        }
        Ok(AntigravityActivitySummary {
            known_turns: pos_u64(known)?,
            unknown_turns: pos_u64(unknown)?,
            conversations: pos_u64(conversations)?,
            by_model: rows
                .into_iter()
                .map(|(m, n)| Ok((m, pos_u64(n)?)))
                .collect::<Result<Vec<_>>>()?,
        })
    }
}
