//! Desktop-side orchestration for the read-only Agent diagnostic command.
use crate::paths::{agent_is_running, database_path, read_pipe_bounded, resolve_agent};
use std::{
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};
use tokenscope_local_ipc::{
    DiagnosticCheck, DiagnosticFacts, DiagnosticReport, DiagnosticSeverity,
};

const SCAN_STALE_WHILE_RUNNING_MS: i64 = 2 * 60 * 60 * 1000;

#[tauri::command]
pub(crate) async fn run_diagnostics(app: AppHandle) -> Result<DiagnosticReport, String> {
    tauri::async_runtime::spawn_blocking(move || collect(&app))
        .await
        .map_err(|error| format!("诊断任务失败：{error}"))?
}

fn collect(app: &AppHandle) -> Result<DiagnosticReport, String> {
    let now = now_ms();
    let db = match database_path(app) {
        Ok(path) => path,
        Err(error) => {
            let mut report = DiagnosticReport::new(now, DiagnosticFacts::default());
            report.push(DiagnosticCheck::new(
                "data_directory",
                "应用数据目录",
                DiagnosticSeverity::Error,
                format!("无法解析或创建应用数据目录：{error}"),
                Some("检查用户目录权限和磁盘状态；不要直接删除数据目录。".into()),
            ));
            return Ok(report);
        }
    };
    let agent = resolve_agent(app);
    let running = match agent_is_running(&db) {
        Ok(value) => value,
        Err(error) => {
            let mut report = DiagnosticReport::new(now, DiagnosticFacts::default());
            report.push(DiagnosticCheck::new(
                "agent_lock",
                "Agent 运行锁",
                DiagnosticSeverity::Warning,
                format!("无法检查采集器运行锁：{error}"),
                Some("确认数据目录可读写，然后重新运行诊断。".into()),
            ));
            report.facts.database_path = db.display().to_string();
            return Ok(report);
        }
    };

    let mut report = match agent.as_deref() {
        Some(path) => match query_agent(path, &db) {
            Ok(report) => report,
            Err(error) => {
                let facts = DiagnosticFacts {
                    database_path: db.display().to_string(),
                    ..DiagnosticFacts::default()
                };
                let mut report = DiagnosticReport::new(now, facts);
                report.push(DiagnosticCheck::new(
                    "agent_query",
                    "诊断进程",
                    DiagnosticSeverity::Error,
                    error,
                    Some("查看应用数据目录 logs/agent.log，并确认 Agent 二进制可执行。".into()),
                ));
                report
            }
        },
        None => {
            let facts = DiagnosticFacts {
                database_path: db.display().to_string(),
                ..DiagnosticFacts::default()
            };
            let mut report = DiagnosticReport::new(now, facts);
            report.push(DiagnosticCheck::new(
                "agent_binary",
                "Agent 二进制",
                DiagnosticSeverity::Error,
                "未找到 tokenscope-agent",
                Some("重新安装应用或检查 resources/tokenscope-agent 是否完整。".into()),
            ));
            report
        }
    };

    report.facts.agent_running = running;
    report.facts.agent_path = agent.as_ref().map(|path| path.display().to_string());
    if let Some(path) = agent.as_ref() {
        report.push(DiagnosticCheck::new(
            "agent_binary",
            "Agent 二进制",
            DiagnosticSeverity::Ok,
            path.display().to_string(),
            None,
        ));
    }
    report.push(if running {
        DiagnosticCheck::new(
            "agent_process",
            "Agent 进程",
            DiagnosticSeverity::Ok,
            "后台采集器正在持有数据库锁",
            None,
        )
    } else {
        DiagnosticCheck::new(
            "agent_process",
            "Agent 进程",
            DiagnosticSeverity::Warning,
            "后台采集器未运行；统计只会在手动刷新时更新",
            Some("重新启动 TokenScope 应用以拉起采集器；若仍不启动，查看 logs/agent.log。".into()),
        )
    });

    if running {
        match report.facts.last_scan_at_ms {
            Some(last_scan) if now.saturating_sub(last_scan) > SCAN_STALE_WHILE_RUNNING_MS => {
                report.push(DiagnosticCheck::new(
                    "scan_stale",
                    "采集活跃度",
                    DiagnosticSeverity::Warning,
                    format!(
                        "采集器在运行，但最近一次扫描已是 {} 小时前",
                        now.saturating_sub(last_scan) / 3_600_000
                    ),
                    Some("检查 Agent 日志中是否持续报错，或重启采集器。".into()),
                ));
            }
            None => report.push(DiagnosticCheck::new(
                "scan_stale",
                "采集活跃度",
                DiagnosticSeverity::Warning,
                "采集器在运行，但还没有任何来源检查点",
                Some("等待一轮扫描；若持续为空，检查来源目录是否存在。".into()),
            )),
            _ => {}
        }
    }

    if let Ok(legacy_dir) = app.path().app_data_dir() {
        let legacy = legacy_dir.join("tokenscope.db");
        if legacy.is_file() && legacy != db {
            report.facts.legacy_database_path = Some(legacy.display().to_string());
            let size = std::fs::metadata(&legacy)
                .map(|meta| meta.len())
                .unwrap_or(0);
            report.push(DiagnosticCheck::new(
                "legacy_database",
                "旧版 Roaming 数据库",
                DiagnosticSeverity::Warning,
                format!(
                    "发现旧路径数据库（{}），当前库已迁到 LocalAppData",
                    format_bytes(size)
                ),
                Some("确认当前统计无误后备份再移除旧文件；不要让旧构建继续写入它。".into()),
            ));
        }
    }

    report.recompute_overall();
    Ok(report)
}

fn query_agent(agent: &Path, db: &Path) -> Result<DiagnosticReport, String> {
    let mut child = Command::new(agent)
        .arg("--diagnose-json")
        .arg("--db")
        .arg(db)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("启动诊断进程失败：{error}"))?;
    let stdout = child.stdout.take().ok_or("Agent stdout 管道不可用")?;
    let stderr = child.stderr.take().ok_or("Agent stderr 管道不可用")?;
    let stdout_reader = thread::spawn(move || read_pipe_bounded(stdout, 4 * 1024 * 1024));
    let stderr_reader = thread::spawn(move || read_pipe_bounded(stderr, 256 * 1024));
    let deadline = Instant::now() + Duration::from_secs(30);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|error| format!("等待诊断进程失败：{error}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("诊断进程超时（30 秒）".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Agent stdout 读取线程异常")?
        .map_err(|error| format!("读取诊断输出失败：{error}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Agent stderr 读取线程异常")?
        .map_err(|error| format!("读取诊断错误输出失败：{error}"))?;
    if !status.success() {
        let detail = String::from_utf8_lossy(&stderr);
        let detail = detail.trim();
        return Err(format!(
            "诊断进程退出状态 {status}{}",
            if detail.is_empty() {
                String::new()
            } else {
                format!("：{detail}")
            }
        ));
    }
    serde_json::from_slice::<DiagnosticReport>(&stdout)
        .map_err(|error| format!("解析诊断结果失败：{error}"))
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn format_bytes(value: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if value >= GB {
        format!("{:.1} GB", value as f64 / GB as f64)
    } else if value >= MB {
        format!("{:.1} MB", value as f64 / MB as f64)
    } else if value >= KB {
        format!("{:.1} KB", value as f64 / KB as f64)
    } else {
        format!("{value} B")
    }
}
