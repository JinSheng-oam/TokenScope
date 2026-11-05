use super::{LocalIdentity, LocalStore};
use crate::error::{Result, StoreError};
use crate::schema;
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::path::Path;
impl LocalStore {
    /// Opens one local SQLite authority. The caller (Agent) must enforce a
    /// single-process writer lock; SQLite IMMEDIATE transactions serialize writes.
    pub fn open(path: &Path) -> Result<Self> {
        let conn = Connection::open(path)?;
        Self::configure(&conn, true)?;
        Self::migrate(conn)
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        Self::configure(&conn, false)?;
        Self::migrate(conn)
    }

    /// Opens an existing database without creating files, running migrations, or
    /// enabling writes. Used by short-lived dashboard queries while the Agent is
    /// the sole writer. The caller must not use mutating methods on this handle.
    pub fn open_readonly(path: &Path) -> Result<Self> {
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        conn.execute_batch(
            "PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000; PRAGMA query_only=ON;",
        )?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version != schema::VERSION {
            return Err(StoreError::UnsupportedSchema(version));
        }
        let migration_count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3,4,5,6,7,8,9)",
            [],
            |r| r.get(0),
        )?;
        let highest: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?;
        if migration_count != schema::VERSION || highest != schema::VERSION {
            return Err(StoreError::DatabaseInconsistent(
                "schema_migrations disagrees with user_version",
            ));
        }
        Ok(Self { conn })
    }

    fn configure(conn: &Connection, persistent: bool) -> Result<()> {
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        if persistent {
            // Each commit is durable when acknowledged, including a power loss.
            conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
        }
        Ok(())
    }

    fn migrate(mut conn: Connection) -> Result<Self> {
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if !(0..=schema::VERSION).contains(&version) {
            return Err(StoreError::UnsupportedSchema(version));
        }
        if version == 0 {
            // Never silently adopt a DB whose schema is unknown / unversioned.
            let legacy: i64 = conn.query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name IN ('usage_records','source_checkpoints','schema_migrations')",
                [], |r| r.get(0)
            )?;
            if legacy != 0 {
                return Err(StoreError::UnsupportedSchema(0));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_001)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (1,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=1;")?;
            tx.commit()?;
        }
        if version <= 1 {
            // Fail closed on mismatched legacy metadata before any v2 DDL.
            let v1: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version=1",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if v1 != 1 || highest != 1 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_002)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (2,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=2;")?;
            tx.commit()?;
        }
        if version <= 2 {
            let prior: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2)",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if prior != 2 || highest != 2 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v3",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_003)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (3,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=3;")?;
            tx.commit()?;
        }
        if version <= 3 {
            let prior: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3)",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if prior != 3 || highest != 3 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v4",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_004)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (4,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=4;")?;
            tx.commit()?;
        }
        if version <= 4 {
            let prior: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3,4)",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if prior != 4 || highest != 4 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v5",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_005)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (5,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=5;")?;
            tx.commit()?;
        }
        if version <= 5 {
            let prior: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3,4,5)",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if prior != 5 || highest != 5 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v6",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_006)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (6,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=6;")?;
            tx.commit()?;
        }
        if version <= 6 {
            let prior: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3,4,5,6)",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if prior != 6 || highest != 6 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v7",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_007)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (7,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=7;")?;
            tx.commit()?;
        }
        if version <= 7 {
            let prior: i64 = conn.query_row(
                "SELECT COUNT(*) FROM schema_migrations WHERE version IN (1,2,3,4,5,6,7)",
                [],
                |r| r.get(0),
            )?;
            let highest: i64 = conn.query_row(
                "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
                [],
                |r| r.get(0),
            )?;
            if prior != 7 || highest != 7 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v8",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_008)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES (8,CAST(strftime('%s','now') AS INTEGER)*1000)", [])?;
            tx.execute_batch("PRAGMA user_version=8;")?;
            tx.commit()?;
        }
        if version <= 8 {
            let (count, highest): (i64, i64) = conn.query_row(
                "SELECT COUNT(*),MAX(version) FROM schema_migrations",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            if count != 8 || highest != 8 {
                return Err(StoreError::DatabaseInconsistent(
                    "legacy migration metadata invalid before v9",
                ));
            }
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute_batch(schema::MIGRATION_009)?;
            tx.execute("INSERT INTO schema_migrations(version,applied_at_ms) VALUES(9,CAST(strftime('%s','now') AS INTEGER)*1000)",[])?;
            tx.execute_batch("PRAGMA user_version=9;")?;
            tx.commit()?;
        }
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM schema_migrations WHERE version IN (1,2,3,4,5,6,7,8,9)",
            [],
            |r| r.get(0),
        )?;
        let future: i64 = conn.query_row(
            "SELECT COALESCE(MAX(version),0) FROM schema_migrations",
            [],
            |r| r.get(0),
        )?;
        if count != schema::VERSION || future != schema::VERSION {
            return Err(StoreError::DatabaseInconsistent(
                "schema_migrations disagrees with user_version",
            ));
        }
        let foreign_key_violations: i64 =
            conn.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                r.get(0)
            })?;
        if foreign_key_violations != 0 {
            return Err(StoreError::DatabaseInconsistent("foreign key violation"));
        }
        Ok(Self { conn })
    }

    pub fn schema_version(&self) -> Result<i64> {
        Ok(self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?)
    }

    /// Transactionally consistent online snapshot. The destination must not
    /// already exist. Never copy a live main DB file without its WAL state.
    pub fn backup_to(&self, destination: &Path) -> Result<()> {
        let path = destination
            .to_str()
            .ok_or(StoreError::InvalidInput("backup path encoding"))?;
        if destination.exists() {
            return Err(StoreError::InvalidInput("backup destination exists"));
        }
        self.conn.execute("VACUUM INTO ?1", [path])?;
        Ok(())
    }

    pub fn quick_integrity_check(&self) -> Result<()> {
        let status: String = self
            .conn
            .query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if status != "ok" {
            return Err(StoreError::DatabaseInconsistent(
                "SQLite quick_check failed",
            ));
        }
        Ok(())
    }

    /// Bootstrap once, before scanning. Identity cannot be rotated implicitly.
    pub fn init_identity(&mut self, identity: &LocalIdentity) -> Result<()> {
        fn valid_text(v: &str, max: usize) -> bool {
            !v.trim().is_empty() && v.len() <= max && !v.chars().any(char::is_control)
        }
        if !valid_text(&identity.device_id, 128)
            || !identity.device_id.is_ascii()
            || identity.device_id.chars().any(char::is_whitespace)
            || !valid_text(&identity.display_name, 128)
            || !valid_text(&identity.time_zone, 128)
        {
            return Err(StoreError::InvalidInput("local identity"));
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing: Option<String> = tx
            .query_row(
                "SELECT device_id FROM local_identity WHERE singleton=1",
                [],
                |r| r.get(0),
            )
            .optional()?;
        match existing {
            None => {
                tx.execute(
                "INSERT INTO local_identity(singleton,device_id,display_name,time_zone) VALUES(1,?1,?2,?3)",
                params![identity.device_id, identity.display_name, identity.time_zone]
            )?;
            }
            Some(id) if id == identity.device_id => {
                tx.execute(
                    "UPDATE local_identity SET display_name=?1, time_zone=?2 WHERE singleton=1",
                    params![identity.display_name, identity.time_zone],
                )?;
            }
            _ => return Err(StoreError::IdentityMismatch),
        }
        tx.commit()?;
        Ok(())
    }

    pub fn identity(&self) -> Result<Option<LocalIdentity>> {
        Ok(self
            .conn
            .query_row(
                "SELECT device_id,display_name,time_zone FROM local_identity WHERE singleton=1",
                [],
                |r| {
                    Ok(LocalIdentity {
                        device_id: r.get(0)?,
                        display_name: r.get(1)?,
                        time_zone: r.get(2)?,
                    })
                },
            )
            .optional()?)
    }
}

