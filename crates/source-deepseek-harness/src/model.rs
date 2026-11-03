#[derive(Clone, Debug)]
pub(super) struct LogCandidate {
    pub(super) path: PathBuf,
    pub(super) generation: u32,
    pub(super) compressed: bool,
}

#[derive(Debug, Default)]
pub(super) struct Inventory {
    pub(super) available: bool,
    pub(super) complete: bool,
    pub(super) paths: Vec<LogCandidate>,
    pub(super) skipped: usize,
}

#[derive(Clone, Debug)]
pub(super) struct SessionHeader {
    pub(super) version: u64,
    pub(super) id: String,
    pub(super) cwd: Option<String>,
    pub(super) created_at_ms: Option<i64>,
    pub(super) is_seeded: bool,
    pub(super) seed_length: Option<u64>,
}

#[derive(Clone, Debug)]
pub(super) struct ModelRoute {
    pub(super) provider: String,
    pub(super) model: String,
}

impl ModelRoute {
    pub(super) fn model_id(&self) -> Option<String> {
        let combined = format!("{}/{}", self.provider, self.model);
        (combined.len() <= 256
            && !combined.trim().is_empty()
            && !combined.chars().any(char::is_control))
        .then_some(combined)
    }
}

#[derive(Clone)]
pub(super) struct IndexedRecord {
    pub(super) source_seq: u64,
    pub(super) record: CanonicalRecord,
}

#[derive(Clone, Copy)]
pub(super) struct ReplacementSlot {
    pub(super) turn: u64,
    pub(super) step: u64,
    pub(super) record_index: usize,
    pub(super) source_seq: u64,
    pub(super) legacy_chunk: bool,
}

#[derive(Default)]
pub(super) struct UsageFold {
    pub(super) records: Vec<IndexedRecord>,
    pub(super) last_slot: Option<ReplacementSlot>,
}

pub(super) struct ParsedSession {
    pub(super) title: Option<String>,
    pub(super) records: Vec<CanonicalRecord>,
    pub(super) model_calls: usize,
}
use std::path::PathBuf;
use tokenscope_core_domain::CanonicalRecord;
