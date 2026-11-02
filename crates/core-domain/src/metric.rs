use std::collections::HashSet;
use serde::{Deserialize, Serialize};
use crate::{CanonicalRecord, DomainError, UsageScope};

/// Five DISJOINT, non-negative usage buckets. Reasoning belongs to output and
/// is not added again. If total is known but breakdown is not, use unclassified.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
pub struct TokenBreakdown {
    pub input_uncached_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub unclassified_tokens: u64,
    pub reasoning_tokens: Option<u64>,
}

impl TokenBreakdown {
    pub fn total(&self) -> Result<u64, DomainError> {
        self.input_uncached_tokens
            .checked_add(self.cache_read_tokens)
            .and_then(|x| x.checked_add(self.cache_write_tokens))
            .and_then(|x| x.checked_add(self.output_tokens))
            .and_then(|x| x.checked_add(self.unclassified_tokens))
            .ok_or(DomainError::TokenOverflow)
    }

    pub fn validate(&self) -> Result<(), DomainError> {
        self.total()?;
        if self.reasoning_tokens.is_some_and(|x| x > self.output_tokens) {
            return Err(DomainError::ReasoningExceedsOutput);
        }
        Ok(())
    }

    pub fn aggregate_only(total: u64) -> Self {
        Self { unclassified_tokens: total, ..Self::default() }
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DomainError> {
        let add = |a: u64, b: u64| a.checked_add(b).ok_or(DomainError::TokenOverflow);
        let reasoning = match (self.reasoning_tokens, other.reasoning_tokens) {
            (Some(a), Some(b)) => Some(add(a, b)?),
            // Unknown is not zero: retain incomplete reasoning provenance.
            _ => None,
        };
        let next = Self {
            input_uncached_tokens: add(self.input_uncached_tokens, other.input_uncached_tokens)?,
            cache_read_tokens: add(self.cache_read_tokens, other.cache_read_tokens)?,
            cache_write_tokens: add(self.cache_write_tokens, other.cache_write_tokens)?,
            output_tokens: add(self.output_tokens, other.output_tokens)?,
            unclassified_tokens: add(self.unclassified_tokens, other.unclassified_tokens)?,
            reasoning_tokens: reasoning,
        };
        next.validate()?;
        Ok(next)
    }
}

/// Cost coverage explicitly preserves unknown prices instead of silently
/// treating all missing costs as USD 0.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceUsageTotal {
    pub usage: TokenBreakdown,
    pub known_cost_microusd: i64,
    pub priced_records: u64,
    pub unpriced_records: u64,
    pub record_count: u64,
}

/// Input must be the current/reconciled view: one record per device + record ID.
/// Unknown/account-global/deleted records are excluded from device totals.
pub fn aggregate_device_local<'a>(
    records: impl IntoIterator<Item = &'a CanonicalRecord>,
) -> Result<DeviceUsageTotal, DomainError> {
    let mut result = DeviceUsageTotal::default();
    let mut ids = HashSet::new();
    for rec in records {
        rec.validate()?;
        if rec.scope != UsageScope::DeviceLocal || rec.is_deleted {
            continue;
        }
        if !ids.insert((rec.owner_device_id.as_str(), rec.record_id.as_str())) {
            return Err(DomainError::DuplicateRecordId);
        }
        result.usage = result.usage.checked_add(rec.usage)?;
        result.record_count = result.record_count.checked_add(1).ok_or(DomainError::TokenOverflow)?;
        if let Some(cost) = rec.estimated_cost_microusd {
            result.known_cost_microusd = result.known_cost_microusd
                .checked_add(cost).ok_or(DomainError::CostOverflow)?;
            result.priced_records = result.priced_records.checked_add(1).ok_or(DomainError::TokenOverflow)?;
        } else {
            result.unpriced_records = result.unpriced_records.checked_add(1).ok_or(DomainError::TokenOverflow)?;
        }
    }
    Ok(result)
}
