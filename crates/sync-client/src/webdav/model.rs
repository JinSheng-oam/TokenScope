use serde::{Deserialize, Serialize};
use tokenscope_core_domain::RevisionedRecord;
use tokenscope_local_store::ProjectName;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavDeviceInfo {
    pub device_id: String,
    pub display_name: String,
    pub os_name: String,
    pub app_version: String,
    pub time_zone: String,
    pub created_at_ms: i64,
    pub last_seen_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavBatchEntry {
    pub batch_seq: u64,
    pub batch_id: String,
    pub count: usize,
    pub file_name: String,
    pub created_at_ms: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavDeviceManifest {
    pub protocol_version: u32,
    pub device_id: String,
    pub last_batch_seq: u64,
    pub updated_at_ms: i64,
    pub batches: Vec<WebDavBatchEntry>,
}

impl WebDavDeviceManifest {
    pub fn new(device_id: String) -> Self {
        Self {
            protocol_version: 1,
            device_id,
            last_batch_seq: 0,
            updated_at_ms: 0,
            batches: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavBatchFile {
    pub protocol_version: u32,
    pub batch_id: String,
    pub device_id: String,
    pub batch_seq: u64,
    pub records: Vec<RevisionedRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavProjectNamesFile {
    pub protocol_version: u32,
    pub device_id: String,
    pub updated_at_ms: i64,
    pub project_names: Vec<ProjectName>,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct WebDavSyncReport {
    pub uploaded_records: usize,
    pub uploaded_batches: usize,
    pub pulled_records: usize,
    pub pulled_batches: usize,
    pub remaining_due: usize,
}
