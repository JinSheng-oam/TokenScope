use crate::{
    paths::{agent_is_running, database_path},
    state::{DashboardRequest, DashboardWatch},
};
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Emitter, Manager};
use tokenscope_local_ipc::{
    DashboardResponse, SessionAnalysisResponse, SessionDeleteRequest, SessionDeleteResponse,
    AGENT_CONTROL_DIRECTORY,
};
use tokenscope_local_store::{LocalStore, SessionAnalysisOptions};
pub(super) fn dashboard_key(
    range: &str,
    start: Option<&str>,
    end: Option<&str>,
    offset: u64,
) -> String {
    format!(
        "{range}|{}|{}|{offset}",
        start.unwrap_or_default(),
        end.unwrap_or_default()
    )
}

/// Returns a cached projection immediately so range switches feel instant; a
/// stale entry is refreshed in the background and pushed as an event.
pub(super) fn query_agent(
    app: &AppHandle,
    range: &str,
    session_offset: u64,
    start_date: Option<&str>,
    end_date: Option<&str>,
    watch_changes: bool,
) -> Result<DashboardResponse, String> {
    if !["today", "7d", "30d", "all", "custom"].contains(&range) {
        return Err("不支持的统计范围".into());
    }
    if range == "custom" && (start_date.is_none() || end_date.is_none()) {
        return Err("自定义范围需要开始和结束日期".into());
    }
    let watch = app.state::<DashboardWatch>();
    if watch_changes {
        if let Ok(mut request) = watch.request.lock() {
            *request = DashboardRequest {
                range: range.to_owned(),
                start_date: start_date.map(str::to_owned),
                end_date: end_date.map(str::to_owned),
            };
        }
    }
    let key = dashboard_key(range, start_date, end_date, session_offset);
    if session_offset == 0 {
        let hit = watch
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned());
        if let Some((cached, _at)) = hit {
            if chrono::DateTime::from_timestamp_millis(cached.snapshot.generated_at_ms)
                .map(|at| at.with_timezone(&chrono::Local).date_naive())
                != Some(chrono::Local::now().date_naive())
                || cached.snapshot.data_revision < watch.revision.load(Ordering::Acquire)
            {
                spawn_dashboard_refresh(app, range, start_date, end_date);
            }
            return Ok(cached);
        }
    }
    let response = query_agent_fresh(app, range, session_offset, start_date, end_date)?;
    if session_offset == 0 {
        if let Ok(mut cache) = watch.cache.lock() {
            cache.insert(key, (response.clone(), Instant::now()));
        }
    }
    Ok(response)
}

fn spawn_dashboard_refresh(
    app: &AppHandle,
    range: &str,
    start_date: Option<&str>,
    end_date: Option<&str>,
) {
    let app = app.clone();
    let range = range.to_owned();
    let start_date = start_date.map(str::to_owned);
    let end_date = end_date.map(str::to_owned);
    let key = dashboard_key(&range, start_date.as_deref(), end_date.as_deref(), 0);
    let already_running = {
        let watch = app.state::<DashboardWatch>();
        let result = match watch.refreshing.lock() {
            Ok(mut refreshing) => !refreshing.insert(key.clone()),
            Err(_) => true,
        };
        result
    };
    if already_running {
        return;
    }
    thread::spawn(move || {
        let result = query_agent_fresh(&app, &range, 0, start_date.as_deref(), end_date.as_deref());
        let watch = app.state::<DashboardWatch>();
        if let Ok(mut refreshing) = watch.refreshing.lock() {
            refreshing.remove(&key);
        }
        match result {
            Ok(response) => {
                if let Ok(mut cache) = watch.cache.lock() {
                    cache.insert(key, (response.clone(), Instant::now()));
                }
                let _ = app.emit("dashboard-updated", response);
            }
            Err(error) => eprintln!("TokenScope dashboard refresh warning: {error}"),
        }
    });
}

