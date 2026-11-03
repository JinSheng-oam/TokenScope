//! Best-effort Antigravity local quota RPC.
//!
//! Security contract:
//! - Discover only Antigravity language-server processes.
//! - Connect only to 127.0.0.1; proxies and redirects are disabled.
//! - A local CSRF token is held in memory only and is never logged or persisted.
//! - No Google OAuth, browser cookies, or remote endpoints are used.
//! - Quota snapshots are stored separately from Token usage/outbox.
use chrono::DateTime;
use reqwest::blocking::{Client, Response};
use reqwest::redirect::Policy;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::io::Read;
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokenscope_local_store::{AntigravityQuota, LocalStore};

const SERVICE: &str = "exa.language_server_pb.LanguageServerService";
const LOCAL_SOURCE_ID: &str = "local_antigravity";
const MAX_RESPONSE_BYTES: usize = 256 * 1024;
const MAX_PORTS: usize = 8;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, PartialEq, Eq)]
struct ProcessInfo {
    pid: u32,
    kind: ProcessKind,
    csrf: String,
    hub_port: Option<u16>,
    extension_port: Option<u16>,
    extension_csrf: Option<String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ProcessKind {
    App,
    Cli,
    Ide,
}
#[derive(Clone, PartialEq, Eq)]
struct Endpoint {
    port: u16,
    csrf: String,
}

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

fn flag_value(command: &str, flag: &str) -> Option<String> {
    let tokens = shell_words(command);
    for (i, token) in tokens.iter().enumerate() {
        if token == flag {
            return tokens.get(i + 1).cloned();
        }
        if let Some(value) = token.strip_prefix(&format!("{flag}=")) {
            return Some(value.to_string());
        }
    }
    None
}

// Small bounded command-line tokenizer: handles quoted executable paths and
// ordinary flag values; it does not execute or re-interpret the command line.
fn shell_words(input: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else if ch == '\\'
                && q == '"'
                && matches!(chars.peek().copied(), Some('"') | Some('\\'))
            {
                // Only consume a backslash escape when it escapes a quote or
                // another backslash. Preserve Windows path separators.
                current.push(chars.next().unwrap_or('\\'));
            } else {
                current.push(ch);
            }
        } else if ch == '\'' || ch == '"' {
            quote = Some(ch);
        } else if ch.is_whitespace() {
            if !current.is_empty() {
                out.push(std::mem::take(&mut current));
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

fn classify_process(line: &str) -> Option<ProcessInfo> {
    let trimmed = line.trim();
    let split = trimmed.find(char::is_whitespace)?;
    let pid: u32 = trimmed[..split].parse().ok()?;
    if pid == 0 {
        return None;
    }
    let command = trimmed[split..].trim();
    let lower = command.to_ascii_lowercase();
    let words = shell_words(command);
    let exe = words.first()?.to_ascii_lowercase();
    let app_data = flag_value(command, "--app_data_dir")
        .unwrap_or_default()
        .to_ascii_lowercase();
    let is_ls = lower.contains("language_server") || lower.contains("language-server");
    let is_cli = exe.ends_with("/agy")
        || exe.ends_with("\\agy.exe")
        || exe == "agy"
        || exe.ends_with("/antigravity-cli")
        || exe.ends_with("\\antigravity-cli.exe")
        || lower.contains("/antigravity-cli/")
        || lower.contains("\\antigravity-cli\\");
    let is_ide = lower.contains("antigravity ide.app")
        || lower.contains("/extensions/antigravity/")
        || app_data == "antigravity-ide";
    let is_antigravity = app_data == "antigravity"
        || app_data == "antigravity-ide"
        || lower.contains("/antigravity.app/")
        || lower.contains("\\antigravity\\")
        || lower.contains("/.antigravity/")
        || lower.contains("/antigravity-cli/")
        || is_ide;
    let hub_mode = words
        .iter()
        .any(|w| w == "--hub" || w.starts_with("--hub="));
    let kind = if is_cli
        && (lower.contains("language-server") || lower.contains("language_server") || hub_mode)
    {
        ProcessKind::Cli
    } else if is_ls && is_antigravity {
        if is_ide {
            ProcessKind::Ide
        } else {
            ProcessKind::App
        }
    } else {
        return None;
    };
    let csrf = flag_value(command, "--csrf_token").unwrap_or_default();
    // Desktop/IDE and hub-mode CLI require CSRF. A standalone CLI LS may not.
    if (kind != ProcessKind::Cli || hub_mode) && csrf.is_empty() {
        return None;
    }
    let hub_port = flag_value(command, "--hub-port")
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|p| *p > 0);
    let extension_port = flag_value(command, "--extension_server_port")
        .and_then(|v| v.parse::<u16>().ok())
        .filter(|p| *p > 0);
    let extension_csrf =
        flag_value(command, "--extension_server_csrf_token").filter(|v| !v.is_empty());
    Some(ProcessInfo {
        pid,
        kind,
        csrf,
        hub_port,
        extension_port,
        extension_csrf,
    })
}

fn run_capture(program: &str, args: &[String], timeout: Duration) -> Result<String, RpcError> {
    let mut child = Command::new(program)
        .args(args)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| RpcError::Discovery)?;
    let stdout = child.stdout.take().ok_or(RpcError::Discovery)?;
    // Drain stdout concurrently: waiting for child exit before reading can
    // deadlock when a process list exceeds the OS pipe buffer.
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let mut limited = stdout.take((1024 * 1024 + 1) as u64);
        limited.read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = std::time::Instant::now();
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(RpcError::Discovery);
            }
        };
        match status {
            Some(status) => {
                let bytes = reader
                    .join()
                    .map_err(|_| RpcError::Discovery)?
                    .map_err(|_| RpcError::Discovery)?;
                if bytes.len() > 1024 * 1024 || !status.success() {
                    return Err(RpcError::Discovery);
                }
                return String::from_utf8(bytes).map_err(|_| RpcError::Discovery);
            }
            None if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(RpcError::Discovery);
            }
            None => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

