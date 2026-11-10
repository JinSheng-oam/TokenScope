use super::source::{clone_source, discover, valid_relative};
use super::{Metadata, Source, MAX_BYTES, MAX_FILES, META};
use crate::agent_customizations::{roots, valid_skill_name, CustomizationId};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) fn skill_root(id: &CustomizationId) -> Result<PathBuf, String> {
    if !valid_skill_name(&id.name) {
        return Err("无效的 skill 名称".into());
    }
    let root = roots(&id.provider)?
        .into_iter()
        .find(|r| r.id == id.root && r.kind == "skill")
        .ok_or("不支持的 skill 目录")?;
    root.require_writable()?;
    Ok(root.path)
}
pub(super) fn snapshot(root: &Path) -> Result<BTreeMap<String, String>, String> {
    fn walk(
        root: &Path,
        dir: &Path,
        out: &mut BTreeMap<String, String>,
        bytes: &mut u64,
    ) -> Result<(), String> {
        for item in fs::read_dir(dir).map_err(|e| format!("读取 skill 失败：{e}"))? {
            let item = item.map_err(|e| format!("读取 skill 失败：{e}"))?;
            if item.file_name() == ".git" {
                continue;
            }
            let ty = item
                .file_type()
                .map_err(|e| format!("读取 skill 失败：{e}"))?;
            let path = item.path();
            let rel = path
                .strip_prefix(root)
                .map_err(|_| "无效的 skill 路径")?
                .to_str()
                .ok_or("skill 路径不是 UTF-8")?
                .replace('\\', "/");
            if rel == META {
                continue;
            }
            if ty.is_symlink() {
                return Err("skill 包含符号链接，无法安全导入".into());
            }
            if ty.is_dir() {
                walk(root, &path, out, bytes)?;
            } else if ty.is_file() {
                *bytes += item
                    .metadata()
                    .map_err(|e| format!("读取文件失败：{e}"))?
                    .len();
                if *bytes > MAX_BYTES || out.len() >= MAX_FILES {
                    return Err("skill 超过 20 MiB 或 500 个文件".into());
                }
                let data = fs::read(&path).map_err(|e| format!("读取文件失败：{e}"))?;
                out.insert(rel, format!("{:x}", Sha256::digest(&data)));
            } else {
                return Err("skill 包含不支持的文件类型".into());
            }
        }
        Ok(())
    }
    let mut files = BTreeMap::new();
    let mut bytes = 0;
    walk(root, root, &mut files, &mut bytes)?;
    Ok(files)
}
pub(super) fn copy_skill(
    from: &Path,
    to: &Path,
    files: &BTreeMap<String, String>,
) -> Result<(), String> {
    fs::create_dir(to).map_err(|e| format!("创建安装目录失败：{e}"))?;
    for name in files.keys() {
        if !valid_relative(name) {
            return Err("来源包含无效路径".into());
        }
        let dest = to.join(name);
        fs::create_dir_all(dest.parent().ok_or("无效路径")?)
            .map_err(|e| format!("创建目录失败：{e}"))?;
        fs::copy(from.join(name), &dest).map_err(|e| format!("复制 skill 失败：{e}"))?;
    }
    Ok(())
}
pub(super) fn write_meta(dir: &Path, meta: &Metadata) -> Result<(), String> {
    let data = serde_json::to_vec(meta).map_err(|_| "来源信息编码失败")?;
    fs::write(dir.join(META), data).map_err(|e| format!("保存来源信息失败：{e}"))
}
pub(super) fn stage(root: &Path) -> Result<PathBuf, String> {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间无效")?
        .as_nanos();
    Ok(root.join(format!(".tokenscope-stage-{}-{n}", std::process::id())))
}
pub(super) fn install(
    id: &CustomizationId,
    source: Source,
    revision: &str,
    skill_path: &str,
) -> Result<(), String> {
    if !valid_relative(skill_path) {
        return Err("无效的 skill 路径".into());
    }
    let root = skill_root(id)?;
    fs::create_dir_all(&root).map_err(|e| format!("创建 skill 目录失败：{e}"))?;
    let target = root.join(&id.name);
    if fs::symlink_metadata(&target).is_ok() {
        return Err("目标 skill 已存在，请选择其他名称".into());
    }
    let (_tmp, repo, latest) = clone_source(&source)?;
    if latest != revision {
        return Err("仓库在预览后发生变化，请重新预览".into());
    }
    let matches = discover(&repo, &source)?;
    if !matches.iter().any(|c| c.path == skill_path) {
        return Err("skill 不在预览结果中".into());
    }
    let from = repo.join(skill_path);
    let files = snapshot(&from)?;
    if !files.contains_key("SKILL.md") {
        return Err("缺少 SKILL.md".into());
    }
    let staging = stage(&root)?;
    let result = (|| {
        copy_skill(&from, &staging, &files)?;
        write_meta(
            &staging,
            &Metadata {
                source,
                revision: latest,
                skill_path: skill_path.into(),
                files,
            },
        )?;
        if fs::symlink_metadata(&target).is_ok() {
            return Err("目标 skill 已存在".into());
        }
        fs::rename(&staging, &target).map_err(|e| format!("安装失败：{e}"))
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

pub(super) fn read_meta(path: &Path) -> Result<Option<Metadata>, String> {
    match fs::read(path.join(META)) {
        Ok(data) if data.len() <= 1024 * 1024 => serde_json::from_slice(&data)
            .map(Some)
            .map_err(|_| "来源记录损坏".into()),
        Ok(_) => Err("来源记录过大".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取来源失败：{e}")),
    }
}
