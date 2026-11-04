//! Read-only self-diagnostics. The Agent owns SQLite, so health checks live
//! here and are exposed as a one-shot JSON command instead of being duplicated
//! in the desktop shell.
use crate::identity;
use fs2::available_space;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_local_ipc::{
    DiagnosticCheck, DiagnosticFacts, DiagnosticReport, DiagnosticSeverity, DiagnosticToolUsage,
};
use tokenscope_local_store::{schema, LocalStore};

const WAL_WARNING_BYTES: u64 = 256 * 1024 * 1024;
const WAL_ERROR_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DISK_WARNING_BYTES: u64 = 1024 * 1024 * 1024;
const DISK_ERROR_BYTES: u64 = 256 * 1024 * 1024;
const SCAN_INFO_AGE_MS: i64 = 6 * 60 * 60 * 1000;
const SCAN_WARNING_AGE_MS: i64 = 24 * 60 * 60 * 1000;
const LOG_WARNING_BYTES: u64 = 20 * 1024 * 1024;

pub(super) fn diagnose(db: &Path) -> DiagnosticReport {
    let now = now_ms();
    let mut facts = DiagnosticFacts {
        database_path: db.display().to_string(),
        expected_schema_version: schema::VERSION,
        sync_last_status: "never".into(),
        ..DiagnosticFacts::default()
    };
    let mut report = DiagnosticReport::new(now, facts.clone());

    let metadata = match fs::metadata(db) {
        Ok(metadata) if metadata.is_file() => metadata,
        Ok(_) => {
            facts.marker_present = identity::marker_path(db).exists();
            report.push(DiagnosticCheck::new(
                "database",
                "数据库文件",
                DiagnosticSeverity::Error,
                format!(
                    "{} 不是普通文件{}",
                    db.display(),
                    if facts.marker_present {
                        "，但设备身份标记仍存在"
                    } else {
                        ""
                    }
                ),
                Some("检查数据目录是否被目录或链接占用；不要直接新建空库。".into()),
            ));
            return finish(report, facts);
        }
        Err(error) => {
            facts.marker_present = identity::marker_path(db).exists();
            report.push(DiagnosticCheck::new(
                "database",
                "数据库文件",
                DiagnosticSeverity::Error,
                if facts.marker_present {
                    format!("数据库缺失或不可读（{error}），但设备身份标记仍存在")
                } else {
                    format!("无法读取数据库：{error}")
                },
                Some(
                    "确认 TokenScope 数据目录存在且当前用户可读；若文件丢失，请从备份恢复，不要删除设备标记。"
                        .into(),
                ),
            ));
            return finish(report, facts);
        }
    };
    facts.database_bytes = metadata.len();
    report.push(if facts.database_bytes == 0 {
        DiagnosticCheck::new(
            "database",
            "数据库文件",
            DiagnosticSeverity::Error,
            "数据库大小为 0 字节，可能已被中断的写入破坏",
            Some("停止采集器并从备份恢复数据库与 .device-id 标记。".into()),
        )
    } else {
        DiagnosticCheck::new(
            "database",
            "数据库文件",
            DiagnosticSeverity::Ok,
            format!("存在，大小 {}", format_bytes(facts.database_bytes)),
            None,
        )
    });

    let wal = sibling(db, "-wal");
    if let Ok(wal_meta) = fs::metadata(&wal) {
        facts.wal_bytes = wal_meta.len();
        if facts.wal_bytes > 0 {
            let severity = if facts.wal_bytes >= WAL_ERROR_BYTES {
                DiagnosticSeverity::Error
            } else if facts.wal_bytes >= WAL_WARNING_BYTES {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Info
            };
            let action = (severity >= DiagnosticSeverity::Warning).then(|| {
                "等待采集器空闲时会自动 checkpoint；长期不下降时重启采集器并备份数据库。".into()
            });
            report.push(DiagnosticCheck::new(
                "wal",
                "SQLite WAL",
                severity,
                format!("WAL 大小 {}", format_bytes(facts.wal_bytes)),
                action,
            ));
        }
    }

    let marker_path = identity::marker_path(db);
    let marker = match identity::read(&marker_path) {
        Ok(value) => value,
        Err(error) => {
            facts.marker_present = marker_path.exists();
            report.push(DiagnosticCheck::new(
                "identity_marker",
                "设备身份标记",
                DiagnosticSeverity::Error,
                format!("标记文件无法解析：{error}"),
                Some(format!(
                    "检查 {} 是否被截断；确认设备身份后再处理，不要直接删除。",
                    marker_path.display()
                )),
            ));
            None
        }
    };
    facts.marker_present = marker_path.exists();

    let store = match LocalStore::open_readonly(db) {
        Ok(store) => store,
        Err(error) => {
            report.push(DiagnosticCheck::new(
                "open",
                "SQLite 打开",
                DiagnosticSeverity::Error,
                format!("只读打开失败：{error}"),
                Some("数据库可能损坏或被其他进程独占；先备份，再重启采集器。".into()),
            ));
            return finish(report, facts);
        }
    };

    report.push(match store.quick_integrity_check() {
        Ok(()) => DiagnosticCheck::new(
            "integrity",
            "SQLite 完整性",
            DiagnosticSeverity::Ok,
            "quick_check 通过",
            None,
        ),
        Err(error) => DiagnosticCheck::new(
            "integrity",
            "SQLite 完整性",
            DiagnosticSeverity::Error,
            format!("quick_check 失败：{error}"),
            Some("停止采集器并立即备份数据库；不要继续写入或执行 VACUUM。".into()),
        ),
    });

    let schema_version = store.schema_version();
    facts.schema_version = schema_version.as_ref().ok().copied();
    report.push(match schema_version {
        Ok(version) if version == schema::VERSION => DiagnosticCheck::new(
            "schema",
            "Schema 版本",
            DiagnosticSeverity::Ok,
            format!("v{version}，与当前采集器一致"),
            None,
        ),
        Ok(version) => DiagnosticCheck::new(
            "schema",
            "Schema 版本",
            DiagnosticSeverity::Error,
            format!("数据库为 v{version}，当前采集器期望 v{}", schema::VERSION),
            Some("升级或回退应用时保持桌面端与 Agent 同版本；先备份数据库。".into()),
        ),
        Err(error) => DiagnosticCheck::new(
            "schema",
            "Schema 版本",
            DiagnosticSeverity::Error,
            format!("无法读取 Schema 版本：{error}"),
            Some("检查数据库是否完整；必要时从备份恢复。".into()),
        ),
    });

    let identity = match store.identity() {
        Ok(value) => value,
        Err(error) => {
            report.push(DiagnosticCheck::new(
                "identity",
                "设备身份",
                DiagnosticSeverity::Error,
                format!("无法读取数据库设备身份：{error}"),
                Some("检查数据库完整性；不要删除身份标记。".into()),
            ));
            None
        }
    };
    facts.device_id = identity.as_ref().map(|value| value.device_id.clone());
    report.push(match (identity.as_ref(), marker.as_deref()) {
        (Some(stored), Some(marked)) if stored.device_id == marked => DiagnosticCheck::new(
            "identity",
            "设备身份",
            DiagnosticSeverity::Ok,
            format!("数据库与标记一致：{}", stored.device_id),
            None,
        ),
        (Some(stored), Some(marked)) => DiagnosticCheck::new(
            "identity",
            "设备身份",
            DiagnosticSeverity::Error,
            format!("数据库={}，标记={}，身份不一致", stored.device_id, marked),
            Some(
                "数据库可能被替换。先备份两个文件；确认哪一份是本机历史后，再恢复对应数据库或标记。"
                    .into(),
            ),
        ),
        (None, Some(_)) => DiagnosticCheck::new(
            "identity",
            "设备身份",
            DiagnosticSeverity::Error,
            "存在身份标记，但数据库没有设备身份",
            Some("数据库可能为空或被替换；先从备份恢复，不要让它被初始化覆盖。".into()),
        ),
        (Some(stored), None) => DiagnosticCheck::new(
            "identity",
            "设备身份",
            DiagnosticSeverity::Warning,
            format!("数据库身份为 {}，但设备标记缺失", stored.device_id),
            Some("下次正常启动会补写标记；若数据库来源不明，先备份并核对历史。".into()),
        ),
        (None, None) => DiagnosticCheck::new(
            "identity",
            "设备身份",
            DiagnosticSeverity::Warning,
            "数据库与标记都还没有设备身份",
            Some("启动采集器后会生成一次性设备 ID。".into()),
        ),
    });

    match store.usage_overview() {
        Ok(overview) => {
            facts.record_count = overview.record_count;
            facts.total_tokens = overview.total_tokens;
            facts.last_event_at_ms = overview.last_event_at_ms;
            facts.tools = overview
                .tools
                .iter()
                .map(|tool| DiagnosticToolUsage {
                    tool_id: tool.tool_id.clone(),
                    total_tokens: tool.total_tokens,
                    record_count: tool.record_count,
                    last_event_at_ms: tool.last_event_at_ms,
                })
                .collect();
            let tool_detail = overview
                .tools
                .iter()
                .map(|tool| {
                    format!(
                        "{} {} Token / {} 条",
                        tool.tool_id,
                        format_number(tool.total_tokens),
                        format_number(tool.record_count)
                    )
                })
                .collect::<Vec<_>>()
                .join("；");
            let severity = if overview.record_count == 0 || overview.total_tokens == 0 {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Ok
            };
            let action = (severity >= DiagnosticSeverity::Warning).then(|| {
                "运行一次采集并查看 Agent 日志；若来源有数据但记录仍为 0，检查来源路径和解析日志。".into()
            });
            report.push(DiagnosticCheck::new(
                "usage",
                "用量记录",
                severity,
                if tool_detail.is_empty() {
                    "没有有效用量记录".into()
                } else {
                    format!(
                        "{} 条记录，累计 {} Token；{}",
                        format_number(overview.record_count),
                        format_number(overview.total_tokens),
                        tool_detail
                    )
                },
                action,
            ));
        }
        Err(error) => report.push(DiagnosticCheck::new(
            "usage",
            "用量记录",
            DiagnosticSeverity::Error,
            format!("无法汇总用量：{error}"),
            Some("检查数据库完整性和 Schema 版本。".into()),
        )),
    }

    match store.source_checkpoints() {
        Ok(checkpoints) => {
            facts.source_count = checkpoints.len() as u64;
            let mut statuses = BTreeMap::<String, u64>::new();
            let mut newest = None;
            for checkpoint in &checkpoints {
                *statuses.entry(checkpoint.last_status.clone()).or_default() += 1;
                newest = Some(newest.map_or(checkpoint.last_scan_ms, |value: i64| {
                    value.max(checkpoint.last_scan_ms)
                }));
            }
            facts.last_scan_at_ms = newest;
            let partial = statuses.get("partial").copied().unwrap_or(0);
            let degraded = statuses.get("degraded").copied().unwrap_or(0);
            let complete = statuses.get("complete").copied().unwrap_or(0);
            let unknown = statuses.get("unknown").copied().unwrap_or(0);
            let severity = if checkpoints.is_empty() || partial + degraded + unknown > 0 {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Ok
            };
            report.push(DiagnosticCheck::new(
                "checkpoints",
                "采集检查点",
                severity,
                if checkpoints.is_empty() {
                    "尚未完成任何来源采集".into()
                } else {
                    format!(
                        "{} 个来源；complete {complete}，partial {partial}，degraded {degraded}，unknown {unknown}",
                        checkpoints.len()
                    )
                },
                (severity >= DiagnosticSeverity::Warning).then(|| {
                    "查看各来源运行明细和 Agent 日志；partial/degraded 通常会给出具体文件。".into()
                }),
            ));
            let (scan_severity, scan_detail) = match newest {
                None => (
                    DiagnosticSeverity::Warning,
                    "没有可用的扫描时间".to_string(),
                ),
                Some(value) => {
                    let age = now.saturating_sub(value);
                    let severity = if age >= SCAN_WARNING_AGE_MS {
                        DiagnosticSeverity::Warning
                    } else if age >= SCAN_INFO_AGE_MS {
                        DiagnosticSeverity::Info
                    } else {
                        DiagnosticSeverity::Ok
                    };
                    (
                        severity,
                        format!("最近一次扫描 {}", relative_age(now, value)),
                    )
                }
            };
            report.push(DiagnosticCheck::new(
                "checkpoint_freshness",
                "采集新鲜度",
                scan_severity,
                scan_detail,
                (scan_severity >= DiagnosticSeverity::Warning).then(|| {
                    "确认后台采集器正在运行；若刚启动，等待一轮 120 秒扫描后再看。".into()
                }),
            ));
        }
        Err(error) => report.push(DiagnosticCheck::new(
            "checkpoints",
            "采集检查点",
            DiagnosticSeverity::Error,
            format!("无法读取检查点：{error}"),
            Some("检查数据库完整性和 Schema 版本。".into()),
        )),
    }

    match store.sync_outbox_status(now) {
        Ok(sync) => {
            facts.sync_pending = sync.pending;
            facts.sync_deferred = sync.deferred;
            facts.sync_blocked = sync.blocked;
            facts.sync_last_status = sync.last_status.clone();
            let severity = if sync.blocked > 0
                || sync.last_status == "error"
                || sync.last_status == "auth_required"
            {
                DiagnosticSeverity::Warning
            } else if sync.pending > 0 && sync.last_status == "never" {
                DiagnosticSeverity::Info
            } else {
                DiagnosticSeverity::Ok
            };
            let sync_detail = if sync.last_status == "never" {
                format!("同步未配置或尚未启用；本地待同步 {} 条", sync.pending)
            } else {
                format!(
                    "pending {}，deferred {}，blocked {}；最近状态 {}{}",
                    sync.pending,
                    sync.deferred,
                    sync.blocked,
                    sync.last_status,
                    sync.last_error
                        .as_deref()
                        .map(|error| format!("（{error}）"))
                        .unwrap_or_default()
                )
            };
            report.push(DiagnosticCheck::new(
                "sync",
                "同步队列",
                severity,
                sync_detail,
                (severity >= DiagnosticSeverity::Warning).then(|| {
                    "检查 Hub 地址、配对凭据和设备是否被撤销；blocked 记录不会自动重试。".into()
                }),
            ));
        }
        Err(error) => report.push(DiagnosticCheck::new(
            "sync",
            "同步队列",
            DiagnosticSeverity::Warning,
            format!("无法读取同步状态：{error}"),
            Some("若未配置 Hub 可忽略；否则检查数据库 Schema。".into()),
        )),
    }

    if let Some(parent) = db.parent() {
        match available_space(parent) {
            Ok(bytes) => {
                facts.free_disk_bytes = Some(bytes);
                let severity = if bytes < DISK_ERROR_BYTES {
                    DiagnosticSeverity::Error
                } else if bytes < DISK_WARNING_BYTES {
                    DiagnosticSeverity::Warning
                } else {
                    DiagnosticSeverity::Ok
                };
                report.push(DiagnosticCheck::new(
                    "disk",
                    "数据目录磁盘",
                    severity,
                    format!("剩余 {}", format_bytes(bytes)),
                    (severity >= DiagnosticSeverity::Warning).then(|| {
                        "清理磁盘后再继续采集，避免 SQLite 写入失败或 WAL 无法回收。".into()
                    }),
                ));
            }
            Err(error) => report.push(DiagnosticCheck::new(
                "disk",
                "数据目录磁盘",
                DiagnosticSeverity::Warning,
                format!("无法读取磁盘剩余空间：{error}"),
                None,
            )),
        }
    }

    if let Some(parent) = db.parent() {
        let log = parent.join("logs").join("agent.log");
        if let Ok(meta) = fs::metadata(&log) {
            facts.log_bytes = meta.len();
            facts.log_modified_at_ms = meta
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|duration| i64::try_from(duration.as_millis()).ok());
            let severity = if facts.log_bytes >= LOG_WARNING_BYTES {
                DiagnosticSeverity::Warning
            } else {
                DiagnosticSeverity::Info
            };
            report.push(DiagnosticCheck::new(
                "log",
                "Agent 日志",
                severity,
                format!("{}，大小 {}", log.display(), format_bytes(facts.log_bytes)),
                (severity >= DiagnosticSeverity::Warning)
                    .then(|| "日志超过 20 MB，检查轮转是否正常或手动清理旧日志。".into()),
            ));
        } else {
            report.push(DiagnosticCheck::new(
                "log",
                "Agent 日志",
                DiagnosticSeverity::Info,
                "尚未生成日志",
                None,
            ));
        }
    }

    finish(report, facts)
}

