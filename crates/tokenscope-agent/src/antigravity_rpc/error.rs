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
            Self::NotRunning => "Antigravity 本地服务未运行",
            Self::Discovery => "无法发现 Antigravity 本地 RPC 端点",
            Self::Unauthorized => "Antigravity 本地 RPC 拒绝了 CSRF 令牌",
            Self::NoQuota => "Antigravity 未返回支持的额度窗口",
            Self::InvalidResponse => "Antigravity 返回了无效的额度响应",
            Self::Network => "Antigravity 本地 RPC 请求失败",
            Self::Store(_) => "无法保存 Antigravity 额度快照",
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
