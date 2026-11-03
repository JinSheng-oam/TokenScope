//! Desktop shell: packaged Agent supervisor, tray lifecycle and read-only stdio queries.
//! The WebView never opens SQLite or reads source logs.
use fs2::FileExt;
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
    thread,
    time::{Duration, Instant, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    webview::WebviewWindowBuilder,
    AppHandle, Emitter, Manager,
};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt as AutostartManagerExt};
use tokenscope_local_ipc::DashboardResponse;
use tokenscope_sync_client::{
    pairing::{DeviceListResponse, PairRequest},
    CredentialKey, HubClient, HubUrl, KeyringCredentialStore, SecretStore,
};
use zeroize::Zeroizing;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AgentStatus {
    running: bool,
    binary_found: bool,
    database_ready: bool,
    message: String,
}

/// Owns only the Agent process started by this desktop instance.
/// An independently launched CLI Agent is never killed by the desktop.
#[derive(Default)]
struct AgentProcess {
    child: Mutex<Option<Child>>,
}

#[derive(Default)]
struct AppLifecycle {
    quit_requested: AtomicBool,
    keep_alive_after_window_close: AtomicBool,
}

/// Remembers which range the UI is showing so background refreshes never
/// replace data from a different selection.
#[derive(Clone, Default)]
struct DashboardRequest {
    range: String,
    start_date: Option<String>,
    end_date: Option<String>,
}

#[derive(Default)]
struct DashboardWatch {
    request: Mutex<DashboardRequest>,
    cache: Mutex<HashMap<String, (DashboardResponse, Instant)>>,
    refreshing: Mutex<HashSet<String>>,
}

fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录：{e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
    Ok(dir.join("tokenscope.db"))
}

fn sync_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录：{e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
    Ok(dir.join("sync-hub-url.txt"))
}

fn persist_sync_hub_url(app: &AppHandle, hub_origin: &str) -> Result<(), String> {
    let hub = HubUrl::parse(hub_origin).map_err(|e| e.to_string())?;
    let path = sync_config_path(app)?;
    let mut temp = path.clone();
    temp.set_extension(format!("tmp-{}", std::process::id()));
    match fs::remove_file(&temp) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("清理旧同步配置临时文件失败：{error}")),
    }
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .map_err(|e| format!("创建同步配置失败：{e}"))?;
    file.write_all(hub.origin().as_bytes())
        .map_err(|e| format!("写入同步配置失败：{e}"))?;
    file.sync_all()
        .map_err(|e| format!("同步配置落盘失败：{e}"))?;
    // Windows rename does not replace an existing destination. Removing the
    // previous non-secret URL may briefly pause sync; the Agent treats a missing
    // or partially-written config as "not configured" and retries next cycle.
    match fs::remove_file(&path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("清理旧同步配置失败：{error}")),
    }
    fs::rename(&temp, &path).map_err(|e| format!("原子替换同步配置失败：{e}"))?;
    Ok(())
}

fn clear_sync_hub_url(app: &AppHandle) -> Result<(), String> {
    let path = sync_config_path(app)?;
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("删除同步配置失败：{error}")),
    }
}

fn agent_file_name() -> &'static str {
    if cfg!(windows) {
        "tokenscope-agent.exe"
    } else {
        "tokenscope-agent"
    }
}

fn resolve_agent(app: &AppHandle) -> Option<PathBuf> {
    let name = agent_file_name();
    let mut candidates = Vec::new();
    if cfg!(debug_assertions) {
        if let Some(path) = env::var_os("TOKENSCOPE_AGENT_PATH") {
            candidates.push(PathBuf::from(path));
        }
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            candidates.push(parent.join(name));
            candidates.push(parent.join("binaries").join(name));
        }
    }
    // Tauri bundle resources preserve `resources/` relative paths.
    if let Ok(resources) = app.path().resource_dir() {
        candidates.push(resources.join("resources").join(name));
        candidates.push(resources.join(name));
        candidates.push(resources.join("binaries").join(name));
    }
    // Workspace binaries are used only for development builds.
    if cfg!(debug_assertions) {
        let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Ok(root) = manifest.join("../../..").canonicalize() {
            candidates.push(root.join("target/debug").join(name));
        }
        candidates.push(manifest.join("target/debug").join(name));
    }
    candidates.into_iter().find_map(|candidate| {
        if !candidate.is_file() {
            return None;
        }
        candidate.canonicalize().ok()
    })
}

