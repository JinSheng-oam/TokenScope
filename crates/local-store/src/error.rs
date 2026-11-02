use std::fmt;
use tokenscope_core_domain::DomainError;

#[derive(Debug)]
pub enum StoreError {
    Sql(rusqlite::Error),
    Domain(DomainError),
    InvalidInput(&'static str),
    NotInitialized,
    IdentityMismatch,
    CheckpointConflict,
    RecordOwnershipConflict,
    SourceArbitrationConflict,
    DuplicateBatchRecord,
    RevisionOverflow,
    IntegerOutOfRange,
    UnsupportedSchema(i64),
    DatabaseInconsistent(&'static str),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sql(e) => write!(f, "SQLite error: {e}"),
            Self::Domain(e) => write!(f, "domain error: {e}"),
            Self::InvalidInput(s) => write!(f, "invalid {s}"),
            Self::NotInitialized => write!(f, "device identity not initialized"),
            Self::IdentityMismatch => write!(f, "immutable local device identity mismatch"),
            Self::CheckpointConflict => write!(f, "stale checkpoint; rescan required"),
            Self::RecordOwnershipConflict => {
                write!(f, "record identity belongs to another device/tool")
            }
            Self::SourceArbitrationConflict => {
                write!(f, "conflicting observations from different sources")
            }
            Self::DuplicateBatchRecord => write!(f, "duplicate record in scan batch"),
            Self::RevisionOverflow => write!(f, "record revision overflow"),
            Self::IntegerOutOfRange => write!(
                f,
                "unsigned token/revision cannot fit SQLite signed integer"
            ),
            Self::UnsupportedSchema(v) => write!(f, "unsupported SQLite schema version {v}"),
            Self::DatabaseInconsistent(s) => write!(f, "database invariant violated: {s}"),
        }
    }
}
impl std::error::Error for StoreError {}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sql(e)
    }
}
impl From<DomainError> for StoreError {
    fn from(e: DomainError) -> Self {
        Self::Domain(e)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

pub(crate) fn db_int(value: u64) -> Result<i64> {
    i64::try_from(value).map_err(|_| StoreError::IntegerOutOfRange)
}

pub(crate) fn pos_u64(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| StoreError::DatabaseInconsistent("negative unsigned integer"))
}
