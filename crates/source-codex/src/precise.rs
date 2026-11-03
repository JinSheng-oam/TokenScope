//! M0-05 precision collector: per-response native usage is authoritative.
//! Legacy sessions use monotonic cumulative deltas. A schema switch triggers
//! a full, source-scoped reconciliation with explicit tombstones.
const VERSION: u32 = 6;
const MAX_BATCH: usize = 50_000; // explicit upper bound pending true streaming decoder
const MAX_SESSION_INDEX_BYTES: u64 = 64 * 1024 * 1024;
const MAX_SESSION_INDEX_LINE: usize = 1024 * 1024;
const MAX_SESSION_INDEX_ROWS: usize = 100_000;

mod collector;
mod model;
mod scanner;
mod usage;

pub use collector::{collect_codex_precise, collect_codex_precise_tolerant};
pub use model::{PreciseReport, PreciseScan};
pub use scanner::{
    scan_precise, scan_precise_body, scan_precise_continuation, scan_precise_continuation_body,
};