fn finish(mut report: DiagnosticReport, facts: DiagnosticFacts) -> DiagnosticReport {
    report.facts = facts;
    report.recompute_overall();
    report
}

fn sibling(db: &Path, suffix: &str) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or_default()
}

fn relative_age(now: i64, then: i64) -> String {
    let age = now.saturating_sub(then);
    if age < 60_000 {
        "刚刚".into()
    } else if age < 3_600_000 {
        format!("{} 分钟前", age / 60_000)
    } else if age < 86_400_000 {
        format!("{} 小时前", age / 3_600_000)
    } else {
        format!("{} 天前", age / 86_400_000)
    }
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

fn format_number(value: u64) -> String {
    let text = value.to_string();
    let mut output = String::with_capacity(text.len() + text.len() / 3);
    for (index, ch) in text.chars().enumerate() {
        if index > 0 && (text.len() - index).is_multiple_of(3) {
            output.push(',');
        }
        output.push(ch);
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_database_is_an_error_and_is_not_created() {
        let dir = std::env::temp_dir().join(format!("tokenscope-diag-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let db = dir.join("tokenscope.db");
        let report = diagnose(&db);
        assert_eq!(report.overall, DiagnosticSeverity::Error);
        assert!(report.checks.iter().any(|check| check.id == "database"));
        assert!(!db.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn severity_ordering_reports_the_worst_check() {
        let mut report = DiagnosticReport::new(1, DiagnosticFacts::default());
        report.push(DiagnosticCheck::new(
            "a",
            "a",
            DiagnosticSeverity::Info,
            "a",
            None,
        ));
        assert_eq!(report.overall, DiagnosticSeverity::Ok);
        report.push(DiagnosticCheck::new(
            "b",
            "b",
            DiagnosticSeverity::Warning,
            "b",
            None,
        ));
        assert_eq!(report.overall, DiagnosticSeverity::Warning);
    }
}
