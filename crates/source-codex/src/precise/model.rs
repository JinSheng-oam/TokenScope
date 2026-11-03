use crate::{Counters, SourceCheckpoint};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{Cursor, Read, Seek, SeekFrom};
use tokenscope_core_domain::CanonicalRecord;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct V2Cursor {
    pub(super) path_hash: String,
    pub(super) tail_hash: String,
    pub(super) mtime_ns: u128,
    pub(super) mode: String, // "legacy" or "modern"
    pub(super) model: Option<String>,
    pub(super) cumulative: Option<Counters>,
    pub(super) legacy_seq: u64,
    pub(super) response_seq: u64,
    /// The rollout arrived as a zstd frame; its offsets are not seekable.
    #[serde(default)]
    pub(super) compressed: bool,
    #[serde(default)]
    pub(super) physical_size: u64,
    #[serde(default)]
    pub(super) scan_size: u64,
}

pub(super) enum BodyReader<'a> {
    File(File),
    Memory(Cursor<&'a [u8]>),
}

impl Read for BodyReader<'_> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::File(file) => file.read(buf),
            Self::Memory(cursor) => cursor.read(buf),
        }
    }
}

impl Seek for BodyReader<'_> {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        match self {
            Self::File(file) => file.seek(pos),
            Self::Memory(cursor) => cursor.seek(pos),
        }
    }
}

#[derive(Debug)]
pub struct PreciseScan {
    pub next: SourceCheckpoint,
    pub records: Vec<CanonicalRecord>,
    pub full_reconcile: bool,
    pub parsed_lines: usize,
    pub pending_line: bool,
    pub mode: String,
    /// First human prompt, used only as a local display label.
    pub title: Option<String>,
}

/// Turns the first user prompt into a short display label by dropping injected
/// XML-ish blocks (for example `<external_codex_apps_open_page>…`).

#[derive(Debug, Default)]
pub struct PreciseReport {
    pub files: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged: usize,
    pub lines: usize,
    pub pending: usize,
    pub reconciled: usize,
    pub unsupported: usize,
    pub missing_source_tombstones: usize,
}
