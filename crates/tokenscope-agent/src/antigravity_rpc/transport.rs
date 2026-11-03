use super::endpoint::Endpoint;
use super::error::RpcError;
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde_json::Value;
use std::io::Read;
use std::time::Duration;

const SERVICE: &str = "exa.language_server_pb.LanguageServerService";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

pub(super) fn build_client() -> Result<Client, RpcError> {
    Client::builder()
        .no_proxy()
        .redirect(Policy::none())
        // Antigravity's local language server may use a self-signed certificate.
        // This client is used exclusively with the hard-coded loopback host.
        .danger_accept_invalid_certs(true)
        .connect_timeout(Duration::from_secs(1))
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(|_| RpcError::Network)
}

pub(super) fn call_rpc(
    client: &Client,
    endpoint: &Endpoint,
    scheme: &str,
    method: &str,
    body: &Value,
) -> Result<Value, RpcError> {
    let url = format!("{scheme}://127.0.0.1:{}/{SERVICE}/{method}", endpoint.port);
    let mut request = client
        .post(url)
        .header("content-type", "application/json")
        .header("connect-protocol-version", "1")
        .header(
            "user-agent",
            concat!("TokenScope/", env!("CARGO_PKG_VERSION")),
        );
    if !endpoint.csrf.is_empty() {
        request = request.header("x-codeium-csrf-token", &endpoint.csrf);
    }
    let response = request.json(body).send().map_err(|_| RpcError::Network)?;
    read_json_response(response)
}

fn read_json_response(response: Response) -> Result<Value, RpcError> {
    if response.status() == reqwest::StatusCode::UNAUTHORIZED
        || response.status() == reqwest::StatusCode::FORBIDDEN
    {
        return Err(RpcError::Unauthorized);
    }
    if !response.status().is_success() {
        return Err(RpcError::Network);
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_RESPONSE_BYTES as u64)
    {
        return Err(RpcError::InvalidResponse);
    }
    let mut bytes = Vec::new();
    response
        .take((MAX_RESPONSE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| RpcError::Network)?;
    if bytes.len() > MAX_RESPONSE_BYTES {
        return Err(RpcError::InvalidResponse);
    }
    serde_json::from_slice(&bytes).map_err(|_| RpcError::InvalidResponse)
}