pub(crate) fn query_agent_fresh(
    app: &AppHandle,
    range: &str,
    session_offset: u64,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<DashboardResponse, String> {
    if !["today", "7d", "30d", "all", "custom"].contains(&range) {
        return Err("不支持的统计范围".into());
    }
    if range == "custom" && (start_date.is_none() || end_date.is_none()) {
        return Err("自定义范围需要开始和结束日期".into());
    }
    let db = database_path(app)?;
    if !db.is_file() {
        return Err("本地数据库尚未初始化，请确认 Agent 已成功启动".into());
    }
    let store = LocalStore::open_readonly(&db).map_err(|e| format!("读取本地数据库失败：{e}"))?;
    let snapshot = store
        .dashboard_snapshot_for_range(range, session_offset, start_date, end_date)
        .map_err(|e| format!("查询 Dashboard 数据失败：{e}"))?;
    let mut response = DashboardResponse::new(snapshot);
    response.validate().map_err(|e| e.to_string())?;
    response.request_key = dashboard_key(range, start_date, end_date, session_offset);
    Ok(response)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn query_agent_session_analysis(
    app: &AppHandle,
    range: &str,
    session_offset: u64,
    search: Option<&str>,
    session_limit: Option<u32>,
    tool_id: Option<&str>,
    model_id: Option<&str>,
    project_id: Option<&str>,
    start_date: Option<&str>,
    end_date: Option<&str>,
) -> Result<SessionAnalysisResponse, String> {
    if !["today", "7d", "30d", "all", "custom"].contains(&range) {
        return Err("不支持的统计范围".into());
    }
    if range == "custom" && (start_date.is_none() || end_date.is_none()) {
        return Err("自定义范围需要开始和结束日期".into());
    }
    let session_limit = session_limit.unwrap_or(50);
    if !(1..=200).contains(&session_limit) {
        return Err("每页会话数必须在 1 到 200 之间".into());
    }
    let filter_cache_key = format!(
        "{}|{range}|{}|{}",
        app.state::<DashboardWatch>()
            .revision
            .load(Ordering::Acquire),
        start_date.unwrap_or_default(),
        end_date.unwrap_or_default()
    );
    let cached_filter_options = app
        .state::<DashboardWatch>()
        .session_filter_options
        .lock()
        .ok()
        .and_then(|cache| cache.get(&filter_cache_key).cloned());
    let db = database_path(app)?;
    if !db.is_file() {
        return Err("本地数据库尚未初始化，请确认 Agent 已成功启动".into());
    }
    let store = LocalStore::open_readonly(&db).map_err(|e| format!("读取本地数据库失败：{e}"))?;
    let analysis = store
        .session_analysis_for_range(SessionAnalysisOptions {
            range,
            session_offset,
            session_limit,
            search: search.filter(|value| !value.is_empty()),
            tool_id: tool_id.filter(|value| !value.is_empty()),
            model_id: model_id.filter(|value| !value.is_empty()),
            project_id: project_id.filter(|value| !value.is_empty()),
            start_date,
            end_date,
            include_filter_options: cached_filter_options.is_none(),
        })
        .map_err(|e| format!("查询会话分析失败：{e}"))?;
    let mut response = SessionAnalysisResponse::new(analysis);
    response.validate().map_err(|e| e.to_string())?;
    let watch = app.state::<DashboardWatch>();
    if let Some(options) = cached_filter_options {
        response.analysis.filter_options = options;
    } else if let Ok(mut cache) = watch.session_filter_options.lock() {
        if cache.len() >= 16 {
            cache.clear();
        }
        cache.insert(filter_cache_key, response.analysis.filter_options.clone());
    }
    Ok(response)
}

pub(super) fn request_agent_session_delete(
    app: &AppHandle,
    tool_id: &str,
    session_id: &str,
) -> Result<SessionDeleteResponse, String> {
    if !["codex", "claude", "antigravity", "deepseek-harness", "pi"].contains(&tool_id) {
        return Err("不支持删除此来源的会话".into());
    }
    if session_id.is_empty() || session_id.len() > 1024 || session_id.chars().any(char::is_control)
    {
        return Err("会话标识无效".into());
    }
    let db = database_path(app)?;
    if !agent_is_running(&db)? {
        return Err("本地采集器未运行，请刷新后重试".into());
    }
    let directory = db
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

    static REQUEST_SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    let sequence = REQUEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let request_id = format!("{:x}-{:x}-{:x}", std::process::id(), timestamp, sequence);
    let request = SessionDeleteRequest {
        request_id: request_id.clone(),
        tool_id: tool_id.to_owned(),
        session_id: session_id.to_owned(),
    };
    let body = serde_json::to_vec(&request).map_err(|e| format!("编码会话删除请求失败：{e}"))?;
    let temporary = directory.join(format!("request-{request_id}.tmp"));
    let request_path = directory.join(format!("request-{request_id}.json"));
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
        return Err(format!("提交会话删除请求失败：{error}"));
    }

    let response_path = directory.join(format!("response-{request_id}.json"));
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match fs::read(&response_path) {
            Ok(bytes) => {
                let _ = fs::remove_file(&response_path);
                if bytes.len() > 8192 {
                    return Err("Agent 会话删除响应超出大小限制".into());
                }
                let response: SessionDeleteResponse = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("读取 Agent 会话删除响应失败：{e}"))?;
                if response.request_id != request_id {
                    return Err("Agent 会话删除响应标识不匹配".into());
                }
                if let Some(error) = response.error.as_deref() {
                    return Err(error.to_owned());
                }
                return Ok(response);
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("读取 Agent 会话删除响应失败：{error}")),
        }
        if Instant::now() >= deadline {
            let _ = fs::remove_file(&request_path);
            return Err("Agent 30 秒内未处理会话删除请求，请稍后重试".into());
        }
        thread::sleep(Duration::from_millis(75));
    }
}
