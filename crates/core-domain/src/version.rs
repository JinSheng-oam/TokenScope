use crate::identity::{hex_sha256, valid_lower_hex};
use crate::{CanonicalRecord, DomainError};
use serde::{Deserialize, Serialize};

const PAYLOAD_DOMAIN: &[u8] = b"tokenscope/canonical-record/v1\0";

/// SHA-256 over schema-tagged canonical JSON, with fixed Rust struct field
/// ordering and NFC normalized metadata. No scan time/observed_at enters hash.
/// Any wire field or format change requires explicit v2 + migration tests.
pub fn payload_hash(record: &CanonicalRecord) -> Result<String, DomainError> {
    let normalized = record.clone().normalized()?;
    let mut bytes = PAYLOAD_DOMAIN.to_vec();
    let payload = serde_json::to_vec(&normalized).map_err(|_| DomainError::SerializationFailed)?;
    bytes.extend_from_slice(&payload);
    Ok(hex_sha256(&bytes))
}

/// Revision is owned/assigned by the transactional local store, not the adapter.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct RevisionedRecord {
    pub record: CanonicalRecord,
    pub record_revision: u64,
    pub payload_hash: String,
}

impl RevisionedRecord {
    /// Caller must use the revision persisted in the same local-store transaction.
    pub fn from_store(record: CanonicalRecord, revision: u64) -> Result<Self, DomainError> {
        if revision == 0 {
            return Err(DomainError::RevisionMustBePositive);
        }
        let record = record.normalized()?;
        let hash = payload_hash(&record)?;
        Ok(Self {
            record,
            record_revision: revision,
            payload_hash: hash,
        })
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        if self.record_revision == 0 {
            return Err(DomainError::RevisionMustBePositive);
        }
        if self.record.clone().normalized()? != self.record {
            return Err(DomainError::NonCanonicalRecord);
        }
        if !valid_lower_hex(&self.payload_hash) || self.payload_hash != payload_hash(&self.record)?
        {
            return Err(DomainError::PayloadHashMismatch);
        }
        Ok(())
    }
}