fn lock_path(db: &Path) -> Result<PathBuf, String> {
    let absolute = if db.exists() {
        db.canonicalize()
            .map_err(|e| format!("规范化数据库路径失败：{e}"))?
    } else {
        let parent = db
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = db.file_name().ok_or("数据库路径缺少文件名")?;
        parent
            .canonicalize()
            .map_err(|e| format!("规范化数据库目录失败：{e}"))?
            .join(name)
    };
    let mut name: OsString = absolute.as_os_str().to_os_string();
    name.push(".lock");
    Ok(PathBuf::from(name))
}

fn shutdown_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(".shutdown");
    PathBuf::from(name)
}

fn agent_is_running(db: &Path) -> Result<bool, String> {
    let path = lock_path(db)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("无法检查 Agent 锁：{e}"))?;
    match file.try_lock_exclusive() {
        Ok(()) => {
            let _ = FileExt::unlock(&file);
            Ok(false)
        }
        Err(error)
            if error.kind() == io::ErrorKind::WouldBlock
                || (cfg!(windows) && error.raw_os_error() == Some(33)) =>
        {
            Ok(true)
        }
        Err(error) => Err(format!("检查 Agent 锁失败：{error}")),
    }
}

fn validate_time_zone(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/_+-".contains(&b))
    {
        return Err("时区必须是有效的 IANA 时区名称".into());
    }
    Ok(())
}

fn log_generation_path(log_path: &Path, generation: usize) -> PathBuf {
    let mut name = log_path.as_os_str().to_os_string();
    name.push(format!(".{generation}"));
    PathBuf::from(name)
}

fn rotate_agent_log(log_path: &Path, max_bytes: u64, generations: usize) -> io::Result<()> {
    if fs::metadata(log_path)
        .map(|m| m.len() <= max_bytes)
        .unwrap_or(true)
    {
        return Ok(());
    }
    for generation in (1..=generations).rev() {
        let current = if generation == 1 {
            log_path.to_path_buf()
        } else {
            log_generation_path(log_path, generation - 1)
        };
        let next = log_generation_path(log_path, generation);
        if current.exists() {
            if next.exists() {
                fs::remove_file(&next)?;
            }
            fs::rename(current, next)?;
        }
    }
    Ok(())
}

fn start_agent_inner(
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

fn stop_agent_inner(app: &AppHandle, process: &AgentProcess) -> Result<String, String> {
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

fn read_pipe_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take((limit + 1) as u64).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Agent response exceeded size limit",
        ));
    }
    Ok(bytes)
}

fn dashboard_key(range: &str, start: Option<&str>, end: Option<&str>, offset: u64) -> String {
    format!(
        "{range}|{}|{}|{offset}",
        start.unwrap_or_default(),
        end.unwrap_or_default()
    )
}

