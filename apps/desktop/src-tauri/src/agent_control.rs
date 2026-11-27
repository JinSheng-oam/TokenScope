use crate::paths::agent_is_running;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokenscope_local_ipc::AGENT_CONTROL_DIRECTORY;

const MAX_RESPONSE_BYTES: usize = 8192;

pub(crate) fn next_request_id() -> String {
    static REQUEST_SEQUENCE: AtomicU64 = AtomicU64::new(1);
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!("{:x}-{:x}-{:x}", std::process::id(), timestamp, sequence)
}

/// Sends an atomic JSON control request to the locked Agent process via `agent-control/`
/// and waits up to `timeout` for the matching response file.
pub(crate) fn send_control_request<Req, Resp>(
    db_path: &Path,
    request_prefix: &str,
    response_prefix: &str,
    request_id: &str,
    request: &Req,
    timeout: Duration,
    action_label: &str,
) -> Result<Resp, String>
where
    Req: Serialize,
    Resp: DeserializeOwned,
{
    if !agent_is_running(db_path)? {
        return Err("本地采集器未运行，请刷新后重试".into());
    }
    let directory = db_path
        .parent()
        .ok_or("无效数据库路径")?
        .join(AGENT_CONTROL_DIRECTORY);
    fs::create_dir_all(&directory).map_err(|e| format!("创建 Agent 控制目录失败：{e}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|e| format!("设置 Agent 控制目录权限失败：{e}"))?;
    }

    let body = serde_json::to_vec(request)
        .map_err(|e| format!("编码{action_label}请求失败：{e}"))?;
    let temporary = directory.join(format!("{request_prefix}{request_id}.tmp"));
    let request_path = directory.join(format!("{request_prefix}{request_id}.json"));
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write_request = (|| -> io::Result<()> {
        let mut file = options.open(&temporary)?;
        file.write_all(&body)?;
        file.sync_all()?;
        fs::rename(&temporary, &request_path)
    })();
    if let Err(error) = write_request {
        let _ = fs::remove_file(&temporary);
        return Err(format!("提交{action_label}请求失败：{error}"));
    }

    let response_path = directory.join(format!("{response_prefix}{request_id}.json"));
    let deadline = Instant::now() + timeout;
    loop {
        match fs::read(&response_path) {
            Ok(bytes) => {
                let _ = fs::remove_file(&response_path);
                if bytes.len() > MAX_RESPONSE_BYTES {
                    return Err(format!("Agent {action_label}响应超出大小限制"));
                }
                return serde_json::from_slice(&bytes)
                    .map_err(|e| format!("读取 Agent {action_label}响应失败：{e}"));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                let _ = fs::remove_file(&request_path);
                return Err(format!("读取 Agent {action_label}响应失败：{error}"));
            }
        }
        if Instant::now() >= deadline {
            let _ = fs::remove_file(&request_path);
            return Err(format!(
                "Agent {} 秒内未处理{action_label}请求，请稍后重试",
                timeout.as_secs()
            ));
        }
        thread::sleep(Duration::from_millis(75));
    }
}
