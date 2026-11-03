use std::io;
use tokenscope_core_domain::CanonicalRecord;
use tokenscope_local_store::SourceCheckpoint;

#[derive(Debug)]
pub enum ClaudeError {
    Io(io::Error),
    Json(serde_json::Error),
    Store(tokenscope_local_store::StoreError),
    Domain(tokenscope_core_domain::DomainError),
    Invalid(&'static str),
    Limit(&'static str),
    Pending,
}
impl std::fmt::Display for ClaudeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error ({:?})", e.kind()),
            Self::Json(_) => write!(f, "malformed JSONL"),
            Self::Store(e) => write!(f, "local store: {e}"),
            Self::Domain(e) => write!(f, "domain: {e}"),
            Self::Invalid(s) => write!(f, "invalid Claude Code usage ({s})"),
            Self::Limit(s) => write!(f, "Claude Code resource limit ({s})"),
            Self::Pending => write!(f, "source is partially written; checkpoint retained"),
        }
    }
}
impl std::error::Error for ClaudeError {}
impl From<io::Error> for ClaudeError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}
impl From<serde_json::Error> for ClaudeError {
    fn from(e: serde_json::Error) -> Self {
        Self::Json(e)
    }
}
impl From<tokenscope_local_store::StoreError> for ClaudeError {
    fn from(e: tokenscope_local_store::StoreError) -> Self {
        Self::Store(e)
    }
}
impl From<tokenscope_core_domain::DomainError> for ClaudeError {
    fn from(e: tokenscope_core_domain::DomainError) -> Self {
        Self::Domain(e)
    }
}
pub(crate) type Result<T> = std::result::Result<T, ClaudeError>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ClaudeReport {
    pub files: usize,
    pub unchanged_files: usize,
    pub pending_files: usize,
    pub parsed_lines: usize,
    pub inserted: usize,
    pub revised: usize,
    pub unchanged_records: usize,
    pub deleted: usize,
}

#[derive(Debug)]
pub struct ClaudeScan {
    pub next: SourceCheckpoint,
    pub records: Vec<CanonicalRecord>,
    pub parsed_lines: usize,
    /// Display-only label: an explicit Claude title, else the first prompt.
    pub title: Option<String>,
}
