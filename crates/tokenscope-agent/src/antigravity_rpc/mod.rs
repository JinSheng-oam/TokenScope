//! Best-effort Antigravity local quota RPC.
//!
//! Security contract:
//! - Discover only Antigravity language-server processes.
//! - Connect only to 127.0.0.1; proxies and redirects are disabled.
//! - A local CSRF token is held in memory only and is never logged or persisted.
//! - No Google OAuth, browser cookies, or remote endpoints are used.
//! - Quota snapshots are stored separately from Token usage/outbox.

mod endpoint;
mod error;
mod process;
mod quota;
mod refresh;
mod transport;

#[cfg(test)]
mod tests;

pub(crate) use error::RpcError;

pub(crate) fn refresh_and_store(
    store: &mut tokenscope_local_store::LocalStore,
) -> Result<usize, RpcError> {
    refresh::refresh_and_store(store)
}

pub(crate) fn fetch_quota() -> Result<Vec<tokenscope_local_store::AntigravityQuota>, RpcError> {
    refresh::fetch_quota()
}
