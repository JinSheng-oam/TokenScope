use std::{fmt, io::Read, time::Duration};

use reqwest::{
    blocking::{Client, Response},
    header::{HeaderValue, AUTHORIZATION, CONTENT_TYPE},
    redirect::Policy,
    Method, StatusCode,
};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{
    pairing::{
        DeviceListResponse, DeviceToken, PairRequest, PairResponse, RenameDeviceResponse,
        RotateTokenResponse, PROTOCOL_VERSION,
    },
    sync::UsageChangesResponse,
    url::HubUrl,
};

const MAX_RESPONSE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubClientError {
    InvalidHubUrl,
    InsecureTransport,
    InvalidRequest(&'static str),
    CredentialStoreUnavailable,
    CredentialMissing,
    PairingCodeInvalid,
    Transport,
    PairOutcomeUnknown,
    ResponseTooLarge,
    RequestTooLarge,
    InvalidResponse,
    ProtocolMismatch,
    Unauthorized,
    DeviceRevoked,
    DeviceAlreadyRegistered,
    NotFound,
    RateLimited,
    ServerRejected(u16),
    ServerUnavailable,
}

impl fmt::Display for HubClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidHubUrl => f.write_str("Hub 地址无效：只允许 HTTPS 地址或本机回环 HTTP 地址"),
            Self::InsecureTransport => f.write_str("非本机 Hub 必须使用 HTTPS"),
            Self::InvalidRequest(field) => write!(f, "请求字段无效：{field}"),
            Self::CredentialStoreUnavailable => f.write_str("操作系统安全凭证库不可用，请解锁系统钥匙串或配置 Secret Service"),
            Self::CredentialMissing => f.write_str("本机尚未保存该 Hub 的设备凭证，请先配对"),
            Self::PairingCodeInvalid => f.write_str("配对码无效、已过期或已经使用，请在 Hub 重新生成配对码"),
            Self::Transport => f.write_str("无法连接 Hub，请检查地址、网络和 TLS 证书"),
            Self::PairOutcomeUnknown => f.write_str("配对请求连接中断，服务端是否已完成注册无法确认；请先在 Hub 设备列表核查，勿直接重复使用配对码"),
            Self::ResponseTooLarge => f.write_str("Hub 响应超过允许大小"),
            Self::RequestTooLarge => f.write_str("同步批次超过 2 MiB，请缩小批次"),
            Self::InvalidResponse => f.write_str("Hub 返回的数据格式无效"),
            Self::ProtocolMismatch => f.write_str("Hub 协议版本不兼容，请升级客户端或服务端"),
            Self::Unauthorized => f.write_str("设备凭证无效或已过期，请重新配对"),
            Self::DeviceRevoked => f.write_str("该设备已被 Hub 管理员撤销"),
            Self::DeviceAlreadyRegistered => f.write_str("该设备已注册，请勿覆盖本机身份"),
            Self::NotFound => f.write_str("Hub 上不存在该设备或接口"),
            Self::RateLimited => f.write_str("请求过于频繁，请稍后重试"),
            Self::ServerRejected(code) => write!(f, "Hub 拒绝请求（HTTP {code}）"),
            Self::ServerUnavailable => f.write_str("Hub 服务暂时不可用"),
        }
    }
}

impl std::error::Error for HubClientError {}

#[derive(Clone)]
pub struct HubClient {
    base: HubUrl,
    http: Client,
}

impl HubClient {
    pub fn new(base: HubUrl) -> Result<Self, HubClientError> {
        let http = Client::builder()
            .no_proxy()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .pool_max_idle_per_host(0)
            .user_agent(concat!(
                env!("CARGO_PKG_NAME"),
                "/",
                env!("CARGO_PKG_VERSION")
            ))
            .build()
            .map_err(|_| HubClientError::Transport)?;
        Ok(Self { base, http })
    }

