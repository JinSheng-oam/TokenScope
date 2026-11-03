//! Versioned stdio IPC envelope shared by the Agent and Tauri bridge.
//! Transport is one request/response per short-lived Agent process; stdout is
//! reserved for one JSON envelope and stderr for diagnostics.
use serde::{Deserialize, Serialize};
use tokenscope_local_store::DashboardSnapshot;

pub const MODULE_ID: &str = "local-ipc";
pub const PROTOCOL_VERSION: u32 = 2;
/// Tracks the store schema so the desktop can never render a snapshot produced
/// by an incompatible Agent/database pair.
pub const SUPPORTED_DB_SCHEMA_VERSION: i64 = tokenscope_local_store::schema::VERSION;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardResponse {
    pub protocol_version: u32,
    pub snapshot: DashboardSnapshot,
}

impl DashboardResponse {
    pub fn new(snapshot: DashboardSnapshot) -> Self {
        Self {
            protocol_version: PROTOCOL_VERSION,
            snapshot,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err("unsupported local IPC protocol version");
        }
        if self.snapshot.schema_version != SUPPORTED_DB_SCHEMA_VERSION {
            return Err("unsupported dashboard database schema version");
        }
        if !["today", "7d", "30d", "all", "custom"].contains(&self.snapshot.range.as_str()) {
            return Err("invalid dashboard range");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DashboardResponse, MODULE_ID, PROTOCOL_VERSION, SUPPORTED_DB_SCHEMA_VERSION};
    use tokenscope_local_store::{
        AntigravityActivitySummary, DashboardSnapshot, ModelActivityBreakdown, SessionUsagePage,
        UsageTotals,
    };

    fn snapshot() -> DashboardSnapshot {
        DashboardSnapshot {
            schema_version: SUPPORTED_DB_SCHEMA_VERSION,
            generated_at_ms: 1,
            range: "7d".into(),
            range_start_ms: Some(1),
            range_end_ms: Some(2),
            time_zone: "UTC".into(),
            time_zone_valid: true,
            identity: None,
            local: UsageTotals::default(),
            remote: tokenscope_local_store::RemoteUsageTotals::default(),
            remote_by_device: vec![],
            remote_by_tool: vec![],
            remote_by_model: vec![],
            remote_by_project: vec![],
            unassigned: UsageTotals::default(),
            account_global: UsageTotals::default(),
            by_tool: vec![],
            by_model: vec![],
            by_project: vec![],
            daily: vec![],
            model_activity: Vec::<ModelActivityBreakdown>::new(),
            session_usage: SessionUsagePage {
                sessions: vec![],
                total: 0,
                offset: 0,
                limit: 50,
            },
            source_health: vec![],
            antigravity_quota: vec![],
            codex_quota: vec![],
            antigravity_activity: AntigravityActivitySummary {
                known_turns: 0,
                unknown_turns: 0,
                conversations: 0,
                by_model: vec![],
            },
            sync_status: tokenscope_local_store::SyncOutboxStatus::default(),
        }
    }

    #[test]
    fn reports_module_and_protocol_version() {
        assert_eq!(MODULE_ID, "local-ipc");
        assert_eq!(PROTOCOL_VERSION, 2);
    }

    #[test]
    fn dashboard_envelope_round_trips_and_rejects_unknown_versions() {
        let response = DashboardResponse::new(snapshot());
        response.validate().unwrap();
        let encoded = serde_json::to_vec(&response).unwrap();
        let decoded: DashboardResponse = serde_json::from_slice(&encoded).unwrap();
        decoded.validate().unwrap();
        let mut custom = DashboardResponse::new(snapshot());
        custom.snapshot.range = "custom".into();
        custom.validate().unwrap();
        let mut incompatible = decoded;
        incompatible.protocol_version = 99;
        assert!(incompatible.validate().is_err());
    }
}
