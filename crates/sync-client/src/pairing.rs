use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fmt};
use zeroize::Zeroize;

use crate::client::HubClientError;

pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PairRequest {
    pub pairing_code: String,
    pub device_id: String,
    pub display_name: String,
    pub os_name: String,
    pub app_version: String,
    pub time_zone: String,
}

impl Drop for PairRequest {
    fn drop(&mut self) {
        self.pairing_code.zeroize();
    }
}

impl fmt::Debug for PairRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairRequest")
            .field("pairing_code", &"[REDACTED]")
            .field("device_id", &self.device_id)
            .field("display_name", &self.display_name)
            .field("os_name", &self.os_name)
            .field("app_version", &self.app_version)
            .field("time_zone", &self.time_zone)
            .finish()
    }
}

impl PairRequest {
    /// Basic client-side validation. The Hub repeats all validation and is authoritative.
    pub fn validate(&self) -> Result<(), &'static str> {
        let valid_code = self.pairing_code.len() == 52
            && self
                .pairing_code
                .bytes()
                .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b));
        if !valid_code {
            return Err("pairing_code");
        }

        let valid_device_id = (8..=128).contains(&self.device_id.len())
            && self
                .device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            && self.device_id.as_bytes()[0].is_ascii_alphanumeric();
        if !valid_device_id {
            return Err("device_id");
        }

        let name = self.display_name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err("display_name");
        }
        if !["windows", "macos", "linux", "other"].contains(&self.os_name.as_str()) {
            return Err("os_name");
        }

        let valid_version = !self.app_version.is_empty()
            && self.app_version.len() <= 64
            && self.app_version.as_bytes()[0].is_ascii_alphanumeric()
            && self
                .app_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(&b));
        if !valid_version {
            return Err("app_version");
        }
        if self.time_zone.is_empty()
            || self.time_zone.len() > 64
            || !self
                .time_zone
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
        {
            return Err("time_zone");
        }
        Ok(())
    }
}

/// A 256-bit opaque bearer credential. Debug output is always redacted.
#[derive(Deserialize, PartialEq, Eq)]
#[serde(transparent)]
pub struct DeviceToken(String);

impl DeviceToken {
    pub fn new(mut value: String) -> Result<Self, &'static str> {
        if value.len() != 43
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            value.zeroize();
            return Err("device_token");
        }
        Ok(Self(value))
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        if self.0.len() != 43
            || !self
                .0
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        {
            return Err("device_token");
        }
        Ok(())
    }

    pub(crate) fn expose_for_authorization(&self) -> &str {
        &self.0
    }
}

impl Drop for DeviceToken {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl fmt::Debug for DeviceToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeviceToken([REDACTED])")
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PairResponse {
    pub protocol_version: u32,
    pub device_id: String,
    pub device_token: DeviceToken,
    pub created_at_ms: i64,
}

impl PairResponse {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err("protocol_version");
        }
        let valid_device_id = (8..=128).contains(&self.device_id.len())
            && self
                .device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            && self.device_id.as_bytes()[0].is_ascii_alphanumeric();
        if !valid_device_id {
            return Err("device_id");
        }
        self.device_token.validate()?;
        if self.created_at_ms < 0 {
            return Err("created_at_ms");
        }
        Ok(())
    }
}

impl fmt::Debug for PairResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairResponse")
            .field("protocol_version", &self.protocol_version)
            .field("device_id", &self.device_id)
            .field("device_token", &"[REDACTED]")
            .field("created_at_ms", &self.created_at_ms)
            .finish()
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RotateTokenResponse {
    pub protocol_version: u32,
    pub device_id: String,
    pub device_token: DeviceToken,
    pub created_at_ms: i64,
}

impl RotateTokenResponse {
    pub fn validate(&self) -> Result<(), HubClientError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(HubClientError::ProtocolMismatch);
        }
        if self.device_id.len() < 8
            || self.device_id.len() > 128
            || self.created_at_ms < 0
            || !self.device_id.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(HubClientError::InvalidResponse);
        }
        self.device_token
            .validate()
            .map_err(HubClientError::InvalidRequest)
    }
}

impl fmt::Debug for RotateTokenResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RotateTokenResponse")
            .field("protocol_version", &self.protocol_version)
            .field("device_id", &self.device_id)
            .field("device_token", &"[REDACTED]")
            .field("created_at_ms", &self.created_at_ms)
            .finish()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeviceSummary {
    pub device_id: String,
    pub display_name: String,
    pub os_name: String,
    pub app_version: String,
    pub time_zone: String,
    pub created_at_ms: i64,
    pub last_seen_ms: Option<i64>,
    pub revoked_at_ms: Option<i64>,
}

