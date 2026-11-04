use std::{
    collections::HashMap,
    fmt,
    sync::{Arc, Mutex},
};

use sha2::{Digest, Sha256};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const WEBDAV_SERVICE_NAME: &str = "dev.tokenscope.desktop.webdav";

#[derive(Clone, PartialEq, Eq)]
pub struct WebDavCredentialKey {
    account: String,
}

impl WebDavCredentialKey {
    pub fn for_endpoint(endpoint: &str, username: &str) -> Self {
        let mut digest = Sha256::new();
        digest.update(b"tokenscope/webdav-credential/v1\0");
        digest.update(endpoint.trim().as_bytes());
        digest.update([0]);
        digest.update(username.trim().as_bytes());
        Self {
            account: hex::encode(digest.finalize()),
        }
    }

    pub fn account(&self) -> &str {
        &self.account
    }
}

impl fmt::Debug for WebDavCredentialKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebDavCredentialKey")
            .field("account", &"[SCOPED]")
            .finish()
    }
}

#[derive(Clone)]
pub struct WebDavCredentials {
    pub username: String,
    pub password: Zeroizing<String>,
}

impl fmt::Debug for WebDavCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebDavCredentials")
            .field("username", &self.username)
            .field("password", &"[REDACTED]")
            .finish()
    }
}

pub trait WebDavSecretStore: Send + Sync {
    fn preflight(&self) -> Result<(), String>;
    fn save(&self, key: &WebDavCredentialKey, password: &str) -> Result<(), String>;
    fn load(&self, key: &WebDavCredentialKey) -> Result<Option<Zeroizing<String>>, String>;
    fn delete(&self, key: &WebDavCredentialKey) -> Result<bool, String>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct KeyringWebDavStore;

impl KeyringWebDavStore {
    fn entry(key: &WebDavCredentialKey) -> Result<keyring::Entry, String> {
        keyring::Entry::new(WEBDAV_SERVICE_NAME, key.account())
            .map_err(|e| format!("无法访问系统安全凭据库：{e}"))
    }
}

impl WebDavSecretStore for KeyringWebDavStore {
    fn preflight(&self) -> Result<(), String> {
        let account = format!("probe-{}", Uuid::new_v4().simple());
        let entry = keyring::Entry::new(WEBDAV_SERVICE_NAME, &account)
            .map_err(|e| format!("系统凭据库不可用：{e}"))?;
        let mut probe = Zeroizing::new(Uuid::new_v4().to_string());
        entry
            .set_password(probe.as_str())
            .map_err(|e| format!("写入系统凭据测试失败：{e}"))?;
        probe.zeroize();
        let _ = entry.delete_credential();
        Ok(())
    }

    fn save(&self, key: &WebDavCredentialKey, password: &str) -> Result<(), String> {
        Self::entry(key)?
            .set_password(password)
            .map_err(|e| format!("写入 WebDAV 凭据失败：{e}"))
    }

    fn load(&self, key: &WebDavCredentialKey) -> Result<Option<Zeroizing<String>>, String> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(format!("读取 WebDAV 凭据失败：{e}")),
        }
    }

    fn delete(&self, key: &WebDavCredentialKey) -> Result<bool, String> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(format!("删除 WebDAV 凭据失败：{e}")),
        }
    }
}

#[derive(Clone, Default)]
pub struct MemoryWebDavStore {
    items: Arc<Mutex<HashMap<String, Zeroizing<String>>>>,
}

impl MemoryWebDavStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl WebDavSecretStore for MemoryWebDavStore {
    fn preflight(&self) -> Result<(), String> {
        Ok(())
    }

    fn save(&self, key: &WebDavCredentialKey, password: &str) -> Result<(), String> {
        let mut map = self.items.lock().map_err(|_| "poisoned".to_string())?;
        map.insert(key.account().to_string(), Zeroizing::new(password.to_string()));
        Ok(())
    }

    fn load(&self, key: &WebDavCredentialKey) -> Result<Option<Zeroizing<String>>, String> {
        let map = self.items.lock().map_err(|_| "poisoned".to_string())?;
        Ok(map.get(key.account()).cloned())
    }

    fn delete(&self, key: &WebDavCredentialKey) -> Result<bool, String> {
        let mut map = self.items.lock().map_err(|_| "poisoned".to_string())?;
        Ok(map.remove(key.account()).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_webdav_store_crud() {
        let store = MemoryWebDavStore::new();
        let key = WebDavCredentialKey::for_endpoint("https://dav.example.com", "alice");
        assert_eq!(store.load(&key).unwrap(), None);

        store.save(&key, "secret_password_123").unwrap();
        let loaded = store.load(&key).unwrap();
        assert_eq!(loaded.as_ref().map(|s| s.as_str()), Some("secret_password_123"));

        let deleted = store.delete(&key).unwrap();
        assert!(deleted);
        assert_eq!(store.load(&key).unwrap(), None);
    }
}
