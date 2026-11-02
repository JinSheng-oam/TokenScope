use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use crate::{DomainError, TokenBreakdown};
use crate::identity::{valid_lower_hex, valid_tool_id};

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageScope { DeviceLocal, AccountGlobal, Unknown }

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Precision { Exact, Aggregate, Estimated, Unknown }

/// Versioned wire payload. `record_revision` and `payload_hash` are *not*
/// adapter fields: they belong to the local store transaction/envelope.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct CanonicalRecord {
    pub record_id: String,
    pub owner_device_id: String,
    pub tool_id: String,
    pub scope: UsageScope,
    pub precision: Precision,
    pub account_id: Option<String>,
    pub model_id: Option<String>,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub event_at_ms: Option<i64>,
    pub usage: TokenBreakdown,
    pub estimated_cost_microusd: Option<i64>,
    pub price_version: Option<String>,
    pub is_deleted: bool,
}

fn check_text(s: &str, name: &'static str, max: usize) -> Result<(), DomainError> {
    if s.trim().is_empty() { return Err(DomainError::MissingField(name)); }
    if s.len() > max { return Err(DomainError::TooLong(name)); }
    if s.chars().any(|c| c.is_control()) { return Err(DomainError::InvalidField(name)); }
    Ok(())
}
fn check_opt(s: &Option<String>, name: &'static str, max: usize) -> Result<(), DomainError> {
    if let Some(s) = s { check_text(s, name, max)?; }
    Ok(())
}
fn check_opaque(s: &Option<String>, name: &'static str, prefix: &str) -> Result<(), DomainError> {
    if let Some(s) = s {
        if !s.starts_with(prefix) || !valid_lower_hex(&s[prefix.len()..]) {
            return Err(DomainError::InvalidField(name));
        }
    }
    Ok(())
}
fn nfc(input: &str) -> String { input.nfc().collect() }
fn nfc_opt(input: Option<String>) -> Option<String> { input.map(|s| nfc(&s)) }

impl CanonicalRecord {
    /// Validate **before** hashing or storage. Project/account/session IDs must
    /// be opaque hashes, not raw project paths, account names or conversation text.
    pub fn validate(&self) -> Result<(), DomainError> {
        if !valid_lower_hex(&self.record_id) { return Err(DomainError::InvalidField("record_id")); }
        check_text(&self.owner_device_id, "owner_device_id", 128)?;
        if !self.owner_device_id.is_ascii() || self.owner_device_id.chars().any(char::is_whitespace) {
            return Err(DomainError::InvalidField("owner_device_id"));
        }
        if !valid_tool_id(&self.tool_id) { return Err(DomainError::InvalidField("tool_id")); }
        check_opaque(&self.account_id, "account_id", "acct_")?;
        check_opt(&self.model_id, "model_id", 256)?;
        check_opaque(&self.project_id, "project_id", "prj_")?;
        check_opaque(&self.session_id, "session_id", "sess_")?;
        check_opt(&self.price_version, "price_version", 128)?;
        self.usage.validate()?;
        if self.scope == UsageScope::AccountGlobal && self.account_id.is_none() {
            return Err(DomainError::MissingField("account_id"));
        }
        if self.precision == Precision::Exact && self.usage.unclassified_tokens != 0 {
            return Err(DomainError::InvalidField("precision"));
        }
        if self.estimated_cost_microusd.is_some_and(|v| v < 0) {
            return Err(DomainError::NegativeCost);
        }
        if self.is_deleted && (self.usage != TokenBreakdown::default() || self.estimated_cost_microusd.is_some()) {
            return Err(DomainError::DeletedRecordContainsUsage);
        }
        Ok(())
    }

    /// Normalize user-readable metadata to NFC; IDs are already opaque ASCII
    /// and remain untouched. Call this before persisting a record.
    pub fn normalized(mut self) -> Result<Self, DomainError> {
        self.model_id = nfc_opt(self.model_id);
        self.price_version = nfc_opt(self.price_version);
        self.validate()?;
        Ok(self)
    }
}
