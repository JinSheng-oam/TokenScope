use crate::{
    paths::{
        agent_is_running, database_path, resolve_agent, rotate_agent_log, shutdown_path,
        sync_config_path, validate_time_zone, webdav_config_path,
    },
    state::{AgentProcess, AgentStatus},
};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};
use tokenscope_local_store::CodexQuota;

pub(crate) fn start_agent_inner(
    app: &AppHandle,
    time_zone: &str,
    process: &AgentProcess,
) -> Result<AgentStatus, String> {
    validate_time_zone(time_zone)?;
    let db = database_path(app)?;
    let data_dir = db.parent().ok_or("无效数据库路径")?;
    let agent = resolve_agent(app);
    let mut child_slot = process.child.lock().map_err(|_| "Agent 进程状态锁损坏")?;

    if let Some(child) = child_slot.as_mut() {
        match child
            .try_wait()
            .map_err(|e| format!("检查 Agent 子进程失败：{e}"))?
        {
            None => {
                return Ok(AgentStatus {
                    running: true,
                    binary_found: agent.is_some(),
                    database_ready: db.is_file(),
                    message: "TokenScope 后台采集器正在运行".into(),
                })
            }
            Some(_) => {
                child_slot.take();
            }
        }
    }

    if agent_is_running(&db)? {
        return Ok(AgentStatus {
            running: true,
            binary_found: agent.is_some(),
            database_ready: db.is_file(),
            message: "检测到由命令行或其他实例启动的 Agent，桌面端不会接管或终止它".into(),
        });
    }
    let Some(agent) = agent else {
        return Ok(AgentStatus {
            running: false,
            binary_found: false,
            database_ready: db.is_file(),
            message: "安装包中未找到 tokenscope-agent，请检查安装文件完整性".into(),
        });
    };

    let stop_file = shutdown_path(&db);
    if stop_file.exists() {
        fs::remove_file(&stop_file).map_err(|e| format!("清理旧 Agent 停止标记失败：{e}"))?;
    }

    let log_dir = data_dir.join("logs");
    fs::create_dir_all(&log_dir).map_err(|e| format!("创建 Agent 日志目录失败：{e}"))?;
    let log_path = log_dir.join("agent.log");
    rotate_agent_log(&log_path, 5 * 1024 * 1024, 3)
        .map_err(|e| format!("轮转 Agent 日志失败：{e}"))?;
    let mut log_options = OpenOptions::new();
    log_options.create(true).append(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        log_options.mode(0o600);
    }
    let log = log_options
        .open(&log_path)
        .map_err(|e| format!("打开 Agent 日志失败：{e}"))?;
    let stderr = log.try_clone().map_err(|e| e.to_string())?;
    let sync_config = sync_config_path(app)?;
    let webdav_config = webdav_config_path(app)?;
    let child = Command::new(&agent)
        .args([
            "--scan-all",
            "--refresh-antigravity-quota",
            "--refresh-codex-quota",
            "--daemon",
            "--sync",
            "--interval-seconds",
            "120",
            "--db",
        ])
        .arg(&db)
        .arg("--sync-config")
        .arg(&sync_config)
        .arg("--sync-webdav-config")
        .arg(&webdav_config)
        .arg("--display-name")
        .arg("This device")
        .arg("--time-zone")
        .arg(time_zone)
        .arg("--shutdown-file")
        .arg(&stop_file)
        .stdin(Stdio::null())
        .stdout(Stdio::from(log))
        .stderr(Stdio::from(stderr))
        .spawn()
        .map_err(|e| format!("启动 Agent 失败：{e}"))?;
    *child_slot = Some(child);

    for _ in 0..40 {
        if agent_is_running(&db)? && db.is_file() {
            return Ok(AgentStatus {
                running: true,
                binary_found: true,
                database_ready: true,
                message: "后台采集器已启动".into(),
            });
        }
        if let Some(child) = child_slot.as_mut() {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| format!("检查 Agent 启动状态失败：{e}"))?
            {
                child_slot.take();
                return Err(format!(
                    "Agent 启动后立即退出，退出状态：{status}；请查看 logs/agent.log"
                ));
            }
        }
        thread::sleep(Duration::from_millis(50));
    }
    Ok(AgentStatus {
        running: false,
        binary_found: true,
        database_ready: db.is_file(),
        message: "Agent 已创建但尚未取得运行锁，请检查应用数据目录 logs/agent.log".into(),
    })
}