/// Returns a cached projection immediately so range switches feel instant; a
/// stale entry is refreshed in the background and pushed as an event.
fn query_agent(
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
    let watch = app.state::<DashboardWatch>();
    if let Ok(mut request) = watch.request.lock() {
        *request = DashboardRequest {
            range: range.to_owned(),
            start_date: start_date.map(str::to_owned),
            end_date: end_date.map(str::to_owned),
        };
    }
    let key = dashboard_key(range, start_date, end_date, session_offset);
    if session_offset == 0 {
        let hit = watch
            .cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(&key).cloned());
        if let Some((cached, at)) = hit {
            if at.elapsed() > Duration::from_secs(10) {
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
        let result =
            query_agent_fresh(&app, &range, 0, start_date.as_deref(), end_date.as_deref());
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

fn query_agent_fresh(
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
    let agent =
        resolve_agent(app).ok_or_else(|| "未找到 tokenscope-agent 可执行文件".to_string())?;
    let mut command = Command::new(agent);
    command
        .arg("--dashboard-json")
        .arg("--range")
        .arg(range)
        .arg("--session-offset")
        .arg(session_offset.to_string())
        .arg("--db")
        .arg(&db);
    if let Some(value) = start_date {
        command.arg("--start-date").arg(value);
    }
    if let Some(value) = end_date {
        command.arg("--end-date").arg(value);
    }
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("启动 Dashboard 查询失败：{e}"))?;

    let stdout_pipe = child.stdout.take().ok_or("Agent stdout pipe unavailable")?;
    let stderr_pipe = child.stderr.take().ok_or("Agent stderr pipe unavailable")?;
    let stdout_reader = thread::spawn(move || read_pipe_bounded(stdout_pipe, 1_048_576));
    let stderr_reader = thread::spawn(move || read_pipe_bounded(stderr_pipe, 65_536));
    // A cold `全部` aggregate over a large local history can exceed five
    // seconds on slower disks; background pushes keep the UI responsive while
    // this bounded wait prevents an orphaned child process.
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| format!("等待 Dashboard 查询失败：{e}"))?
        {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err("Dashboard 查询超时（20 秒）".into());
        }
        thread::sleep(Duration::from_millis(10));
    };
    let stdout = stdout_reader
        .join()
        .map_err(|_| "Agent stdout reader panicked")?
        .map_err(|e| format!("读取 Agent stdout 失败：{e}"))?;
    let stderr = stderr_reader
        .join()
        .map_err(|_| "Agent stderr reader panicked")?
        .map_err(|e| format!("读取 Agent stderr 失败：{e}"))?;
    if !status.success() {
        return Err(format!(
            "Agent 查询失败：{}",
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    let response: DashboardResponse =
        serde_json::from_slice(&stdout).map_err(|e| format!("Agent 返回的数据格式无效：{e}"))?;
    response.validate().map_err(|e| e.to_string())?;
    Ok(response)
}

fn dashboard_signature(db: &Path) -> Option<(u64, u64)> {
    let wal = PathBuf::from(format!("{}-wal", db.display()));
    let meta = fs::metadata(&wal).or_else(|_| fs::metadata(db)).ok()?;
    let modified = meta
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as u64;
    Some((meta.len(), modified))
}

/// One long-lived reader: after the first snapshot, watch the Agent's WAL and
/// push a fresh projection only when new usage actually landed. This replaces
/// the previous timer that spawned a full query every 30 seconds.
fn spawn_dashboard_watch(app: AppHandle) {
    thread::spawn(move || {
        let mut last = None;
        loop {
            thread::sleep(Duration::from_millis(1_200));
            let Ok(db) = database_path(&app) else { continue };
            if !db.is_file() {
                continue;
            }
            let signature = dashboard_signature(&db);
            if signature.is_none() || signature == last {
                continue;
            }
            last = signature;
            // Scans commit in bursts; let the burst settle before reading.
            thread::sleep(Duration::from_millis(800));
            let request = app
                .state::<DashboardWatch>()
                .request
                .lock()
                .map(|value| value.clone())
                .unwrap_or_default();
            if request.range.is_empty() {
                continue;
            }
            match query_agent_fresh(
                &app,
                &request.range,
                0,
                request.start_date.as_deref(),
                request.end_date.as_deref(),
            ) {
                Ok(response) => {
                    let key = dashboard_key(
                        &request.range,
                        request.start_date.as_deref(),
                        request.end_date.as_deref(),
                        0,
                    );
                    if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
                        cache.insert(key, (response.clone(), Instant::now()));
                    }
                    if let Err(error) = app.emit("dashboard-updated", response) {
                        eprintln!("TokenScope dashboard push warning: {error}");
                    }
                }
                Err(error) => eprintln!("TokenScope dashboard push warning: {error}"),
            }
        }
    });
}

/// Warms the standard ranges once in the background so the first manual switch
/// hits the cache instead of paying for another cold aggregate.
fn spawn_dashboard_warmup(app: AppHandle) {
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(6));
        for range in ["today", "7d", "30d", "all"] {
            let key = dashboard_key(range, None, None, 0);
            let cached = app
                .state::<DashboardWatch>()
                .cache
                .lock()
                .map(|cache| cache.contains_key(&key))
                .unwrap_or(true);
            if cached {
                continue;
            }
            if let Ok(response) = query_agent_fresh(&app, range, 0, None, None) {
                if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
                    cache.insert(key, (response, Instant::now()));
                }
            }
        }
    });
}

