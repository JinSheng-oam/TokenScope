#[derive(Debug)]
pub enum RpcError {
    NotRunning,
    Discovery,
    Unauthorized,
    NoQuota,
    InvalidResponse,
    Network,
    Store(#[allow(dead_code)] tokenscope_local_store::StoreError),
}

impl std::fmt::Display for RpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            Self::NotRunning => "Antigravity language server is not running",
            Self::Discovery => "could not discover Antigravity local RPC endpoint",
            Self::Unauthorized => "Antigravity local RPC rejected the CSRF token",
            Self::NoQuota => "Antigravity returned no supported quota windows",
            Self::InvalidResponse => "Antigravity returned an invalid quota response",
            Self::Network => "Antigravity local RPC request failed",
            Self::Store(_) => "could not persist Antigravity quota snapshot",
        };
        f.write_str(text)
    }
}

impl std::error::Error for RpcError {}

impl From<tokenscope_local_store::StoreError> for RpcError {
    fn from(value: tokenscope_local_store::StoreError) -> Self {
        Self::Store(value)
    }
}