    /// Pairing is deliberately not retried: a transport failure may occur
    /// after the Hub has consumed the one-time code and committed registration.
    pub fn pair(&self, request: &PairRequest) -> Result<PairResponse, HubClientError> {
        request.validate().map_err(HubClientError::InvalidRequest)?;
        let response = self
            .send_json::<_, PairResponse>(Method::POST, "/api/v1/pair", Some(request), None)
            .map_err(|error| match error {
                HubClientError::Unauthorized => HubClientError::PairingCodeInvalid,
                HubClientError::Transport
                | HubClientError::ResponseTooLarge
                | HubClientError::InvalidResponse => HubClientError::PairOutcomeUnknown,
                other => other,
            })?;
        if response.protocol_version != PROTOCOL_VERSION {
            return Err(HubClientError::ProtocolMismatch);
        }
        response
            .validate()
            .map_err(HubClientError::InvalidRequest)?;
        if response.device_id != request.device_id {
            return Err(HubClientError::ProtocolMismatch);
        }
        Ok(response)
    }

    pub fn list_devices(&self, token: &DeviceToken) -> Result<DeviceListResponse, HubClientError> {
        let response = self.send_json::<(), DeviceListResponse>(
            Method::GET,
            "/api/v1/devices",
            None,
            Some(token),
        )?;
        response.validate()?;
        Ok(response)
    }

    pub fn pull_usage_changes(
        &self,
        token: &DeviceToken,
        cursor: u64,
        limit: usize,
    ) -> Result<UsageChangesResponse, HubClientError> {
        if limit == 0 || limit > 500 {
            return Err(HubClientError::InvalidRequest("pull_limit"));
        }
        let path = format!("/api/v1/usage/changes?cursor={cursor}&limit={limit}");
        let response: UsageChangesResponse =
            self.send_json::<(), _>(Method::GET, &path, None, Some(token))?;
        response.validate(cursor, limit)?;
        Ok(response)
    }

    pub fn rename_self(
        &self,
        token: &DeviceToken,
        display_name: &str,
    ) -> Result<RenameDeviceResponse, HubClientError> {
        #[derive(Serialize)]
        struct Request<'a> {
            display_name: &'a str,
        }
        let name = display_name.trim();
        if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
            return Err(HubClientError::InvalidRequest("display_name"));
        }
        let response: RenameDeviceResponse = self.send_json(
            Method::PATCH,
            "/api/v1/devices/self",
            Some(&Request { display_name: name }),
            Some(token),
        )?;
        response.validate()?;
        Ok(response)
    }

    pub fn rotate_self_token(
        &self,
        token: &DeviceToken,
        expected_device_id: &str,
    ) -> Result<RotateTokenResponse, HubClientError> {
        #[derive(Serialize)]
        struct EmptyRequest {}
        let response = self.send_json::<EmptyRequest, RotateTokenResponse>(
            Method::POST,
            "/api/v1/devices/self/rotate-token",
            Some(&EmptyRequest {}),
            Some(token),
        )?;
        response.validate()?;
        if response.device_id != expected_device_id {
            return Err(HubClientError::ProtocolMismatch);
        }
        Ok(response)
    }

    pub(crate) fn send_json<B, R>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
        token: Option<&DeviceToken>,
    ) -> Result<R, HubClientError>
    where
        B: Serialize + ?Sized,
        R: DeserializeOwned,
    {
        let url = self.base.endpoint(path)?;
        let mut request = self.http.request(method, url);
        if let Some(value) = body {
            request = request.header(CONTENT_TYPE, "application/json").json(value);
        }
        if let Some(token) = token {
            let header = Zeroizing::new(format!("Bearer {}", token.expose_for_authorization()));
            let value = HeaderValue::from_str(header.as_str())
                .map_err(|_| HubClientError::InvalidRequest("device_token"))?;
            request = request.header(AUTHORIZATION, value);
        }
        let response = request.send().map_err(|_| HubClientError::Transport)?;
        let status = response.status();
        let bytes = Zeroizing::new(read_bounded(response)?);
        if !status.is_success() {
            return Err(map_status(status, &bytes));
        }
        let decoded: R =
            serde_json::from_slice(&bytes).map_err(|_| HubClientError::InvalidResponse)?;
        Ok(decoded)
    }
}

fn read_bounded(response: Response) -> Result<Vec<u8>, HubClientError> {
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES)
    {
        return Err(HubClientError::ResponseTooLarge);
    }
    let mut bytes = Vec::new();
    response
        .take(MAX_RESPONSE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HubClientError::Transport)?;
    if bytes.len() as u64 > MAX_RESPONSE_BYTES {
        return Err(HubClientError::ResponseTooLarge);
    }
    Ok(bytes)
}

