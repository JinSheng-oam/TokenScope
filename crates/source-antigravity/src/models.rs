use std::collections::BTreeMap;
use tokenscope_core_domain::CanonicalRecord;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NativeUsage {
    pub(super) fixed_prompt_tokens: u64,
    pub(super) input_tokens: u64,
    pub(super) cache_read_tokens: u64,
    pub(super) output_tokens: u64,
    pub(super) reasoning_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Snapshot {
    pub(super) stem: String,
    pub(super) project_id: Option<String>,
    pub(super) counts: BTreeMap<String, u64>,
    pub(super) known_turns: u64,
    pub(super) unknown_turns: u64,
    pub(super) skipped_rows: u64,
    pub(super) records: Vec<CanonicalRecord>,
    pub(super) missing_usage: BTreeMap<String, u64>,
}

#[derive(Clone)]
pub(super) struct ProjectMetadata {
    pub(super) project_id: String,
    pub(super) display_name: Option<String>,
}