#[tauri::command]
async fn start_agent(app: AppHandle, time_zone: String) -> Result<AgentStatus, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let process = app.state::<AgentProcess>();
        start_agent_inner(&app, &time_zone, process.inner())
    })
    .await
    .map_err(|e| format!("Agent 启动任务失败：{e}"))?
}

#[tauri::command]
async fn stop_agent(app: AppHandle) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let process = app.state::<AgentProcess>();
        stop_agent_inner(&app, process.inner())
    })
    .await
    .map_err(|e| format!("Agent 停止任务失败：{e}"))?
}

#[tauri::command]
fn get_agent_status(app: AppHandle) -> Result<AgentStatus, String> {
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

#[tauri::command]
async fn get_dashboard_snapshot(
    app: AppHandle,
    range: String,
    session_offset: u64,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<DashboardResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        query_agent(
            &app,
            &range,
            session_offset,
            start_date.as_deref(),
            end_date.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Dashboard 查询任务失败：{e}"))?
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PairHubResult {
    hub_origin: String,
    device_id: String,
    created_at_ms: i64,
}

fn sync_context(
    app: &AppHandle,
    hub_url: &str,
) -> Result<(HubUrl, CredentialKey, String, String), String> {
    let hub = HubUrl::parse(hub_url).map_err(|e| e.to_string())?;
    let dashboard = query_agent_fresh(app, "all", 0, None, None)?;
    let identity = dashboard
        .snapshot
        .identity
        .ok_or_else(|| "本机设备身份尚未初始化，请先启动 Agent 并完成一次采集".to_string())?;
    let key = CredentialKey::for_device(&hub, &identity.device_id).map_err(|e| e.to_string())?;
    Ok((hub, key, identity.device_id, dashboard.snapshot.time_zone))
}

fn platform_os_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        "other"
    }
}

#[tauri::command]
async fn pair_hub_device(
    app: AppHandle,
    hub_url: String,
    pairing_code: String,
    display_name: String,
) -> Result<PairHubResult, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut pairing_code = Zeroizing::new(pairing_code);
        let (hub, key, device_id, time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        // Probe the native secure store before consuming the one-time pairing code.
        store.preflight().map_err(|e| e.to_string())?;
        let request = PairRequest {
            pairing_code: std::mem::take(&mut *pairing_code),
            device_id: device_id.clone(),
            display_name: display_name.trim().to_string(),
            os_name: platform_os_name().to_string(),
            app_version: env!("CARGO_PKG_VERSION").to_string(),
            time_zone,
        };
        request.validate().map_err(|field| format!("配对信息无效：{field}"))?;
        let client = HubClient::new(hub.clone()).map_err(|e| e.to_string())?;
        let response = client.pair(&request).map_err(|e| e.to_string())?;
        if let Err(error) = store.save(&key, &response.device_token) {
            // The Hub has already consumed the code and registered the device.
            // Never expose the one-time token in UI/logs as a fallback.
            return Err(format!("Hub 已注册设备，但操作系统安全凭证库保存失败（{error}）。请在 Hub 管理端撤销该设备，再生成新配对码重新配对。"));
        }
        if let Err(error) = persist_sync_hub_url(&app, hub.origin()) {
            let _ = store.delete(&key);
            return Err(format!("设备已在 Hub 注册，但本机同步配置保存失败（{error}）。请在 Hub 管理端撤销该设备后重新配对。"));
        }
        Ok(PairHubResult {
            hub_origin: hub.origin().to_string(),
            device_id: response.device_id,
            created_at_ms: response.created_at_ms,
        })
    }).await.map_err(|e| format!("设备配对任务失败：{e}"))?
}

#[tauri::command]
async fn list_hub_devices(app: AppHandle, hub_url: String) -> Result<DeviceListResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, _device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        let token = store
            .load(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "本机尚未与该 Hub 配对".to_string())?;
        HubClient::new(hub)
            .and_then(|client| client.list_devices(&token))
            .map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| format!("读取 Hub 设备列表失败：{e}"))?
}

