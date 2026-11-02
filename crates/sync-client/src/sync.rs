use crate::{
    client::{HubClient, HubClientError},
    pairing::DeviceToken,
};
use serde::{Deserialize, Serialize};
use tokenscope_core_domain::RevisionedRecord;

pub const MAX_SYNC_BATCH: usize = 200;
pub const MAX_SYNC_REQUEST_BYTES: usize = 2 * 1024 * 1024;
#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct UsageBatchRequest<'a> {
    protocol_version: u32,
    batch_id: &'a str,
    records: &'a [RevisionedRecord],
}
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UsageAckStatus {
    Accepted,
    Unchanged,
    Superseded,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageAckItem {
    pub record_id: String,
    pub record_revision: u64,
    pub status: UsageAckStatus,
    pub hub_revision: u64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageBatchResponse {
    pub protocol_version: u32,
    pub batch_id: String,
    pub results: Vec<UsageAckItem>,
}
impl UsageBatchResponse {
    pub fn validate_for(
        &self,
        batch_id: &str,
        records: &[RevisionedRecord],
    ) -> Result<(), HubClientError> {
        if self.protocol_version != 1
            || self.batch_id != batch_id
            || self.results.len() != records.len()
        {
            return Err(HubClientError::ProtocolMismatch);
        }
        for (ack, record) in self.results.iter().zip(records) {
            if ack.record_id != record.record.record_id
                || ack.record_revision != record.record_revision
                || ack.hub_revision == 0
            {
                return Err(HubClientError::InvalidResponse);
            }
        }
        Ok(())
    }
}
impl HubClient {
    /// Upload at most 200 immutable revision envelopes. This method does not
    /// ACK the local Outbox; caller must compare-and-delete each accepted item.
    pub fn push_usage_batch(
        &self,
        token: &DeviceToken,
        batch_id: &str,
        records: &[RevisionedRecord],
    ) -> Result<UsageBatchResponse, HubClientError> {
        if batch_id.len() < 16
            || batch_id.len() > 128
            || !batch_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err(HubClientError::InvalidRequest("batch_id"));
        }
        if records.is_empty() || records.len() > MAX_SYNC_BATCH {
            return Err(HubClientError::InvalidRequest("records"));
        }
        for record in records {
            record
                .validate()
                .map_err(|_| HubClientError::InvalidRequest("record_envelope"))?;
        }
        let request = UsageBatchRequest {
            protocol_version: 1,
            batch_id,
            records,
        };
        let encoded =
            serde_json::to_vec(&request).map_err(|_| HubClientError::InvalidRequest("records"))?;
        if encoded.len() > MAX_SYNC_REQUEST_BYTES {
            return Err(HubClientError::RequestTooLarge);
        }
        let response: UsageBatchResponse = self.send_json(
            reqwest::Method::POST,
            "/api/v1/usage/batch",
            Some(&request),
            Some(token),
        )?;
        response.validate_for(batch_id, records)?;
        Ok(response)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageChange {
    pub change_seq: u64,
    pub owner_device_id: String,
    pub envelope: RevisionedRecord,
    pub changed_at_ms: i64,
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageChangesResponse {
    pub protocol_version: u32,
    pub changes: Vec<UsageChange>,
    pub next_cursor: u64,
    pub high_watermark: u64,
    pub has_more: bool,
}
impl UsageChangesResponse {
    pub fn validate(&self, requested_cursor: u64, limit: usize) -> Result<(), HubClientError> {
        if self.protocol_version != 1
            || self.changes.len() > limit
            || self.high_watermark < requested_cursor
            || self.next_cursor < requested_cursor
            || self.next_cursor > self.high_watermark
        {
            return Err(HubClientError::ProtocolMismatch);
        }
        let mut previous = requested_cursor;
        for change in &self.changes {
            if change.change_seq <= previous
                || change.change_seq > self.high_watermark
                || change.owner_device_id != change.envelope.record.owner_device_id
                || change.changed_at_ms < 0
            {
                return Err(HubClientError::InvalidResponse);
            }
            change
                .envelope
                .validate()
                .map_err(|_| HubClientError::InvalidResponse)?;
            previous = change.change_seq;
        }
        if previous != self.next_cursor || self.has_more != (self.next_cursor < self.high_watermark)
        {
            return Err(HubClientError::InvalidResponse);
        }
        Ok(())
    }
}

#[cfg(test)]
mod pull_tests {
    use super::*;
    #[test]
    fn empty_change_page_is_valid_without_advancing_cursor() {
        let page = UsageChangesResponse {
            protocol_version: 1,
            changes: vec![],
            next_cursor: 7,
            high_watermark: 7,
            has_more: false,
        };
        assert!(page.validate(7, 200).is_ok());
    }
    #[test]
    fn malformed_cursor_or_more_flag_is_rejected() {
        let page = UsageChangesResponse {
            protocol_version: 1,
            changes: vec![],
            next_cursor: 8,
            high_watermark: 9,
            has_more: false,
        };
        assert!(page.validate(7, 200).is_err());
    }
    #[test]
    fn canonical_revision_envelope_is_accepted_in_change_feed() {
        let fixture = include_str!("../../../fixtures/domain/canonical-record-v1.json");
        let envelope: RevisionedRecord = serde_json::from_str(fixture).unwrap();
        let page = UsageChangesResponse {
            protocol_version: 1,
            changes: vec![UsageChange {
                change_seq: 8,
                owner_device_id: envelope.record.owner_device_id.clone(),
                envelope,
                changed_at_ms: 123,
            }],
            next_cursor: 8,
            high_watermark: 8,
            has_more: false,
        };
        assert!(page.validate(7, 200).is_ok());
    }
}
