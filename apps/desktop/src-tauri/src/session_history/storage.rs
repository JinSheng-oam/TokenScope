use super::{codec, io, locate, MAX_FILE};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static LAST_SAVE: Mutex<Option<(PathBuf, String)>> = Mutex::new(None);

pub(super) fn read_bytes(file: &File) -> Result<Vec<u8>, String> {
    if file.metadata().map_err(io)?.len() > MAX_FILE as u64 {
        return Err("会话超过 128 MiB，暂不支持编辑".into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() > MAX_FILE {
        return Err("会话超过 128 MiB，暂不支持编辑".into());
    }
    Ok(bytes)
}

pub(super) fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn old_enough(file: &File) -> bool {
    file.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|time| time.elapsed().ok())
        .is_some_and(|elapsed| elapsed >= Duration::from_secs(30))
}

fn same_revision(
    path: &Path,
    file: &File,
    expected: &str,
    last: &Option<(PathBuf, String)>,
) -> bool {
    old_enough(file)
        || last
            .as_ref()
            .is_some_and(|(p, r)| p == path && r == expected)
}

pub(super) fn is_writable_for_cached_revision(
    path: &Path,
    file: &File,
    expected: &str,
) -> Result<bool, String> {
    let last = LAST_SAVE.lock().map_err(|_| "会话保存锁不可用")?;
    Ok(same_revision(path, file, expected, &last))
}

pub(super) fn create_copy(path: &Path, bytes: &[u8], original: &File) -> Result<(), String> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut target = options.open(path).map_err(io)?;
    let result = target
        .set_permissions(original.metadata().map_err(io)?.permissions())
        .and_then(|_| target.write_all(bytes))
        .and_then(|_| target.sync_all());
    drop(target);
    if let Err(error) = result {
        let _ = fs::remove_file(path);
        return Err(io(error));
    }
    Ok(())
}

pub(super) fn mutate<T>(
    tool_id: &str,
    session_id: &str,
    expected_revision: &str,
    part: Option<usize>,
    apply: impl FnOnce(&Path, &File, &[u8], usize, &mut Option<(PathBuf, String)>) -> Result<T, String>,
) -> Result<T, String> {
    let mut last = LAST_SAVE.lock().map_err(|_| "会话保存锁不可用")?;
    let (paths, _) = locate::locate(tool_id, session_id)?;
    if part.is_none() && paths.len() != 1 {
        return Err("此会话有多个分段，暂不支持删除整个会话".into());
    }
    let path = locate::part_path(&paths, part.unwrap_or_default())?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Allow readers and atomic replacement; reject an open writer.
        options.share_mode(0x1 | 0x4);
    }
    let original = options
        .open(path)
        .map_err(|e| format!("无法锁定会话文件，请退出原会话后重试：{e}"))?;
    let bytes = read_bytes(&original)?;
    if locate::file_revision(path, &revision(&bytes)) != expected_revision {
        return Err("会话已在原 Agent 中变化，请重新打开后再编辑".into());
    }
    if !codec::inspect(
        tool_id,
        &bytes,
        0,
        same_revision(path, &original, expected_revision, &last),
    )?
    .writable
    {
        return Err("会话仍在运行或刚更新，请关闭原会话并等待 30 秒后重试".into());
    }
    apply(path, &original, &bytes, paths.len(), &mut last)
}

pub(super) fn backup_path(path: &Path, stamp: u128) -> Result<PathBuf, String> {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("会话文件名无效")?;
    Ok(path.with_file_name(format!("{name}.tokenscope-{stamp}.bak")))
}

pub(super) fn stamp() -> Result<u128, String> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos())
}

pub(super) fn move_to_backup(path: &Path, backup: &Path) -> Result<(), String> {
    if backup.try_exists().map_err(io)? {
        return Err("备份文件已存在，请重试".into());
    }
    fs::rename(path, backup).map_err(io)
}