#[tauri::command]
async fn rename_hub_device(
    app: AppHandle,
    hub_url: String,
    display_name: String,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        let token = store
            .load(&key)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "本机尚未与该 Hub 配对".to_string())?;
        let response = HubClient::new(hub)
            .and_then(|client| client.rename_self(&token, &display_name))
            .map_err(|e| e.to_string())?;
        if response.device_id != device_id {
            return Err("Hub 返回的设备身份与本机不一致".into());
        }
        Ok(response.display_name)
    })
    .await
    .map_err(|e| format!("修改 Hub 设备名称失败：{e}"))?
}

#[tauri::command]
async fn rotate_hub_device_token(app: AppHandle, hub_url: String) -> Result<i64, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let store = KeyringCredentialStore;
        let old_token = store.load(&key).map_err(|e| e.to_string())?
            .ok_or_else(|| "本机尚未与该 Hub 配对".to_string())?;
        let rotated = HubClient::new(hub).and_then(|client| client.rotate_self_token(&old_token, &device_id)).map_err(|e| e.to_string())?;
        store.save(&key, &rotated.device_token).map_err(|error| format!(
            "Hub 已轮换凭证，但新凭证无法写入系统安全存储（{error}）。旧凭证已失效，请在 Hub 管理端撤销设备并重新配对。"
        ))?;
        Ok(rotated.created_at_ms)
    }).await.map_err(|e| format!("轮换 Hub 设备凭证失败：{e}"))?
}

#[tauri::command]
async fn disconnect_hub_device(app: AppHandle, hub_url: String) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (hub, key, _device_id, _time_zone) = sync_context(&app, &hub_url)?;
        let _ = hub;
        let deleted = KeyringCredentialStore
            .delete(&key)
            .map_err(|e| e.to_string())?;
        clear_sync_hub_url(&app)?;
        Ok(deleted)
    })
    .await
    .map_err(|e| format!("删除本机 Hub 凭证失败：{e}"))?
}

#[tauri::command]
fn get_autostart_enabled(app: AppHandle) -> Result<bool, String> {
    app.autolaunch()
        .is_enabled()
        .map_err(|e| format!("读取开机启动状态失败：{e}"))
}

#[tauri::command]
async fn set_autostart_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let manager = app.autolaunch();
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        };
        result.map_err(|e| format!("修改开机启动设置失败：{e}"))?;
        manager
            .is_enabled()
            .map_err(|e| format!("确认开机启动状态失败：{e}"))
    })
    .await
    .map_err(|e| format!("开机启动设置任务失败：{e}"))?
}

fn show_or_create_main_window(app: &AppHandle) -> Result<(), String> {
    let window = if let Some(window) = app.get_webview_window("main") {
        window
    } else {
        let config = app
            .config()
            .app
            .windows
            .iter()
            .find(|window| window.label == "main")
            .ok_or_else(|| "Tauri 配置中缺少 main 窗口".to_string())?;
        WebviewWindowBuilder::from_config(app, config)
            .and_then(|builder| builder.build())
            .map_err(|e| format!("创建 TokenScope 窗口失败：{e}"))?
    };
    let _ = window.center();
    window
        .show()
        .map_err(|e| format!("显示 TokenScope 窗口失败：{e}"))?;
    window
        .unminimize()
        .map_err(|e| format!("还原 TokenScope 窗口失败：{e}"))?;
    let _ = window.set_always_on_top(true);
    let _ = window.set_focus();
    let _ = window.set_always_on_top(false);
    Ok(())
}

