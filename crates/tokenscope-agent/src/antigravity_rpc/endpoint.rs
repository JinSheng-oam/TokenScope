use super::error::RpcError;
use super::process::{detect_processes, run_capture, ProcessKind};
use std::collections::{BTreeSet, HashSet};
use std::env;
use std::fs;
use std::path::Path;
use std::time::Duration;

const MAX_PORTS: usize = 8;
const MAX_DAEMON_DESCRIPTOR_BYTES: u64 = 16 * 1024;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub(super) port: u16,
    pub(super) csrf: String,
}

fn parse_port(value: Option<&serde_json::Value>) -> Option<u16> {
    value?
        .as_u64()
        .or_else(|| value?.as_str()?.parse::<u64>().ok())
        .and_then(|port| u16::try_from(port).ok())
        .filter(|port| *port > 0)
}

fn valid_csrf(value: Option<&str>) -> Option<String> {
    value
        .filter(|csrf| {
            !csrf.is_empty()
                && csrf.len() <= 512
                && csrf.bytes().all(|byte| byte.is_ascii_graphic())
        })
        .map(str::to_string)
}

pub(super) fn daemon_endpoints_from_dir(directory: &Path, expected_pid: u32) -> Vec<Endpoint> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with("hub_") || !name.ends_with(".json") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        if !metadata.is_file() || metadata.len() > MAX_DAEMON_DESCRIPTOR_BYTES {
            continue;
        }
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(descriptor) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        if descriptor.get("pid").and_then(serde_json::Value::as_u64)
            != Some(u64::from(expected_pid))
        {
            continue;
        }
        let Some(csrf) = valid_csrf(descriptor.get("csrfToken").and_then(|v| v.as_str())) else {
            continue;
        };
        let modified = metadata.modified().unwrap_or(std::time::UNIX_EPOCH);
        let http_port = parse_port(descriptor.get("httpPort"));
        let https_port = parse_port(descriptor.get("httpsPort"));
        if let Some(port) = http_port {
            candidates.push((
                modified,
                Endpoint {
                    port,
                    csrf: csrf.clone(),
                },
            ));
        }
        if let Some(port) = https_port {
            candidates.push((modified, Endpoint { port, csrf }));
        }
    }
    candidates.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.port.cmp(&b.1.port)));
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .map(|(_, endpoint)| endpoint)
        .filter(|endpoint| seen.insert((endpoint.port, endpoint.csrf.clone())))
        .collect()
}

fn daemon_endpoints(pid: u32) -> Vec<Endpoint> {
    let Some(home) = env::var_os("USERPROFILE").or_else(|| env::var_os("HOME")) else {
        return Vec::new();
    };
    daemon_endpoints_from_dir(
        &Path::new(&home)
            .join(".gemini")
            .join("antigravity")
            .join("daemon"),
        pid,
    )
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

pub(super) fn discover_endpoints() -> Result<Vec<Endpoint>, RpcError> {
    let infos = detect_processes()?;
    // `detect_processes` returns at most one process per source kind, so
    // multiple app instances cannot starve a running CLI or IDE server.
    let selected = infos;
    let mut endpoints = Vec::new();
    for info in selected {
        endpoints.extend(daemon_endpoints(info.pid));
        let mut ports = BTreeSet::new();
        if !info.csrf.is_empty() || info.kind == ProcessKind::Cli {
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
        }
        if let Some(port) = info.extension_port.filter(|_| {
            info.extension_csrf
                .as_ref()
                .is_some_and(|csrf| !csrf.is_empty())
                || !info.csrf.is_empty()
        }) {
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
    let mut seen = HashSet::new();
    endpoints.retain(|endpoint| seen.insert((endpoint.port, endpoint.csrf.clone())));
    endpoints.truncate(MAX_PORTS);
    Ok(endpoints)
}