fn detect_processes() -> Result<Vec<ProcessInfo>, RpcError> {
    #[cfg(windows)]
    let output = run_capture("powershell", &[
        "-NoProfile".into(), "-NonInteractive".into(), "-Command".into(),
        "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); Get-CimInstance Win32_Process | Where-Object { $_.Name -like 'language_server*' -or $_.Name -like 'language-server*' -or $_.Name -like 'agy*' -or $_.Name -like 'antigravity*' } | ForEach-Object { \"$($_.ProcessId) $($_.CommandLine)\" }".into()
    ], Duration::from_secs(5))?;
    #[cfg(not(windows))]
    let output = run_capture(
        "ps",
        &["-ax".into(), "-o".into(), "pid=,command=".into()],
        Duration::from_secs(5),
    )?;

    let mut by_kind = std::collections::BTreeMap::<ProcessKind, ProcessInfo>::new();
    for info in output.lines().filter_map(classify_process) {
        by_kind.entry(info.kind).or_insert(info);
    }
    if by_kind.is_empty() {
        return Err(RpcError::NotRunning);
    }
    Ok(by_kind.into_values().collect())
}

fn listening_ports(pid: u32) -> Result<Vec<u16>, RpcError> {
    #[cfg(windows)]
    // `Get-NetTCPConnection` regularly takes several seconds on Windows while
    // `netstat` returns the same listener table in milliseconds.
    let output = run_capture(
        "netstat",
        &["-ano".into(), "-p".into(), "TCP".into()],
        Duration::from_secs(5),
    )?;
    #[cfg(not(windows))]
    let output = {
        let lsof = run_capture(
            "lsof",
            &[
                "-nP".into(),
                "-iTCP".into(),
                "-sTCP:LISTEN".into(),
                "-a".into(),
                "-p".into(),
                pid.to_string(),
            ],
            Duration::from_secs(2),
        );
        match lsof {
            Ok(s) if !s.trim().is_empty() => s,
            _ => {
                let ss = run_capture("ss", &["-ltnp".into()], Duration::from_secs(2))?;
                ss.lines()
                    .filter(|line| line.contains(&format!("pid={pid},")))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    };
    let mut ports = BTreeSet::new();
    #[cfg(windows)]
    for line in output.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 || !parts[3].eq_ignore_ascii_case("LISTENING") {
            continue;
        }
        if parts[4].parse::<u32>() != Ok(pid) {
            continue;
        }
        if let Some((_, port)) = parts[1].rsplit_once(':') {
            if let Ok(port) = port.parse::<u16>() {
                if port > 0 {
                    ports.insert(port);
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        for cap in output.split_whitespace() {
            if let Some((_, tail)) = cap.rsplit_once(':') {
                let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                if let Ok(port) = digits.parse::<u16>() {
                    if port > 0 {
                        ports.insert(port);
                    }
                }
            }
        }
    }
    if ports.is_empty() {
        return Err(RpcError::Discovery);
    }
    Ok(ports.into_iter().take(MAX_PORTS).collect())
}

fn build_client() -> Result<Client, RpcError> {
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

fn call_rpc(
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

fn fraction(value: &Value) -> Option<u16> {
    let v = value.as_f64()?;
    if !v.is_finite() || !(0.0..=1.0).contains(&v) {
        return None;
    }
    Some((v * 10_000.0).round() as u16)
}
fn remaining_fraction(bucket: &Value) -> Option<u16> {
    if bucket.get("disabled").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    bucket
        .get("remainingFraction")
        .and_then(fraction)
        .or_else(|| {
            bucket
                .pointer("/remaining/remainingFraction")
                .and_then(fraction)
        })
        .or_else(|| {
            if bucket.pointer("/remaining/case").and_then(Value::as_str)
                == Some("remainingFraction")
            {
                bucket.pointer("/remaining/value").and_then(fraction)
            } else {
                None
            }
        })
}
fn parse_time(value: &Value) -> Option<i64> {
    if let Some(n) = value.as_i64() {
        return Some(if n > 20_000_000_000 {
            n
        } else {
            n.saturating_mul(1000)
        })
        .filter(|n| *n >= 0);
    }
    let s = value.as_str()?;
    let parsed = DateTime::parse_from_rfc3339(s).ok()?;
    let ms = parsed.timestamp_millis();
    (ms >= 0).then_some(ms)
}
fn pool_id(display: &str) -> String {
    let lower = display.to_ascii_lowercase();
    if lower.contains("argon") {
        return "gemini_4_argon".into();
    }
    if lower.contains("claude") || lower.contains("gpt") {
        return "claude_gpt".into();
    }
    if lower.contains("gemini") {
        return "gemini".into();
    }
    // Preserve unknown upstream group identity instead of collapsing every
    // future provider into a single `other` bucket.
    let mut slug = String::new();
    for ch in lower.chars().take(128) {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch);
        } else if !slug.is_empty() && !slug.ends_with('_') {
            slug.push('_');
        }
        if slug.len() >= 64 {
            break;
        }
    }
    let slug = slug.trim_matches('_');
    if slug.is_empty() {
        "other".into()
    } else {
        slug.to_string()
    }
}
fn window_id(bucket: &Value) -> Option<&'static str> {
    let values = ["window", "bucketId", "displayName"];
    for key in values {
        let raw = bucket
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_ascii_lowercase()
            .replace('_', "-");
        if raw.contains("weekly") {
            return Some("weekly");
        }
        if raw.contains("daily") {
            return Some("daily");
        }
        if raw.contains("session")
            || raw.contains("5h")
            || raw.contains("5-hour")
            || raw.contains("five hour")
            || raw.contains("five-hour")
        {
            return Some("five_hour");
        }
    }
    None
}
fn parse_grouped_quota(
    payload: &Value,
    source_id: &str,
    observed_at_ms: i64,
) -> Vec<AntigravityQuota> {
    let root = payload
        .get("response")
        .or_else(|| payload.get("summary"))
        .unwrap_or(payload);
    let Some(groups) = root.get("groups").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut rows = std::collections::BTreeMap::<(String, String), AntigravityQuota>::new();
    for group in groups.iter().take(32) {
        let name = group
            .get("displayName")
            .and_then(Value::as_str)
            .unwrap_or("other");
        let pool = pool_id(name);
        let Some(buckets) = group.get("buckets").and_then(Value::as_array) else {
            continue;
        };
        for bucket in buckets.iter().take(16) {
            if rows.len() >= 200 {
                break;
            }
            let Some(window) = window_id(bucket) else {
                continue;
            };
            let disabled = bucket.get("disabled").and_then(Value::as_bool) == Some(true);
            let remaining = remaining_fraction(bucket);
            // Missing/invalid percentages must not erase a previously known
            // value. Explicitly disabled pools are retained as unknown.
            if !disabled && remaining.is_none() {
                continue;
            }
            let row = AntigravityQuota {
                source_id: source_id.to_string(),
                pool_id: pool.clone(),
                window_id: window.to_string(),
                remaining_basis_points: remaining,
                reset_at_ms: bucket.get("resetTime").and_then(parse_time),
                observed_at_ms,
            };
            let key = (row.pool_id.clone(), row.window_id.clone());
            match rows.get(&key) {
                // If upstream splits one pool across multiple groups, preserve
                // the lowest remaining quota (conservative); tie-break by the
                // earliest known reset time.
                Some(old) => {
                    let old_remaining = old.remaining_basis_points.unwrap_or(u16::MAX);
                    let new_remaining = row.remaining_basis_points.unwrap_or(u16::MAX);
                    let old_reset = old.reset_at_ms.unwrap_or(i64::MAX);
                    let new_reset = row.reset_at_ms.unwrap_or(i64::MAX);
                    if new_remaining < old_remaining
                        || (new_remaining == old_remaining && new_reset < old_reset)
                    {
                        rows.insert(key, row);
                    }
                }
                None => {
                    rows.insert(key, row);
                }
            }
        }
        if rows.len() >= 200 {
            break;
        }
    }
    rows.into_values().collect()
}

fn parse_legacy_quota(
    payload: &Value,
    source_id: &str,
    observed_at_ms: i64,
) -> Vec<AntigravityQuota> {
    const BLACKLIST: &[&str] = &[
        "MODEL_CHAT_20706",
        "MODEL_CHAT_23310",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH_THINKING",
        "MODEL_GOOGLE_GEMINI_2_5_FLASH_LITE",
        "MODEL_GOOGLE_GEMINI_2_5_PRO",
        "MODEL_PLACEHOLDER_M19",
        "MODEL_PLACEHOLDER_M9",
        "MODEL_PLACEHOLDER_M12",
    ];
    let configs = payload
        .pointer("/userStatus/cascadeModelConfigData/clientModelConfigs")
        .or_else(|| {
            payload.pointer("/response/userStatus/cascadeModelConfigData/clientModelConfigs")
        })
        .or_else(|| payload.pointer("/response/clientModelConfigs"))
        .or_else(|| payload.get("clientModelConfigs"))
        .and_then(Value::as_array);
    let Some(configs) = configs else {
        return Vec::new();
    };
    let mut rows = std::collections::BTreeMap::<String, AntigravityQuota>::new();
    for cfg in configs.iter().take(512) {
        let model_id = cfg
            .pointer("/modelOrAlias/model")
            .and_then(Value::as_str)
            .unwrap_or("");
        if model_id.is_empty() || BLACKLIST.contains(&model_id) {
            continue;
        }
        let label = cfg.get("label").and_then(Value::as_str).unwrap_or(model_id);
        let Some(quota) = cfg.get("quotaInfo") else {
            continue;
        };
        let Some(remaining) = quota.get("remainingFraction").and_then(fraction) else {
            continue;
        };
        let pool = pool_id(&format!("{label} {model_id}"));
        let row = AntigravityQuota {
            source_id: source_id.to_string(),
            pool_id: pool.clone(),
            window_id: "other".into(),
            remaining_basis_points: Some(remaining),
            reset_at_ms: quota.get("resetTime").and_then(parse_time),
            observed_at_ms,
        };
        match rows.get(&pool) {
            Some(old) if old.remaining_basis_points.unwrap_or(u16::MAX) <= remaining => {}
            _ => {
                rows.insert(pool, row);
            }
        }
    }
    rows.into_values().collect()
}

// Quota snapshots are device-local and not synchronized. We intentionally do
// not persist a Google email or a reversible hash of it; one local Antigravity
// account is represented by one latest-snapshot namespace.
fn local_source_id() -> &'static str {
    LOCAL_SOURCE_ID
}

fn discover_endpoints() -> Result<Vec<Endpoint>, RpcError> {
    let infos = detect_processes()?;
    // `detect_processes` returns at most one process per source kind, so
    // multiple app instances cannot starve a running CLI or IDE server.
    let selected = infos;
    let mut endpoints = Vec::new();
    for info in selected {
        let mut ports = BTreeSet::new();
        if let Some(p) = info.hub_port {
            ports.insert(p);
        }
        if let Ok(found) = listening_ports(info.pid) {
            ports.extend(found);
        }
        for port in ports.into_iter().take(MAX_PORTS) {
            endpoints.push(Endpoint {
                port,
                csrf: info.csrf.clone(),
            });
        }
        if let Some(port) = info.extension_port {
            endpoints.push(Endpoint {
                port,
                csrf: info
                    .extension_csrf
                    .clone()
                    .unwrap_or_else(|| info.csrf.clone()),
            });
        }
    }
    if endpoints.is_empty() {
        return Err(RpcError::Discovery);
    }
    let mut seen = std::collections::HashSet::new();
    endpoints.retain(|endpoint| seen.insert((endpoint.port, endpoint.csrf.clone())));
    endpoints.truncate(MAX_PORTS);
    Ok(endpoints)
}

fn fetch_quota() -> Result<Vec<AntigravityQuota>, RpcError> {
    let endpoints = discover_endpoints()?;
    let client = build_client()?;
    let observed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RpcError::InvalidResponse)?
        .as_millis();
    let observed_at_ms = i64::try_from(observed).map_err(|_| RpcError::InvalidResponse)?;
    let started = std::time::Instant::now();
    let mut last = RpcError::NoQuota;
    for endpoint in endpoints.into_iter().take(6) {
        if started.elapsed() > Duration::from_secs(12) {
            break;
        }
        for scheme in ["https", "http"] {
            if started.elapsed() > Duration::from_secs(12) {
                break;
            }
            match call_rpc(
                &client,
                &endpoint,
                scheme,
                "RetrieveUserQuotaSummary",
                &json!({"forceRefresh": true}),
            ) {
                Ok(payload) => {
                    let rows = parse_grouped_quota(&payload, local_source_id(), observed_at_ms);
                    if !rows.is_empty() {
                        return Ok(rows);
                    }
                    last = RpcError::NoQuota;
                }
                Err(e @ RpcError::Unauthorized) => last = e,
                Err(e) => last = e,
            }
            // Older language-server builds may expose only user-status model
            // configs. Keep the fallback on the same local endpoint and never
            // infer a five-hour/weekly window when upstream does not say so.
            if started.elapsed() <= Duration::from_secs(12) {
                if let Ok(status) = call_rpc(
                    &client,
                    &endpoint,
                    scheme,
                    "GetUserStatus",
                    &json!({"metadata": {
                        "ideName": "antigravity", "extensionName": "antigravity", "ideVersion": "unknown", "locale": "en"
                    }}),
                ) {
                    let rows = parse_legacy_quota(&status, local_source_id(), observed_at_ms);
                    if !rows.is_empty() {
                        return Ok(rows);
                    }
                }
                if let Ok(configs) = call_rpc(
                    &client,
                    &endpoint,
                    scheme,
                    "GetCommandModelConfigs",
                    &json!({"metadata": {
                        "ideName": "antigravity", "extensionName": "antigravity", "ideVersion": "unknown", "locale": "en"
                    }}),
                ) {
                    let rows = parse_legacy_quota(&configs, local_source_id(), observed_at_ms);
                    if !rows.is_empty() {
                        return Ok(rows);
                    }
                }
            }
        }
    }
    Err(last)
}

pub fn refresh_and_store(store: &mut LocalStore) -> Result<usize, RpcError> {
    let rows = fetch_quota()?;
    let received = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| RpcError::InvalidResponse)?
        .as_millis();
    let received = i64::try_from(received).map_err(|_| RpcError::InvalidResponse)?;
    store
        .upsert_antigravity_quotas(&rows, received)
        .map_err(RpcError::Store)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shell_tokenizer_preserves_quoted_windows_paths() {
        let line = r#""C:\Program Files\Antigravity\language_server.exe" --app_data_dir antigravity --csrf_token win-token"#;
        let p = classify_process(&format!("7777 {line}")).unwrap();
        assert_eq!(p.pid, 7777);
        assert_eq!(p.csrf, "win-token");
    }
    #[test]
    fn parses_antigravity_app_process_and_never_requires_token_in_output() {
        let line = "53602 /Applications/Antigravity.app/Contents/Resources/bin/language_server --csrf_token secret-value --app_data_dir antigravity --extension_server_port 12345 --extension_server_csrf_token ext-secret";
        let p = classify_process(line).unwrap();
        assert_eq!(p.pid, 53602);
        assert_eq!(p.kind, ProcessKind::App);
        assert_eq!(p.csrf, "secret-value");
        assert_eq!(p.extension_port, Some(12345));
    }
    #[test]
    fn rejects_unrelated_and_tokenless_desktop_processes() {
        assert!(classify_process("123 /bin/bash --login").is_none());
        assert!(classify_process("53602 /Applications/Antigravity.app/Contents/Resources/bin/language_server --app_data_dir antigravity").is_none());
    }
    #[test]
    fn allows_standalone_cli_without_csrf_but_not_hub_mode() {
        let cli = classify_process("60123 /Users/x/.antigravity/bin/agy language-server --stdio")
            .unwrap();
        assert_eq!(cli.kind, ProcessKind::Cli);
        assert!(cli.csrf.is_empty());
        assert!(classify_process(
            "60123 /Users/x/.antigravity/bin/agy --hub --hub-port=55555 --app_data_dir=antigravity"
        )
        .is_none());
    }
    #[test]
    fn parses_grouped_windows_without_confusing_zero_and_unknown() {
        let payload = json!({"groups":[
            {"displayName":"Gemini Models","buckets":[
                {"window":"5h","remainingFraction":0.0,"resetTime":"2026-11-02T12:00:00Z"},
                {"window":"weekly","remainingFraction":1.0}
            ]},
            {"displayName":"Claude and GPT models","buckets":[{"window":"weekly","disabled":true,"remainingFraction":1.0}]},
            {"displayName":"Gemini 4 Argon","buckets":[{"window":"weekly","remainingFraction":0.5}]}
        ]});
        let rows = parse_grouped_quota(&payload, "local_anonymous", 1_800_000_000_000);
        assert_eq!(rows.len(), 4);
        let gemini_5h = rows
            .iter()
            .find(|r| r.pool_id == "gemini" && r.window_id == "five_hour")
            .unwrap();
        let gemini_weekly = rows
            .iter()
            .find(|r| r.pool_id == "gemini" && r.window_id == "weekly")
            .unwrap();
        let claude_weekly = rows.iter().find(|r| r.pool_id == "claude_gpt").unwrap();
        let argon_weekly = rows.iter().find(|r| r.pool_id == "gemini_4_argon").unwrap();
        assert_eq!(gemini_5h.remaining_basis_points, Some(0));
        assert_eq!(gemini_weekly.remaining_basis_points, Some(10000));
        assert_eq!(claude_weekly.remaining_basis_points, None);
        assert_eq!(argon_weekly.window_id, "weekly");
    }
    #[test]
    fn grouped_duplicate_pool_keeps_lowest_remaining_quota() {
        let payload = json!({"groups":[
            {"displayName":"Gemini Models","buckets":[{"window":"weekly","remainingFraction":0.8}]},
            {"displayName":"Gemini Models","buckets":[{"window":"weekly","remainingFraction":0.4}]}
        ]});
        let rows = parse_grouped_quota(&payload, "local_antigravity", 1_800_000_000_000);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].remaining_basis_points, Some(4000));
    }
    #[test]
    fn unknown_group_keeps_a_safe_slug_and_invalid_fraction_is_skipped() {
        let payload = json!({"groups":[
            {"displayName":"Future Provider Plus","buckets":[{"window":"weekly","remainingFraction":0.5}]},
            {"displayName":"Gemini Models","buckets":[{"window":"weekly","remainingFraction":1.5}]}
        ]});
        let rows = parse_grouped_quota(&payload, "local_antigravity", 1_800_000_000_000);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].pool_id, "future_provider_plus");
    }
    #[test]
    fn parses_legacy_model_config_as_other_window_without_inventing_period() {
        let payload = json!({"userStatus":{"cascadeModelConfigData":{"clientModelConfigs":[
            {"label":"Gemini Pro","modelOrAlias":{"model":"gemini-pro"},"quotaInfo":{"remainingFraction":0.42,"resetTime":"2026-11-03T00:00:00Z"}},
            {"label":"Placeholder","modelOrAlias":{"model":"MODEL_PLACEHOLDER_M9"},"quotaInfo":{"remainingFraction":1.0}}
        ]}}});
        let rows = parse_legacy_quota(&payload, "local_anonymous", 1_800_000_000_000);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].window_id, "other");
        assert_eq!(rows[0].remaining_basis_points, Some(4200));
    }
    #[test]
    fn quota_source_identity_is_device_local_and_contains_no_account_data() {
        assert_eq!(local_source_id(), "local_antigravity");
        assert!(!local_source_id().contains('@'));
    }
    #[test]
    fn rejects_fraction_outside_range() {
        assert_eq!(fraction(&json!(1.2)), None);
        assert_eq!(fraction(&json!(-0.1)), None);
    }
    #[test]
    fn local_rpc_does_not_follow_redirects() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let redirect_target = TcpListener::bind("127.0.0.1:0").unwrap();
        let redirect_port = redirect_target.local_addr().unwrap().port();
        redirect_target.set_nonblocking(true).unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 2048];
            let _ = stream.read(&mut request).unwrap();
            write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{redirect_port}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        let client = build_client().unwrap();
        let result = call_rpc(
            &client,
            &Endpoint {
                port,
                csrf: String::new(),
            },
            "http",
            "GetUserStatus",
            &json!({}),
        );
        assert!(matches!(result, Err(RpcError::Network)));
        server.join().unwrap();
        assert!(
            redirect_target.accept().is_err(),
            "redirect target must never be contacted"
        );
    }
    #[test]
    fn local_rpc_posts_to_loopback_with_csrf_and_parses_json() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut request = [0_u8; 4096];
            let n = stream.read(&mut request).unwrap();
            let text = String::from_utf8_lossy(&request[..n]);
            assert!(text.starts_with("POST /exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary HTTP/1.1"));
            assert!(text
                .to_ascii_lowercase()
                .contains("x-codeium-csrf-token: test-csrf"));
            let body = r#"{"groups":[]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
        });
        let client = build_client().unwrap();
        let response = call_rpc(
            &client,
            &Endpoint {
                port,
                csrf: "test-csrf".into(),
            },
            "http",
            "RetrieveUserQuotaSummary",
            &json!({"forceRefresh":true}),
        )
        .unwrap();
        assert_eq!(response["groups"].as_array().unwrap().len(), 0);
        server.join().unwrap();
    }
}
