use super::{paths::check_target, resolve, CustomizationId, Root};
use std::{
    fs,
    io::Write,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE: usize = 1024 * 1024;

pub(super) fn read(id: &CustomizationId) -> Result<Option<String>, String> {
    let (root, path) = resolve(id)?;
    read_path(&root, &path)
}
pub(crate) fn read_path(root: &Root, path: &Path) -> Result<Option<String>, String> {
    check_target(root, path)?;
    match fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= MAX_FILE as u64 => fs::read_to_string(path)
            .map(Some)
            .map_err(|e| format!("读取文件失败：{e}")),
        Ok(_) => Err("文件不是普通文件或超过 1 MiB".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取文件失败：{e}")),
    }
}

#[cfg(windows)]
pub(crate) fn replace_file(from: &Path, to: &Path, exists: bool) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *mut std::ffi::c_void,
            reserved: *mut std::ffi::c_void,
        ) -> i32;
    }
    let from: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // ReplaceFileW preserves the existing target's security attributes. For a
    // new file, MoveFileExW without REPLACE_EXISTING rejects a racing creator.
    let result = if exists {
        unsafe {
            ReplaceFileW(
                to.as_ptr(),
                from.as_ptr(),
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        }
    } else {
        unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x8) }
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
pub(crate) fn replace_file(from: &Path, to: &Path, exists: bool) -> std::io::Result<()> {
    if exists {
        fs::rename(from, to)
    } else {
        fs::hard_link(from, to)
    }
}

pub(super) fn save(
    id: &CustomizationId,
    expected: Option<String>,
    content: String,
) -> Result<(), String> {
    let (root, path) = resolve(id)?;
    save_path(&root, &path, expected, content)
}
pub(crate) fn save_path(
    root: &Root,
    path: &Path,
    expected: Option<String>,
    content: String,
) -> Result<(), String> {
    if content.len() > MAX_FILE {
        return Err("文件超过 1 MiB".into());
    }
    if read_path(root, path)? != expected {
        return Err("文件已在外部更改，请重新加载后再保存".into());
    }
    let parent = path.parent().ok_or("无效路径")?;
    fs::create_dir_all(parent).map_err(|e| format!("创建目录失败：{e}"))?;
    check_target(root, path)?;
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间无效")?
        .as_nanos();
    let temp = parent.join(format!(".tokenscope-{}-{stamp}.tmp", std::process::id()));
    let result = (|| {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts
            .open(&temp)
            .map_err(|e| format!("创建临时文件失败：{e}"))?;
        if let Ok(meta) = fs::metadata(path) {
            file.set_permissions(meta.permissions())
                .map_err(|e| format!("设置文件权限失败：{e}"))?;
        }
        file.write_all(content.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("写入文件失败：{e}"))?;
        drop(file);
        if read_path(root, path)? != expected {
            return Err("文件已在外部更改，请重新加载后再保存".into());
        }
        replace_file(&temp, path, expected.is_some()).map_err(|e| format!("保存文件失败：{e}"))
    })();
    let _ = fs::remove_file(&temp);
    result
}
