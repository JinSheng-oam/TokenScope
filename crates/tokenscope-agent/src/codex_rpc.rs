//! Read Codex plan quota through Codex's own local app-server protocol.
//! The Agent never reads Codex auth files or handles OAuth tokens.

use serde_json::{json, Value};
use std::{
    io::{self, Read, Write},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokenscope_local_store::{CodexQuota, LocalStore};

const RESPONSE_LIMIT: u64 = 256 * 1024;
const PROCESS_TIMEOUT: Duration = Duration::from_secs(12);

fn observed_now_ms() -> Result<i64, &'static str> {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "system clock before epoch")?;
    i64::try_from(elapsed.as_millis()).map_err(|_| "system clock out of range")
}

fn window_from_value(value: &Value, pool_id: &str, observed_at_ms: i64) -> Option<CodexQuota> {
    if value.is_null() {
        return None;
    }
    let duration = value.get("windowDurationMins")?.as_u64()?;
    let window_duration_minutes = u32::try_from(duration).ok()?;
    if window_duration_minutes == 0 {
        return None;
    }

    let remaining_basis_points = match value.get("usedPercent") {
        None | Some(Value::Null) => None,
        Some(number) => {
            let used_percent = number.as_f64()?;
            if !used_percent.is_finite() || !(0.0..=100.0).contains(&used_percent) {
                return None;
            }
            let used_basis_points = (used_percent * 100.0).round() as u16;
            Some(10_000_u16.saturating_sub(used_basis_points))
        }
    };
    let reset_at_ms = match value.get("resetsAt") {
        None | Some(Value::Null) => None,
        Some(value) => {
            let seconds = value.as_i64()?;
            if seconds < 0 {
                return None;
            }
            Some(seconds.checked_mul(1000)?)
        }
    };
    Some(CodexQuota {
        source_id: "local_codex".into(),
        pool_id: pool_id.into(),
        window_duration_minutes,
        remaining_basis_points,
        reset_at_ms,
        observed_at_ms,
    })
}

fn parse_rate_limits(result: &Value, observed_at_ms: i64) -> Result<Vec<CodexQuota>, &'static str> {
    let limits = result
        .get("rateLimits")
        .or_else(|| {
            result
                .get("rateLimitsByLimitId")
                .and_then(|groups| groups.get("codex"))
        })
        .ok_or("Codex returned no rate limits")?;
    let pool_id = limits
        .get("limitId")
        .and_then(Value::as_str)
        .unwrap_or("codex");
    if pool_id != "codex" {
        return Err("Codex returned an unexpected quota pool");
    }
    let mut rows = Vec::with_capacity(2);
    for key in ["primary", "secondary"] {
        if let Some(value) = limits.get(key).filter(|v| !v.is_null()) {
            rows.push(
                window_from_value(value, pool_id, observed_at_ms)
                    .ok_or("Codex returned an invalid rate-limit window")?,
            );
        }
    }
    if rows.is_empty() {
        return Err("Codex returned no quota windows");
    }
    Ok(rows)
}

fn stream_protocol_lines<R: Read>(mut reader: R, sender: Sender<io::Result<Vec<u8>>>) {
    let mut pending = Vec::new();
    let mut total = 0_u64;
    let mut chunk = [0_u8; 4096];
    loop {
        let read = match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                let _ = sender.send(Err(error));
                return;
            }
        };
        total = total.saturating_add(read as u64);
        if total > RESPONSE_LIMIT {
            let _ = sender.send(Err(io::Error::other("Codex app-server response too large")));
            return;
        }
        pending.extend_from_slice(&chunk[..read]);
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let mut line = pending.drain(..=end).collect::<Vec<_>>();
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if sender.send(Ok(line)).is_err() {
                return;
            }
        }
    }
    if !pending.is_empty() {
        let _ = sender.send(Ok(pending));
    }
}

fn wait_for_response(
    receiver: &Receiver<io::Result<Vec<u8>>>,
    id: u64,
    deadline: Instant,
) -> Result<Value, &'static str> {
    loop {
        let timeout = deadline.saturating_duration_since(Instant::now());
        if timeout.is_zero() {
            return Err("Codex app-server timed out");
        }
        let line = receiver
            .recv_timeout(timeout)
            .map_err(|_| "Codex app-server returned no response")?
            .map_err(|_| "Codex app-server output was invalid or too large")?;
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            continue;
        };
        if message.get("id").and_then(Value::as_u64) == Some(id) {
            if message.get("error").is_some() {
                return Err("Codex quota is unavailable; check Codex sign-in");
            }
            return message
                .get("result")
                .cloned()
                .ok_or("Codex app-server returned an invalid response");
        }
    }
}

