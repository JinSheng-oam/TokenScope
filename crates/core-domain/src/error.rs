use std::error::Error;
use std::fmt;

/// Stable, non-sensitive errors. Never include user source content in messages.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DomainError {
    MissingField(&'static str),
    InvalidField(&'static str),
    TooLong(&'static str),
    TokenOverflow,
    ReasoningExceedsOutput,
    NegativeCost,
    DeletedRecordContainsUsage,
    DuplicateRecordId,
    CostOverflow,
    RevisionMustBePositive,
    PayloadHashMismatch,
    NonCanonicalRecord,
    SerializationFailed,
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing {field}"),
            Self::InvalidField(field) => write!(f, "invalid {field}"),
            Self::TooLong(field) => write!(f, "{field} is too long"),
            Self::TokenOverflow => write!(f, "token total overflow"),
            Self::ReasoningExceedsOutput => write!(f, "reasoning exceeds output"),
            Self::NegativeCost => write!(f, "cost must not be negative"),
            Self::DeletedRecordContainsUsage => write!(f, "deleted record contains usage or cost"),
            Self::DuplicateRecordId => write!(f, "duplicate record identity in aggregation"),
            Self::CostOverflow => write!(f, "cost sum overflow"),
            Self::RevisionMustBePositive => write!(f, "record revision must be positive"),
            Self::PayloadHashMismatch => write!(f, "record payload hash mismatch"),
            Self::NonCanonicalRecord => write!(f, "record metadata must use Unicode NFC"),
            Self::SerializationFailed => write!(f, "canonical serialization failed"),
        }
    }
}
impl Error for DomainError {}
