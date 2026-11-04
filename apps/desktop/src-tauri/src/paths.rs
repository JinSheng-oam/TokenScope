use fs2::FileExt;
use std::{
    env,
    ffi::OsString,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Manager};
use tokenscope_sync_client::HubUrl;

pub(crate) fn database_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| format!("无法获取应用本地数据目录：{e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用本地数据目录：{e}"))?;
    let db = dir.join("tokenscope.db");
    migrate_legacy_database(app, &db)?;
    Ok(db)
}

/// One-time move out of Roaming. Roaming profiles are copied between Windows
/// machines; a live SQLite database must not travel with them. The legacy file
/// stays in place if any copy fails, so a failed migration never loses data.
fn migrate_legacy_database(app: &AppHandle, target: &Path) -> Result<(), String> {
    if target.exists() {
        return Ok(());
    }
    let Ok(legacy_dir) = app.path().app_data_dir() else {
        return Ok(());
    };
    if !legacy_dir.join("tokenscope.db").is_file() {
        return Ok(());
    }
    let legacy = legacy_dir.join("tokenscope.db");
    stop_legacy_agent(&legacy)?;
    let target_dir = target.parent().ok_or("无效数据库路径")?;
    let suffixes = ["", "-wal", "-shm", ".device-id"];
    let mut staged = Vec::new();
    for suffix in suffixes {
        let source = legacy_dir.join(format!("tokenscope.db{suffix}"));
        if !source.exists() {
            continue;
        }
        let final_path = target_dir.join(format!("tokenscope.db{suffix}"));
        let mut temporary = final_path.as_os_str().to_os_string();
        temporary.push(".migrating");
        let temporary = PathBuf::from(temporary);
        let _ = fs::remove_file(&temporary);
        if let Err(error) = fs::copy(&source, &temporary) {
            for (temporary, _) in staged {
                let _ = fs::remove_file(temporary);
            }
            let _ = fs::remove_file(&temporary);
            return Err(format!("迁移数据库失败：{error}"));
        }
        staged.push((temporary, final_path));
    }
    if staged.is_empty() {
        return Ok(());
    }
    let mut installed = Vec::new();
    for (temporary, final_path) in staged {
        if let Err(error) = fs::rename(&temporary, &final_path) {
            // Roll back so the next start retries from a clean target.
            for path in &installed {
                let _ = fs::remove_file(path);
            }
            let _ = fs::remove_file(&temporary);
            return Err(format!("安装迁移后的数据库失败：{error}"));
        }
        installed.push(final_path);
    }
    for suffix in suffixes {
        let _ = fs::remove_file(legacy_dir.join(format!("tokenscope.db{suffix}")));
    }
    Ok(())
}

/// A previous build may still own the Roaming database. Copying it while that
/// agent writes would fork the data, so ask it to stop through the existing
/// shutdown marker and wait for the lock to be released.
fn stop_legacy_agent(legacy_db: &Path) -> Result<(), String> {
    if !agent_is_running(legacy_db).unwrap_or(false) {
        return Ok(());
    }
    let mut name = legacy_db.as_os_str().to_os_string();
    name.push(".shutdown");
    let marker = PathBuf::from(name);
    fs::write(&marker, b"").map_err(|e| format!("无法通知旧版采集器停止：{e}"))?;
    for _ in 0..100 {
        thread::sleep(Duration::from_millis(100));
        if !agent_is_running(legacy_db).unwrap_or(true) {
            let _ = fs::remove_file(&marker);
            return Ok(());
        }
    }
    let _ = fs::remove_file(&marker);
    Err("旧版 TokenScope 采集器仍在运行，数据库迁移已中止；请先退出旧版应用后重试".into())
}

pub(crate) fn sync_config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("无法获取应用数据目录：{e}"))?;
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建应用数据目录：{e}"))?;
    Ok(dir.join("sync-hub-url.txt"))
}

pub(crate) fn persist_sync_hub_url(app: &AppHandle, hub_origin: &str) -> Result<(), String> {
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

pub(crate) fn clear_sync_hub_url(app: &AppHandle) -> Result<(), String> {
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

pub(crate) fn resolve_agent(app: &AppHandle) -> Option<PathBuf> {
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

pub(crate) fn lock_path(db: &Path) -> Result<PathBuf, String> {
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

pub(crate) fn shutdown_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(".shutdown");
    PathBuf::from(name)
}

pub(crate) fn agent_is_running(db: &Path) -> Result<bool, String> {
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

pub(crate) fn validate_time_zone(value: &str) -> Result<(), String> {
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

pub(crate) fn rotate_agent_log(
    log_path: &Path,
    max_bytes: u64,
    generations: usize,
) -> io::Result<()> {
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

pub(crate) fn read_pipe_bounded(reader: impl Read, limit: usize) -> io::Result<Vec<u8>> {
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