#[cfg(windows)]
fn codex_command() -> Command {
    use std::os::windows::process::CommandExt;
    let executable = std::env::var_os("TOKENSCOPE_CODEX_CLI_PATH").unwrap_or_else(|| {
        std::env::var_os("LOCALAPPDATA")
            .map(std::path::PathBuf::from)
            .map(|p| p.join("OpenAI").join("Codex").join("bin"))
            .and_then(|p| std::fs::read_dir(p).ok())
            .and_then(|entries| {
                entries
                    .flatten()
                    .map(|e| e.path().join("codex.exe"))
                    .filter(|p| p.is_file())
                    .max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok())
            })
            .map(|p| p.into_os_string())
            .unwrap_or_else(|| "codex.exe".into())
    });
    let mut command = Command::new(executable);
    command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    command
}

#[cfg(not(windows))]
fn codex_command() -> Command {
    let executable =
        std::env::var_os("TOKENSCOPE_CODEX_CLI_PATH").unwrap_or_else(|| "codex".into());
    Command::new(executable)
}

pub fn fetch_rate_limits() -> Result<Vec<CodexQuota>, &'static str> {
    let mut child = codex_command()
        .args(["app-server", "--listen", "stdio://"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|_| "Codex CLI app-server is unavailable")?;

    let stdout = child
        .stdout
        .take()
        .ok_or("Codex app-server stdout unavailable")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Codex app-server stderr unavailable")?;
    let (stdout_sender, stdout_receiver) = mpsc::channel();
    let stdout_reader = thread::spawn(move || stream_protocol_lines(stdout, stdout_sender));
    // Drain stderr without retaining it. It may contain local diagnostics and is
    // deliberately never written to the Agent log or database.
    let stderr_reader =
        thread::spawn(move || io::copy(&mut stderr.take(RESPONSE_LIMIT), &mut io::sink()));

    let mut input = child
        .stdin
        .take()
        .ok_or("Codex app-server stdin unavailable")?;
    let initialize = json!({
        "method": "initialize",
        "id": 1,
        "params": {
            "clientInfo": {
                "name": "tokenscope-agent",
                "title": "TokenScope",
                "version": env!("CARGO_PKG_VERSION")
            },
            "capabilities": {}
        }
    });
    let initialized = json!({"method":"initialized","params":{}});
    let quota_request = json!({"method":"account/rateLimits/read","id":2,"params":{}});
    let deadline = Instant::now() + PROCESS_TIMEOUT;
    let response = (|| {
        serde_json::to_writer(&mut input, &initialize)
            .map_err(|_| "Could not write to Codex app-server")?;
        input
            .write_all(b"\n")
            .map_err(|_| "Could not write to Codex app-server")?;
        wait_for_response(&stdout_receiver, 1, deadline)?;
        serde_json::to_writer(&mut input, &initialized)
            .map_err(|_| "Could not write to Codex app-server")?;
        input
            .write_all(b"\n")
            .map_err(|_| "Could not write to Codex app-server")?;
        serde_json::to_writer(&mut input, &quota_request)
            .map_err(|_| "Could not write to Codex app-server")?;
        input
            .write_all(b"\n")
            .map_err(|_| "Could not write to Codex app-server")?;
        wait_for_response(&stdout_receiver, 2, deadline)
    })();
    drop(input);
    let _ = child.kill();
    let _ = child.wait();
    let _ = stdout_reader.join();
    let _ = stderr_reader.join();
    let response = response?;
    parse_rate_limits(&response, observed_now_ms()?)
}

pub fn refresh_and_store(store: &mut LocalStore) -> Result<usize, &'static str> {
    let snapshots = fetch_rate_limits()?;
    store
        .upsert_codex_quotas(&snapshots, observed_now_ms()?)
        .map_err(|_| "Could not persist Codex quota snapshot")
}

#[cfg(test)]
mod tests {
    use super::parse_rate_limits;
    use serde_json::json;

    #[test]
    fn parses_available_windows_without_conflating_zero_and_missing() {
        let rows = parse_rate_limits(
            &json!({"rateLimits":{
                "limitId":"codex",
                "primary":{"usedPercent":100,"windowDurationMins":300,"resetsAt":1_800_000_000},
                "secondary":{"usedPercent":null,"windowDurationMins":10080,"resetsAt":null}
            }}),
            123,
        )
        .unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].remaining_basis_points, Some(0));
        assert_eq!(rows[0].reset_at_ms, Some(1_800_000_000_000));
        assert_eq!(rows[1].remaining_basis_points, None);
        assert_eq!(rows[1].reset_at_ms, None);
    }

    #[test]
    fn rejects_non_codex_pool_and_percent_outside_range() {
        assert!(parse_rate_limits(&json!({"rateLimits":{"limitId":"other","primary":{"usedPercent":1,"windowDurationMins":300}}}), 1).is_err());
        assert!(parse_rate_limits(&json!({"rateLimits":{"limitId":"codex","primary":{"usedPercent":101,"windowDurationMins":300}}}), 1).is_err());
    }

    #[test]
    fn parses_codex_window_from_rate_limit_pool_map() {
        let rows = parse_rate_limits(
            &json!({"rateLimitsByLimitId":{"codex":{"primary":{"usedPercent":25,"windowDurationMins":300}}}}),
            10,
        )
        .unwrap();
        assert_eq!(rows[0].remaining_basis_points, Some(7_500));
    }
}
