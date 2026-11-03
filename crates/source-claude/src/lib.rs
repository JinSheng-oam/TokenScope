//! M0-06: privacy-preserving Claude Code JSONL collector.
//!
//! Uses one authoritative usage snapshot per message.id, NOT per
//! assistant JSONL line. Full reconciliation on change handles streaming
//! partial rows, rewrites and revisions without accumulating duplicate counts.
//! Reads only local usage metadata; never persists prompts or raw file paths.

mod collector;
mod metadata;
mod model;
mod scanner;
mod usage;

pub const MODULE_ID: &str = "source-claude";
pub const PARSER_VERSION: u32 = 2;

pub use collector::collect_claude;
pub use metadata::discover_claude_files;
pub use model::{ClaudeError, ClaudeReport, ClaudeScan};
pub use scanner::scan_claude_file;