fn build_tray(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let open = MenuItem::with_id(app, "open", "打开 TokenScope", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 TokenScope", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &separator, &quit])?;
    let icon = app
        .default_window_icon()
        .cloned()
        .ok_or("missing default tray icon")?;
    TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            if let tauri::tray::TrayIconEvent::Click {
                button: tauri::tray::MouseButton::Left,
                button_state: tauri::tray::MouseButtonState::Up,
                ..
            } = event
            {
                let handle = tray.app_handle().clone();
                let _ = show_or_create_main_window(&handle);
            }
        })
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => {
                let handle = app.clone();
                let _ = show_or_create_main_window(&handle);
            }
            "quit" => {
                app.state::<AppLifecycle>()
                    .quit_requested
                    .store(true, Ordering::SeqCst);
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .manage(AgentProcess::default())
        .manage(AppLifecycle::default())
        .manage(DashboardWatch::default())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .setup(|app| {
            build_tray(app)?;
            let process = app.state::<AgentProcess>();
            let time_zone = iana_time_zone::get_timezone().unwrap_or_else(|_| "UTC".to_string());
            if let Err(error) = start_agent_inner(app.handle(), &time_zone, process.inner()) {
                eprintln!("TokenScope Agent startup warning: {error}");
            }
            spawn_dashboard_watch(app.handle().clone());
            spawn_dashboard_warmup(app.handle().clone());
            // Login autostart runs the collector and tray only. No WebView is
            // created until the user chooses Open, keeping idle memory lower.
            let is_bg = env::args().any(|arg| arg == "--background");
            if !is_bg {
                if let Err(error) = show_or_create_main_window(app.handle()) {
                    eprintln!("TokenScope window startup warning: {error}");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // Destroy the WebView instead of merely hiding it. The tray
                // process and Agent remain alive; opening the app recreates it.
                window
                    .app_handle()
                    .state::<AppLifecycle>()
                    .keep_alive_after_window_close
                    .store(true, Ordering::SeqCst);
                api.prevent_close();
                let app_handle = window.app_handle().clone();
                let _ = window.destroy();
                // Clear a stale keep-alive marker if this platform does not
                // emit ExitRequested when the final WebView is destroyed.
                thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    app_handle
                        .state::<AppLifecycle>()
                        .keep_alive_after_window_close
                        .store(false, Ordering::SeqCst);
                });
            }
        })
        .invoke_handler(tauri::generate_handler![
            start_agent,
            stop_agent,
            get_agent_status,
            get_dashboard_snapshot,
            get_autostart_enabled,
            set_autostart_enabled,
            pair_hub_device,
            list_hub_devices,
            rename_hub_device,
            rotate_hub_device_token,
            disconnect_hub_device
        ])
        .build(tauri::generate_context!())
        .expect("TokenScope desktop initialization failed");

    app.run(|app_handle, event| match event {
        tauri::RunEvent::ExitRequested { api, .. } => {
            // Only suppress the exit request associated with our own window
            // destruction. OS shutdown and unrelated exit requests are allowed.
            let lifecycle = app_handle.state::<AppLifecycle>();
            if !lifecycle.quit_requested.load(Ordering::SeqCst)
                && lifecycle
                    .keep_alive_after_window_close
                    .swap(false, Ordering::SeqCst)
            {
                api.prevent_exit();
            }
        }
        tauri::RunEvent::Exit => {
            let process = app_handle.state::<AgentProcess>();
            let _ = stop_agent_inner(app_handle, process.inner());
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen {
            has_visible_windows: false,
        } => {
            let handle = app_handle.clone();
            thread::spawn(move || {
                if let Err(error) = show_or_create_main_window(&handle) {
                    eprintln!("TokenScope reopen warning: {error}");
                }
            });
        }
        _ => {}
    });
}

#[cfg(test)]
mod tests {
    use super::{rotate_agent_log, shutdown_path, validate_time_zone};
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_dir() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("tokenscope-desktop-{n}"));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn shutdown_marker_is_a_sibling_of_the_database() {
        let path = PathBuf::from("/tmp/tokenscope.db");
        assert_eq!(
            shutdown_path(&path),
            PathBuf::from("/tmp/tokenscope.db.shutdown")
        );
    }

    #[test]
    fn log_rotation_keeps_bounded_generations() {
        let dir = temp_dir();
        let log = dir.join("agent.log");
        fs::write(&log, b"123456789").unwrap();
        rotate_agent_log(&log, 4, 3).unwrap();
        assert!(!log.exists());
        assert_eq!(fs::read(dir.join("agent.log.1")).unwrap(), b"123456789");
        fs::write(&log, b"abcdefgh").unwrap();
        rotate_agent_log(&log, 4, 3).unwrap();
        assert_eq!(fs::read(dir.join("agent.log.1")).unwrap(), b"abcdefgh");
        assert_eq!(fs::read(dir.join("agent.log.2")).unwrap(), b"123456789");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn timezone_input_is_bounded_and_rejects_path_characters() {
        assert!(validate_time_zone("Asia/Shanghai").is_ok());
        assert!(validate_time_zone("../../etc/passwd").is_err());
        assert!(validate_time_zone(&"a".repeat(129)).is_err());
    }
}
