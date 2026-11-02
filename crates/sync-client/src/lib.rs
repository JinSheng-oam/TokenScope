//! Secure client primitives for the self-hosted TokenScope Hub.
//!
//! The crate deliberately does not persist credentials in SQLite or expose
//! them through Debug/Display. HTTP requests are direct, TLS-validated, do not
//! use ambient proxies, and never follow redirects.

pub mod client;
pub mod credentials;
pub mod pairing;
pub mod sync;
pub mod url;
pub mod worker;

pub use client::{HubClient, HubClientError};
pub use credentials::{CredentialKey, KeyringCredentialStore, SecretStore};
pub use url::HubUrl;

/// Build-time module identifier.
pub const MODULE_ID: &str = "sync-client";

#[cfg(test)]
mod tests {
    use super::MODULE_ID;

    #[test]
    fn reports_module_id() {
        assert_eq!(MODULE_ID, "sync-client");
    }
}