#[derive(Deserialize)]
struct ErrorEnvelope {
    error: ErrorCode,
}
#[derive(Deserialize)]
struct ErrorCode {
    code: String,
}

fn map_status(status: StatusCode, body: &[u8]) -> HubClientError {
    let code = serde_json::from_slice::<ErrorEnvelope>(body)
        .ok()
        .map(|e| e.error.code);
    match status.as_u16() {
        401 if matches!(
            code.as_deref(),
            Some(
                "device_auth_required"
                    | "device_credential_invalid"
                    | "pairing_code_invalid_or_expired"
            )
        ) =>
        {
            HubClientError::Unauthorized
        }
        403 if matches!(code.as_deref(), Some("device_revoked")) => HubClientError::DeviceRevoked,
        409 if matches!(code.as_deref(), Some("device_already_registered")) => {
            HubClientError::DeviceAlreadyRegistered
        }
        404 => HubClientError::NotFound,
        429 => HubClientError::RateLimited,
        502..=504 => HubClientError::ServerUnavailable,
        value => HubClientError::ServerRejected(value),
    }
}

pub fn ensure_protocol_version(version: u32) -> Result<(), HubClientError> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(HubClientError::ProtocolMismatch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        thread,
    };

    fn mock_server(
        status: &'static str,
        body: &'static str,
        capture: Option<std::sync::mpsc::Sender<String>>,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut request = Vec::new();
                let mut buffer = [0u8; 4096];
                let mut header_end = None;
                let mut expected = 0usize;
                loop {
                    let n = stream.read(&mut buffer).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    request.extend_from_slice(&buffer[..n]);
                    if header_end.is_none() {
                        if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                            header_end = Some(pos + 4);
                            let headers = String::from_utf8_lossy(&request[..pos]);
                            expected = headers
                                .lines()
                                .find_map(|line| {
                                    let (key, value) = line.split_once(':')?;
                                    key.eq_ignore_ascii_case("content-length")
                                        .then(|| value.trim().parse::<usize>().ok())
                                        .flatten()
                                })
                                .unwrap_or(0);
                        }
                    }
                    if let Some(end) = header_end {
                        if request.len() >= end + expected {
                            break;
                        }
                    }
                }
                if let Some(sender) = capture {
                    let _ = sender.send(String::from_utf8_lossy(&request).to_string());
                }
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = stream.write_all(response.as_bytes());
            }
        });
        format!("http://{address}")
    }

    #[test]
    fn pairing_posts_once_and_returns_validated_token() {
        let (tx, rx) = std::sync::mpsc::channel();
        let address = mock_server(
            "201 Created",
            r#"{"protocol_version":1,"device_id":"office-pc-01","device_token":"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA","created_at_ms":10}"#,
            Some(tx),
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let request = PairRequest {
            pairing_code: "A".repeat(52),
            device_id: "office-pc-01".into(),
            display_name: "Office PC".into(),
            os_name: "linux".into(),
            app_version: "0.1.0".into(),
            time_zone: "UTC".into(),
        };
        let response = client.pair(&request).unwrap();
        assert_eq!(response.device_id, request.device_id);
        let wire = rx.recv().unwrap();
        assert!(wire.starts_with("POST /api/v1/pair HTTP/1.1"));
        assert!(wire.contains(&request.pairing_code));
    }

    #[test]
    fn revoked_and_expired_credentials_are_mapped_without_echoing_body() {
        let address = mock_server(
            "403 Forbidden",
            r#"{"error":{"code":"device_revoked"}}"#,
            None,
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("A".repeat(43)).unwrap();
        assert_eq!(
            client.list_devices(&token).unwrap_err(),
            HubClientError::DeviceRevoked
        );
    }

    #[test]
    fn redirects_are_not_followed() {
        let address = mock_server("302 Found", r#"{"error":{"code":"redirect"}}"#, None);
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("A".repeat(43)).unwrap();
        assert!(matches!(
            client.list_devices(&token),
            Err(HubClientError::ServerRejected(302))
        ));
    }

    #[test]
    fn response_body_is_not_used_as_an_error_message() {
        let address = mock_server(
            "401 Unauthorized",
            r#"{"error":{"code":"device_credential_invalid"},"secret":"DO_NOT_ECHO"}"#,
            None,
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("A".repeat(43)).unwrap();
        let error = client.list_devices(&token).unwrap_err();
        assert_eq!(error, HubClientError::Unauthorized);
        assert!(!error.to_string().contains("DO_NOT_ECHO"));
    }

    #[test]
    fn pairing_rejection_is_reported_as_invalid_code() {
        let address = mock_server(
            "401 Unauthorized",
            r#"{"error":{"code":"pairing_code_invalid_or_expired"}}"#,
            None,
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let request = PairRequest {
            pairing_code: "A".repeat(52),
            device_id: "office-pc-01".into(),
            display_name: "Office PC".into(),
            os_name: "linux".into(),
            app_version: "0.1.0".into(),
            time_zone: "UTC".into(),
        };
        assert_eq!(
            client.pair(&request).unwrap_err(),
            HubClientError::PairingCodeInvalid
        );
    }

    #[test]
    fn list_devices_sends_bearer_token_and_validates_payload() {
        let (tx, rx) = std::sync::mpsc::channel();
        let address = mock_server(
            "200 OK",
            r#"{"protocol_version":1,"devices":[{"device_id":"office-pc-01","display_name":"Office PC","os_name":"windows","app_version":"0.1.0","time_zone":"UTC","created_at_ms":1,"last_seen_ms":2,"revoked_at_ms":null}]}"#,
            Some(tx),
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("Z".repeat(43)).unwrap();
        let response = client.list_devices(&token).unwrap();
        assert_eq!(response.devices.len(), 1);
        let wire = rx.recv().unwrap();
        assert!(wire.starts_with("GET /api/v1/devices HTTP/1.1"));
        assert!(wire
            .to_ascii_lowercase()
            .contains(&format!("authorization: bearer {}", "Z".repeat(43)).to_ascii_lowercase()));
    }

    #[test]
    fn list_devices_rejects_incompatible_protocol() {
        let address = mock_server("200 OK", r#"{"protocol_version":2,"devices":[]}"#, None);
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("Z".repeat(43)).unwrap();
        assert_eq!(
            client.list_devices(&token).unwrap_err(),
            HubClientError::ProtocolMismatch
        );
    }

    #[test]
    fn rename_self_posts_only_display_name_and_returns_safe_result() {
        let (tx, rx) = std::sync::mpsc::channel();
        let address = mock_server(
            "200 OK",
            r#"{"protocol_version":1,"device_id":"office-pc-01","display_name":"New Office"}"#,
            Some(tx),
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("Z".repeat(43)).unwrap();
        let response = client.rename_self(&token, " New Office ").unwrap();
        assert_eq!(response.display_name, "New Office");
        let wire = rx.recv().unwrap();
        assert!(wire.starts_with("PATCH /api/v1/devices/self HTTP/1.1"));
        assert!(wire.contains(r#"{"display_name":"New Office"}"#));
    }

    #[test]
    fn rotate_self_token_posts_empty_object_and_redacts_returned_token() {
        let (tx, rx) = std::sync::mpsc::channel();
        let address = mock_server(
            "200 OK",
            r#"{"protocol_version":1,"device_id":"office-pc-01","device_token":"BBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBB","created_at_ms":20}"#,
            Some(tx),
        );
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("Z".repeat(43)).unwrap();
        let response = client.rotate_self_token(&token, "office-pc-01").unwrap();
        assert!(!format!("{response:?}").contains(&"B".repeat(43)));
        let wire = rx.recv().unwrap();
        assert!(wire.starts_with("POST /api/v1/devices/self/rotate-token HTTP/1.1"));
        assert!(wire.ends_with("{}"));
    }

    #[test]
    fn oversized_response_is_rejected() {
        let huge = "x".repeat((MAX_RESPONSE_BYTES + 1) as usize);
        let body: &'static str = Box::leak(huge.into_boxed_str());
        let address = mock_server("200 OK", body, None);
        let client = HubClient::new(HubUrl::parse(&address).unwrap()).unwrap();
        let token = DeviceToken::new("Z".repeat(43)).unwrap();
        assert_eq!(
            client.list_devices(&token).unwrap_err(),
            HubClientError::ResponseTooLarge
        );
    }
}
