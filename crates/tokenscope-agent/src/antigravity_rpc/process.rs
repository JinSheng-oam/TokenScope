use super::error::RpcError;
use std::io::Read;
use std::process::Command;
use std::time::Duration;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct ProcessInfo {
    pub(super) pid: u32,
    pub(super) kind: ProcessKind,
    pub(super) csrf: String,
    pub(super) hub_port: Option<u16>,
    pub(super) extension_port: Option<u16>,
    pub(super) extension_csrf: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum ProcessKind {
    App,
    Cli,
    Ide,
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

pub(super) fn classify_process(line: &str) -> Option<ProcessInfo> {
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

pub(super) fn run_capture(
    program: &str,
    args: &[String],
    timeout: Duration,
) -> Result<String, RpcError> {
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

pub(super) fn detect_processes() -> Result<Vec<ProcessInfo>, RpcError> {
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
