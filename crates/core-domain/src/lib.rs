//! TokenScope canonical usage domain (M0-02).
//!
//! This crate is deliberately IO-free. Adapters produce proposed records;
//! only the local-store layer may assign revisions and publish an outbox item.
//!
//! Invariants:
//! - Token fields are disjoint. Reasoning is a *subset* of output.
//! - Account-global and unknown usage never appear in device totals.
//! - Costs use integer micro-USD and retain partial-coverage information.
//! - Payloads are Unicode NFC normalized before hashing and persistence.
//! - Record IDs use stable, length-prefixed native identities; never prompt text.

pub mod error;
pub mod identity;
pub mod metric;
pub mod record;
pub mod version;

pub use error::DomainError;
pub use identity::{account_key, project_key, session_key, stable_record_id};
pub use metric::{aggregate_device_local, DeviceUsageTotal, TokenBreakdown};
pub use record::{CanonicalRecord, Precision, UsageScope};
pub use version::{payload_hash, RevisionedRecord};

pub const MODULE_ID: &str = "core-domain";
