use super::error::RpcError;
use super::process::{detect_processes, run_capture};
use std::collections::{BTreeSet, HashSet};
use std::time::Duration;

const MAX_PORTS: usize = 8;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct Endpoint {
    pub(super) port: u16,
    pub(super) csrf: String,
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
    let mut seen = HashSet::new();
    endpoints.retain(|endpoint| seen.insert((endpoint.port, endpoint.csrf.clone())));
    endpoints.truncate(MAX_PORTS);
    Ok(endpoints)
}