#[cfg(test)]
mod readonly_tests {
    use super::*;
    use crate::repository::models::AntigravityQuota;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn v8_upgrade_preserves_existing_usage_and_adds_revision_tracking() {
        let conn = Connection::open_in_memory().unwrap();
        for (i, migration) in [
            schema::MIGRATION_001,
            schema::MIGRATION_002,
            schema::MIGRATION_003,
            schema::MIGRATION_004,
            schema::MIGRATION_005,
            schema::MIGRATION_006,
            schema::MIGRATION_007,
            schema::MIGRATION_008,
        ]
        .iter()
        .enumerate()
        {
            conn.execute_batch(migration).unwrap();
            conn.execute(
                "INSERT INTO schema_migrations(version,applied_at_ms) VALUES(?1,0)",
                [i as i64 + 1],
            )
            .unwrap();
        }
        conn.execute_batch("PRAGMA user_version=8;
            INSERT INTO usage_records(record_id,owner_device_id,tool_id,scope,input_uncached_tokens,precision,record_revision,payload_hash,observed_at_ms,updated_at_ms)
            VALUES('existing','device','codex','device_local',123,'exact',1,'hash',0,0);").unwrap();
        let store = LocalStore::migrate(conn).unwrap();
        assert_eq!(store.schema_version().unwrap(), schema::VERSION);
        assert_eq!(
            store
                .conn
                .query_row(
                    "SELECT input_uncached_tokens FROM usage_records WHERE record_id='existing'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            123
        );
        assert_eq!(store.dashboard_revisions().unwrap(), (0, 0));
        store
            .conn
            .execute(
                "UPDATE usage_records SET payload_hash='changed' WHERE record_id='existing'",
                [],
            )
            .unwrap();
        assert_eq!(store.dashboard_revisions().unwrap().0, 1);
    }

    #[test]
    fn readonly_open_does_not_migrate_and_rejects_mutations() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tokenscope-readonly-{}-{nonce}.db",
            std::process::id()
        ));
        {
            let mut writer = LocalStore::open(&path).unwrap();
            writer
                .init_identity(&LocalIdentity {
                    device_id: "readonly-test".into(),
                    display_name: "Test".into(),
                    time_zone: "UTC".into(),
                })
                .unwrap();
        }
        {
            let mut reader = LocalStore::open_readonly(&path).unwrap();
            assert_eq!(reader.schema_version().unwrap(), schema::VERSION);
            let quota = AntigravityQuota {
                source_id: "local".into(),
                pool_id: "gemini".into(),
                window_id: "weekly".into(),
                remaining_basis_points: Some(5000),
                reset_at_ms: None,
                observed_at_ms: 100,
            };
            assert!(reader.upsert_antigravity_quotas(&[quota], 100).is_err());
        }
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(format!("{}-wal", path.display()));
        let _ = std::fs::remove_file(format!("{}-shm", path.display()));
    }
}
