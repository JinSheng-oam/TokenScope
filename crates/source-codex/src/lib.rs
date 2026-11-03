//! M0-04: bounded, resumable Codex JSONL session-total collector.
//!
//! - token_count.info.total_token_usage is CUMULATIVE, never summed across lines.
//! - last_token_usage is deliberately ignored (may repeat on rate-limit updates).
//! - one revisable record per logical session; file moves do not create duplicates.
//! - a complete malformed line or counter regression FAILS CLOSED.
//! - truncated trailing lines are left unread for the next scan.
//! - no prompt, response, source code, raw path, token or cookie is persisted.
//! - this milestone does NOT claim accurate per-day/per-model attribution.

mod collector;
mod model;
mod rollout;
mod scanner;

pub mod precise;

pub const MODULE_ID: &str = "source-codex";
pub const PARSER_VERSION: u32 = 1;
pub(crate) const MAX_FILES: usize = 20_000;

pub use collector::{collect_codex, discover_codex_files};
pub use model::{CodexError, CollectReport, ScanOutcome};
pub(crate) use model::{Counters, Result};
pub use precise::{
    collect_codex_precise, collect_codex_precise_tolerant, scan_precise, PreciseReport, PreciseScan,
};
pub(crate) use rollout::{
    bounded_line, event_ms, hash_path, mtime_ns, now_ms, read_session_header_metadata,
    session_header_from_bytes, sha_tail,
};
pub use rollout::{decompress_rollout, header_from_bytes, is_zstd_rollout, sha_tail_bytes};
pub use scanner::scan_file;

pub(crate) use tokenscope_local_store::SourceCheckpoint;