impl DeviceSummary {
    pub fn validate(&self) -> Result<(), &'static str> {
        let valid_device_id = (8..=128).contains(&self.device_id.len())
            && self.device_id.as_bytes()[0].is_ascii_alphanumeric()
            && self
                .device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b));
        if !valid_device_id {
            return Err("device_id");
        }
        if self.display_name.trim().is_empty()
            || self.display_name.chars().count() > 64
            || self.display_name.chars().any(char::is_control)
        {
            return Err("display_name");
        }
        if !["windows", "macos", "linux", "other"].contains(&self.os_name.as_str()) {
            return Err("os_name");
        }
        if self.app_version.is_empty()
            || self.app_version.len() > 64
            || !self.app_version.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .app_version
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b".+_-".contains(&b))
        {
            return Err("app_version");
        }
        if self.time_zone.is_empty()
            || self.time_zone.len() > 64
            || !self
                .time_zone
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
        {
            return Err("time_zone");
        }
        if self.created_at_ms < 0
            || self.last_seen_ms.is_some_and(|v| v < 0)
            || self.revoked_at_ms.is_some_and(|v| v < 0)
        {
            return Err("timestamp");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DeviceListResponse {
    pub protocol_version: u32,
    pub devices: Vec<DeviceSummary>,
}

impl DeviceListResponse {
    pub fn validate(&self) -> Result<(), HubClientError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(HubClientError::ProtocolMismatch);
        }
        if self.devices.len() > 1000 {
            return Err(HubClientError::InvalidResponse);
        }
        let mut seen = HashSet::with_capacity(self.devices.len());
        for device in &self.devices {
            device
                .validate()
                .map_err(|_| HubClientError::InvalidResponse)?;
            if !seen.insert(device.device_id.as_str()) {
                return Err(HubClientError::InvalidResponse);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct RenameDeviceResponse {
    pub protocol_version: u32,
    pub device_id: String,
    pub display_name: String,
}

impl RenameDeviceResponse {
    pub fn validate(&self) -> Result<(), HubClientError> {
        if self.protocol_version != PROTOCOL_VERSION {
            return Err(HubClientError::ProtocolMismatch);
        }
        if self.device_id.len() < 8
            || self.device_id.len() > 128
            || !self.device_id.as_bytes()[0].is_ascii_alphanumeric()
            || !self
                .device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(HubClientError::InvalidResponse);
        }
        if self.display_name.trim().is_empty()
            || self.display_name.chars().count() > 64
            || self.display_name.chars().any(char::is_control)
        {
            return Err(HubClientError::InvalidResponse);
        }
        Ok(())
    }
}

#[derive(Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PairingCodeResponse {
    pub pairing_code: String,
    pub expires_at_ms: i64,
}

impl PairingCodeResponse {
    pub fn validate(&self) -> Result<(), &'static str> {
        let valid_code = self.pairing_code.len() == 52
            && self
                .pairing_code
                .bytes()
                .all(|b| b.is_ascii_uppercase() || (b'2'..=b'7').contains(&b));
        if !valid_code || self.expires_at_ms < 0 {
            return Err("pairing_code_response");
        }
        Ok(())
    }
}

impl fmt::Debug for PairingCodeResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PairingCodeResponse")
            .field("pairing_code", &"[REDACTED]")
            .field("expires_at_ms", &self.expires_at_ms)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> PairRequest {
        PairRequest {
            pairing_code: "A".repeat(52),
            device_id: "office-pc-01".into(),
            display_name: "Office PC".into(),
            os_name: "windows".into(),
            app_version: "0.1.0".into(),
            time_zone: "Asia/Shanghai".into(),
        }
    }

    #[test]
    fn validates_pair_request() {
        assert!(request().validate().is_ok());
    }

    #[test]
    fn rejects_invalid_device_id() {
        let mut r = request();
        r.device_id = "short".into();
        assert_eq!(r.validate(), Err("device_id"));
    }

    #[test]
    fn pairing_request_debug_is_redacted() {
        let request = request();
        assert!(!format!("{request:?}").contains(&request.pairing_code));
    }

    #[test]
    fn token_debug_is_redacted() {
        let token = DeviceToken::new("A".repeat(43)).unwrap();
        assert!(!format!("{token:?}").contains(&"A".repeat(43)));
        assert!(format!("{token:?}").contains("REDACTED"));
    }

    #[test]
    fn pair_response_debug_does_not_leak_token() {
        let response = PairResponse {
            protocol_version: 1,
            device_id: "office-pc-01".into(),
            device_token: DeviceToken::new("B".repeat(43)).unwrap(),
            created_at_ms: 1,
        };
        let debug = format!("{response:?}");
        assert!(!debug.contains(&"B".repeat(43)));
        assert!(response.validate().is_ok());
    }

    #[test]
    fn rotate_response_debug_redacts_token() {
        let response = RotateTokenResponse {
            protocol_version: 1,
            device_id: "office-pc-01".into(),
            device_token: DeviceToken::new("D".repeat(43)).unwrap(),
            created_at_ms: 10,
        };
        assert!(!format!("{response:?}").contains(&"D".repeat(43)));
        assert!(response.validate().is_ok());
    }

    #[test]
    fn device_list_rejects_duplicate_ids_and_unknown_protocol() {
        let device = DeviceSummary {
            device_id: "office-pc-01".into(),
            display_name: "Office PC".into(),
            os_name: "windows".into(),
            app_version: "0.1.0".into(),
            time_zone: "UTC".into(),
            created_at_ms: 1,
            last_seen_ms: None,
            revoked_at_ms: None,
        };
        let duplicate = DeviceListResponse {
            protocol_version: 1,
            devices: vec![device.clone(), device],
        };
        assert!(duplicate.validate().is_err());
        let incompatible = DeviceListResponse {
            protocol_version: 99,
            devices: vec![],
        };
        assert_eq!(
            incompatible.validate(),
            Err(HubClientError::ProtocolMismatch)
        );
    }

    #[test]
    fn pairing_code_response_debug_is_redacted() {
        let response = PairingCodeResponse {
            pairing_code: "C".repeat(52),
            expires_at_ms: 10,
        };
        assert!(!format!("{response:?}").contains(&response.pairing_code));
        assert!(response.validate().is_ok());
    }
}