#[derive(serde::Serialize)]
pub(crate) struct CodexQuotaRefreshResult {
    snapshots: Vec<CodexQuota>,
    persisted: bool,
    warning: Option<String>,
}

fn refresh_codex_quota_inner(app: &AppHandle) -> Result<CodexQuotaRefreshResult, String> {
    let agent =
        resolve_agent(app).ok_or_else(|| "未找到 tokenscope-agent 可执行文件".to_string())?;
    let mut command = Command::new(agent);
    command
        .arg("--query-codex-quota-json")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let output = command
        .output()
        .map_err(|e| format!("启动 Codex 额度查询失败：{e}"))?;
    if !output.status.success() {
        return Err(format!(
            "Codex 额度查询失败：{}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let snapshots: Vec<CodexQuota> = serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("解析 Codex 额度数据失败：{e}"))?;
    if snapshots.is_empty() {
        return Err("Codex 未返回额度窗口".into());
    }
    Ok(CodexQuotaRefreshResult {
        snapshots,
        persisted: false,
        warning: None,
    })
}

#[tauri::command]
pub(crate) async fn refresh_codex_quota_now(
    app: AppHandle,
) -> Result<CodexQuotaRefreshResult, String> {
    tauri::async_runtime::spawn_blocking(move || refresh_codex_quota_inner(&app))
        .await
        .map_err(|error| format!("Codex 额度刷新任务失败：{error}"))?
}

pub(crate) fn stop_agent_inner(app: &AppHandle, process: &AgentProcess) -> Result<String, String> {
    let db = database_path(app)?;
    let mut child_slot = process.child.lock().map_err(|_| "Agent 进程状态锁损坏")?;
    if child_slot.is_none() {
        return Ok(if agent_is_running(&db)? {
            "检测到外部 Agent，未对其发送停止信号".into()
        } else {
            "Agent 已停止".into()
        });
    }

    let stop_file = shutdown_path(&db);
    let mut marker_options = OpenOptions::new();
    marker_options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        marker_options.mode(0o600);
    }
    let mut marker = marker_options
        .open(&stop_file)
        .map_err(|e| format!("创建 Agent 停止标记失败：{e}"))?;
    marker
        .write_all(b"shutdown\n")
        .map_err(|e| format!("写入 Agent 停止标记失败：{e}"))?;
    drop(marker);

    let deadline = Instant::now() + Duration::from_secs(8);
    loop {
        if let Some(child) = child_slot.as_mut() {
            if let Some(status) = child
                .try_wait()
                .map_err(|e| format!("等待 Agent 退出失败：{e}"))?
            {
                child_slot.take();
                let _ = fs::remove_file(&stop_file);
                return Ok(format!("Agent 已正常退出：{status}"));
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        thread::sleep(Duration::from_millis(50));
    }

    if let Some(mut child) = child_slot.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    let _ = fs::remove_file(&stop_file);
    Ok("Agent 未在 8 秒内响应停止请求，已执行强制终止".into())
}

#[tauri::command]
pub(crate) async fn start_agent(app: AppHandle, time_zone: String) -> Result<AgentStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let process = app.state::<AgentProcess>();
        start_agent_inner(&app, &time_zone, process.inner())
    })
    .await
    .map_err(|e| format!("Agent 启动任务失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn stop_agent(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let process = app.state::<AgentProcess>();
        stop_agent_inner(&app, process.inner())
    })
    .await
    .map_err(|e| format!("Agent 停止任务失败：{e}"))?
}

#[tauri::command]
pub(crate) fn get_agent_status(app: AppHandle) -> Result<AgentStatus, String> {
    let db = database_path(&app)?;
    let process = app.state::<AgentProcess>();
    {
        let mut child_slot = process.child.lock().map_err(|_| "Agent 进程状态锁损坏")?;
        if let Some(child) = child_slot.as_mut() {
            if child
                .try_wait()
                .map_err(|e| format!("检查 Agent 子进程失败：{e}"))?
                .is_some()
            {
                child_slot.take();
            }
        }
    }
    let running = agent_is_running(&db)?;
    let binary_found = resolve_agent(&app).is_some();
    Ok(AgentStatus {
        running,
        binary_found,
        database_ready: db.is_file(),
        message: if running {
            "后台采集器正在运行".into()
        } else if !binary_found {
            "未找到 Agent 可执行文件".into()
        } else {
            "后台采集器未运行".into()
        },
    })
}
