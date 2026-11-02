use std::fmt;

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{pairing::DeviceToken, url::HubUrl, HubClientError};

const SERVICE_NAME: &str = "dev.tokenscope.desktop.hub";

/// Opaque, non-secret keyring locator. Its account field is a SHA-256 digest
/// of Hub origin + device ID, not the bearer credential itself.
#[derive(Clone, PartialEq, Eq)]
pub struct CredentialKey {
    account: String,
}

impl CredentialKey {
    pub fn for_device(hub: &HubUrl, device_id: &str) -> Result<Self, HubClientError> {
        if device_id.len() < 8
            || device_id.len() > 128
            || !device_id.as_bytes()[0].is_ascii_alphanumeric()
            || !device_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
        {
            return Err(HubClientError::InvalidRequest("device_id"));
        }
        let mut digest = Sha256::new();
        digest.update(b"tokenscope/hub-credential/v1\0");
        digest.update(hub.origin().as_bytes());
        digest.update([0]);
        digest.update(device_id.as_bytes());
        Ok(Self {
            account: hex::encode(digest.finalize()),
        })
    }

    fn account(&self) -> &str {
        &self.account
    }
}

impl fmt::Debug for CredentialKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CredentialKey")
            .field("account", &"[SCOPED]")
            .finish()
    }
}

/// Storage contract allows deterministic tests while production uses the OS
/// secure store. Implementations must never log the secret value.
pub trait SecretStore: Send + Sync {
    fn preflight(&self) -> Result<(), HubClientError>;
    fn save(&self, key: &CredentialKey, token: &DeviceToken) -> Result<(), HubClientError>;
    fn load(&self, key: &CredentialKey) -> Result<Option<DeviceToken>, HubClientError>;
    fn delete(&self, key: &CredentialKey) -> Result<bool, HubClientError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyringCredentialStore;

impl KeyringCredentialStore {
    fn entry(key: &CredentialKey) -> Result<keyring::Entry, HubClientError> {
        keyring::Entry::new(SERVICE_NAME, key.account())
            .map_err(|_| HubClientError::CredentialStoreUnavailable)
    }
}

impl SecretStore for KeyringCredentialStore {
    fn preflight(&self) -> Result<(), HubClientError> {
        let account = format!("probe-{}", Uuid::new_v4().simple());
        let entry = keyring::Entry::new(SERVICE_NAME, &account)
            .map_err(|_| HubClientError::CredentialStoreUnavailable)?;
        let probe = Zeroizing::new(Uuid::new_v4().to_string());
        entry
            .set_password(probe.as_str())
            .map_err(|_| HubClientError::CredentialStoreUnavailable)?;
        entry
            .delete_credential()
            .map_err(|_| HubClientError::CredentialStoreUnavailable)?;
        Ok(())
    }

    fn save(&self, key: &CredentialKey, token: &DeviceToken) -> Result<(), HubClientError> {
        token.validate().map_err(HubClientError::InvalidRequest)?;
        Self::entry(key)?
            .set_password(token.expose_for_authorization())
            .map_err(|_| HubClientError::CredentialStoreUnavailable)
    }

    fn load(&self, key: &CredentialKey) -> Result<Option<DeviceToken>, HubClientError> {
        match Self::entry(key)?.get_password() {
            Ok(value) => DeviceToken::new(value)
                .map(Some)
                .map_err(HubClientError::InvalidRequest),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(HubClientError::CredentialStoreUnavailable),
        }
    }

    fn delete(&self, key: &CredentialKey) -> Result<bool, HubClientError> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(_) => Err(HubClientError::CredentialStoreUnavailable),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct MemoryStore(Mutex<HashMap<String, String>>);

    impl SecretStore for MemoryStore {
        fn preflight(&self) -> Result<(), HubClientError> {
            Ok(())
        }
        fn save(&self, key: &CredentialKey, token: &DeviceToken) -> Result<(), HubClientError> {
            self.0
                .lock()
                .unwrap()
                .insert(key.account.clone(), token.expose_for_authorization().into());
            Ok(())
        }
        fn load(&self, key: &CredentialKey) -> Result<Option<DeviceToken>, HubClientError> {
            self.0
                .lock()
                .unwrap()
                .get(key.account())
                .cloned()
                .map(DeviceToken::new)
                .transpose()
                .map_err(HubClientError::InvalidRequest)
        }
        fn delete(&self, key: &CredentialKey) -> Result<bool, HubClientError> {
            Ok(self.0.lock().unwrap().remove(key.account()).is_some())
        }
    }

    #[test]
    fn credential_key_is_scoped_to_hub_and_device_without_exposing_values() {
        let a = HubUrl::parse("https://hub-a.example").unwrap();
        let b = HubUrl::parse("https://hub-b.example").unwrap();
        let first = CredentialKey::for_device(&a, "office-pc-01").unwrap();
        let same = CredentialKey::for_device(&a, "office-pc-01").unwrap();
        let other_hub = CredentialKey::for_device(&b, "office-pc-01").unwrap();
        let other_device = CredentialKey::for_device(&a, "home-pc-01").unwrap();
        assert_eq!(first, same);
        assert_ne!(first, other_hub);
        assert_ne!(first, other_device);
        assert!(!format!("{first:?}").contains(first.account()));
    }

    #[test]
    fn memory_secret_store_round_trips_and_deletes() {
        let hub = HubUrl::parse("https://hub.example").unwrap();
        let key = CredentialKey::for_device(&hub, "office-pc-01").unwrap();
        let token = DeviceToken::new("A".repeat(43)).unwrap();
        let store = MemoryStore::default();
        assert_eq!(store.load(&key).unwrap(), None);
        store.save(&key, &token).unwrap();
        assert_eq!(
            store
                .load(&key)
                .unwrap()
                .unwrap()
                .expose_for_authorization(),
            token.expose_for_authorization()
        );
        assert!(store.delete(&key).unwrap());
        assert!(!store.delete(&key).unwrap());
        assert_eq!(store.load(&key).unwrap(), None);
    }
}
