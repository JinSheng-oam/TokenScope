//! Versioned stdio IPC envelope shared by the Agent and Tauri bridge.
//! Transport is one request/response per short-lived Agent process; stdout is
//! reserved for one JSON envelope and stderr for diagnostics.
use serde::{Deserialize, Serialize};
use tokenscope_local_store::{DashboardSnapshot, SessionAnalysisPage};

pub const MODULE_ID: &str = "local-ipc";
pub const PROTOCOL_VERSION: u32 = 2;
pub const AGENT_CONTROL_DIRECTORY: &str = "agent-control";
pub const SESSION_ANALYSIS_PROTOCOL_VERSION: u32 = 1;
/// Tracks the store schema so the desktop can never render a snapshot produced
/// by an incompatible Agent/database pair.
pub const SUPPORTED_DB_SCHEMA_VERSION: i64 = tokenscope_local_store::schema::VERSION;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DashboardResponse {
    #[serde(default)]
    pub request_key: String,
    pub protocol_version: u32,
    pub snapshot: DashboardSnapshot,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SessionAnalysisResponse {
    pub protocol_version: u32,
    pub analysis: SessionAnalysisPage,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDeleteRequest {
    pub request_id: String,
    pub tool_id: String,
    pub session_id: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDeleteResponse {
    pub request_id: String,
    pub deleted_records: usize,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    #[default]
    Ok,
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticCheck {
    pub id: String,
    pub title: String,
    pub severity: DiagnosticSeverity,
    pub detail: String,
    pub action: Option<String>,
}

impl DiagnosticCheck {
    pub fn new(
        id: impl Into<String>,
        title: impl Into<String>,
        severity: DiagnosticSeverity,
        detail: impl Into<String>,
        action: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            severity,
            detail: detail.into(),
            action,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiagnosticToolUsage {
    pub tool_id: String,
    pub total_tokens: u64,
    pub record_count: u64,
    pub last_event_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct DiagnosticFacts {
    pub database_path: String,
    pub database_bytes: u64,
    pub wal_bytes: u64,
    pub schema_version: Option<i64>,
    pub expected_schema_version: i64,
    pub device_id: Option<String>,
    pub marker_present: bool,
    pub record_count: u64,
    pub total_tokens: u64,
    pub source_count: u64,
    pub last_event_at_ms: Option<i64>,
    pub last_scan_at_ms: Option<i64>,
    pub free_disk_bytes: Option<u64>,
    pub agent_running: bool,
    pub agent_path: Option<String>,
    pub legacy_database_path: Option<String>,
    pub log_bytes: u64,
    pub log_modified_at_ms: Option<i64>,
    pub sync_pending: u64,
    pub sync_deferred: u64,
    pub sync_blocked: u64,
    pub sync_last_status: String,
    pub tools: Vec<DiagnosticToolUsage>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DiagnosticReport {
    pub generated_at_ms: i64,
    pub overall: DiagnosticSeverity,
    pub checks: Vec<DiagnosticCheck>,
    pub facts: DiagnosticFacts,
}

impl DiagnosticReport {
    pub fn new(generated_at_ms: i64, facts: DiagnosticFacts) -> Self {
        Self {
            generated_at_ms,
            overall: DiagnosticSeverity::Ok,
            checks: Vec::new(),
            facts,
        }
    }

    pub fn push(&mut self, check: DiagnosticCheck) {
        self.checks.push(check);
        self.recompute_overall();
    }

    pub fn recompute_overall(&mut self) {
        // Informational checks must not make a healthy system look degraded.
        self.overall = if self
            .checks
            .iter()
            .any(|check| check.severity == DiagnosticSeverity::Error)
        {
            DiagnosticSeverity::Error
        } else if self
            .checks
            .iter()
            .any(|check| check.severity == DiagnosticSeverity::Warning)
        {
            DiagnosticSeverity::Warning
        } else {
            DiagnosticSeverity::Ok
        };
    }
}

impl DashboardResponse {
    pub fn new(snapshot: DashboardSnapshot) -> Self {
        Self {
            request_key: String::new(),
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

impl SessionAnalysisResponse {
    pub fn new(analysis: SessionAnalysisPage) -> Self {
        Self {
            protocol_version: SESSION_ANALYSIS_PROTOCOL_VERSION,
            analysis,
        }
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != SESSION_ANALYSIS_PROTOCOL_VERSION {
            return Err("unsupported session analysis protocol version");
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
            data_revision: 0,
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
            remote_daily: vec![],
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
