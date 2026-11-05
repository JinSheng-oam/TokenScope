//! GitHub-backed skill discovery and updates; no downloaded code is executed.
use crate::agent_customizations::{
    display_path, resolve, roots, valid_skill_name, CustomizationId,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

const META: &str = ".tokenscope-source.json";
const MAX_FILES: usize = 500;
const MAX_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Source {
    url: String,
    branch: Option<String>,
    subpath: Option<String>,
    skill: Option<String>,
}
#[derive(Clone, Serialize)]
pub(crate) struct Candidate {
    path: String,
    name: String,
    preview: String,
}
#[derive(Serialize)]
pub(crate) struct Inspection {
    source: Source,
    revision: String,
    candidates: Vec<Candidate>,
}
#[derive(Serialize, Deserialize)]
struct Metadata {
    source: Source,
    revision: String,
    skill_path: String,
    files: BTreeMap<String, String>,
}
#[derive(Serialize)]
pub(crate) struct UpdateInfo {
    kind: String,
    source: Option<String>,
    status: String,
    detail: String,
}

fn safe_part(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}
fn source_from(input: &str) -> Result<Source, String> {
    let input = input.trim().trim_end_matches('/');
    let (remaining, marketplace) = if let Some(s) = input.strip_prefix("https://github.com/") {
        (s, false)
    } else if let Some(s) = input.strip_prefix("https://skills.sh/") {
        (s, true)
    } else if !input.contains("://") {
        (input, true)
    } else {
        return Err("请输入 GitHub 仓库或 skills.sh 页面地址".into());
    };
    let parts: Vec<&str> = remaining.split('/').collect();
    if parts.len() < 2 || !safe_part(parts[0]) {
        return Err("来源应包含 GitHub 用户和仓库".into());
    }
    let (raw_repo, named_skill) = if marketplace {
        parts[1]
            .split_once('@')
            .map_or((parts[1], None), |(repo, skill)| (repo, Some(skill)))
    } else {
        (parts[1], None)
    };
    let repo = raw_repo.strip_suffix(".git").unwrap_or(raw_repo);
    if !safe_part(repo) {
        return Err("无效的仓库名称".into());
    }
    let mut branch = None;
    let mut subpath = None;
    let mut skill = None;
    if marketplace {
        if parts.len() > 3
            || (named_skill.is_some() && parts.len() > 2)
            || named_skill.is_some_and(|s| !safe_part(s))
            || parts.get(2).is_some_and(|s| !safe_part(s))
        {
            return Err("skills.sh 地址格式应为 owner/repo/skill".into());
        }
        skill = named_skill
            .or_else(|| parts.get(2).copied())
            .map(str::to_owned);
    } else if parts.len() > 2 {
        if parts.len() < 5
            || parts[2] != "tree"
            || !safe_part(parts[3])
            || parts[4..].iter().any(|s| !safe_part(s))
        {
            return Err("GitHub 子目录地址应为 /tree/分支/目录".into());
        }
        branch = Some(parts[3].to_owned());
        subpath = Some(parts[4..].join("/"));
    }
    Ok(Source {
        url: format!("https://github.com/{}/{repo}.git", parts[0]),
        branch,
        subpath,
        skill,
    })
}

struct Temp(PathBuf);
impl Temp {
    fn new() -> Result<Self, String> {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "系统时间无效")?
            .as_nanos();
        let dir = env::temp_dir().join(format!("tokenscope-skill-{}-{n}", std::process::id()));
        fs::create_dir(&dir).map_err(|e| format!("创建临时目录失败：{e}"))?;
        Ok(Self(dir))
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
    let mut command = Command::new("git");
    command.args(args).env("GIT_TERMINAL_PROMPT", "0");
    if let Some(path) = cwd {
        command.current_dir(path);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command.output().map_err(|e| format!("无法运行 Git：{e}"))?;
    if !output.status.success() {
        return Err("Git 获取仓库失败，请检查地址、网络和 Git 安装".into());
    }
    String::from_utf8(output.stdout)
        .map(|s| s.trim().to_owned())
        .map_err(|_| "Git 输出格式无效".into())
}
fn clone_source(source: &Source) -> Result<(Temp, PathBuf, String), String> {
    let canonical = source_from(&source.url)?;
    if canonical.url != source.url
        || source.branch.as_deref().is_some_and(|s| !safe_part(s))
        || source
            .subpath
            .as_deref()
            .is_some_and(|s| !valid_relative(s) || s.split('/').any(|part| !safe_part(part)))
        || source.skill.as_deref().is_some_and(|s| !safe_part(s))
    {
        return Err("来源信息无效".into());
    }
    let tmp = Temp::new()?;
    let repo = tmp.0.join("repo");
    let target = repo.to_str().ok_or("临时路径无法表示")?;
    let mut args = vec![
        "-c",
        "protocol.file.allow=never",
        "clone",
        "--quiet",
        "--depth",
        "1",
    ];
    if let Some(branch) = source.branch.as_deref() {
        args.extend(["--branch", branch]);
    }
    args.extend(["--", &source.url, target]);
    git(&args, None)?;
    let revision = git(&["rev-parse", "HEAD"], Some(&repo))?;
    Ok((tmp, repo, revision))
}
fn valid_relative(path: &str) -> bool {
    path.is_empty()
        || (path.len() <= 1024
            && Path::new(path)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            && !path.contains('\\'))
}
fn discover(repo: &Path, source: &Source) -> Result<Vec<Candidate>, String> {
    let start = source.subpath.as_deref().unwrap_or("");
    if !valid_relative(start) {
        return Err("无效的仓库子目录".into());
    }
    let actual_start = fs::canonicalize(repo.join(start)).map_err(|_| "仓库子目录不存在")?;
    let actual_repo = fs::canonicalize(repo).map_err(|_| "仓库目录不存在")?;
    if !actual_start.starts_with(&actual_repo) {
        return Err("仓库子目录越过来源边界".into());
    }
    let mut pending = vec![(repo.join(start), 0usize)];
    let mut found = Vec::new();
    let mut visited = 0usize;
    while let Some((dir, depth)) = pending.pop() {
        visited += 1;
        if visited > 5000 {
            return Err("仓库目录过多，请提供更具体的子目录".into());
        }
        if dir.join("SKILL.md").is_file() {
            let path = dir
                .strip_prefix(repo)
                .map_err(|_| "无效的仓库目录")?
                .to_string_lossy()
                .replace('\\', "/");
            let name = if path.is_empty() {
                source
                    .url
                    .rsplit('/')
                    .next()
                    .and_then(|s| s.strip_suffix(".git"))
                    .ok_or("无效的仓库名称")?
                    .to_owned()
            } else {
                dir.file_name()
                    .and_then(|v| v.to_str())
                    .ok_or("skill 名称不是 UTF-8")?
                    .to_owned()
            };
            if source.skill.as_ref().is_none_or(|want| want == &name) && valid_skill_name(&name) {
                let manifest = dir.join("SKILL.md");
                let metadata = fs::symlink_metadata(&manifest).map_err(|_| "无法读取 SKILL.md")?;
                if metadata.file_type().is_symlink() || metadata.len() > 1024 * 1024 {
                    return Err("SKILL.md 是符号链接或超过 1 MiB".into());
                }
                let body =
                    fs::read_to_string(manifest).map_err(|_| "SKILL.md 不是可读的 UTF-8 文件")?;
                let preview = body.chars().take(12000).collect();
                found.push(Candidate {
                    path,
                    name,
                    preview,
                });
            }
            continue;
        }
        if depth >= 8 {
            continue;
        }
        let entries = fs::read_dir(&dir).map_err(|e| format!("读取仓库目录失败：{e}"))?;
        for item in entries {
            let item = item.map_err(|e| format!("读取仓库目录失败：{e}"))?;
            let ty = item
                .file_type()
                .map_err(|e| format!("读取仓库目录失败：{e}"))?;
            let name = item.file_name().to_string_lossy().to_string();
            if ty.is_dir()
                && !ty.is_symlink()
                && (!name.starts_with('.')
                    || matches!(
                        name.as_str(),
                        ".agents" | ".claude" | ".codex" | ".gemini" | ".dsh"
                    ))
                && !matches!(name.as_str(), "node_modules" | "target" | "vendor")
            {
                pending.push((item.path(), depth + 1));
            }
        }
    }
    found.sort_by(|a, b| a.path.cmp(&b.path));
    if found.is_empty() {
        return Err("来源中没有找到匹配的 SKILL.md".into());
    }
    Ok(found)
}
fn inspect(input: &str) -> Result<Inspection, String> {
    let source = source_from(input)?;
    let (_tmp, repo, revision) = clone_source(&source)?;
    let candidates = discover(&repo, &source)?;
    Ok(Inspection {
        source,
        revision,
        candidates,
    })
}
fn skill_root(id: &CustomizationId) -> Result<PathBuf, String> {
    if !valid_skill_name(&id.name) {
        return Err("无效的 skill 名称".into());
    }
    let root = roots(&id.provider)?
        .into_iter()
        .find(|r| r.id == id.root && r.kind == "skill")
        .ok_or("不支持的 skill 目录")?;
    Ok(root.path)
}
fn snapshot(root: &Path) -> Result<BTreeMap<String, String>, String> {
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
fn copy_skill(from: &Path, to: &Path, files: &BTreeMap<String, String>) -> Result<(), String> {
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
fn write_meta(dir: &Path, meta: &Metadata) -> Result<(), String> {
    let data = serde_json::to_vec(meta).map_err(|_| "来源信息编码失败")?;
    fs::write(dir.join(META), data).map_err(|e| format!("保存来源信息失败：{e}"))
}
fn stage(root: &Path) -> Result<PathBuf, String> {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间无效")?
        .as_nanos();
    Ok(root.join(format!(".tokenscope-stage-{}-{n}", std::process::id())))
}
fn install(
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

#[derive(Deserialize)]
struct ManagerSkill {
    id: String,
    path: String,
    source_type: String,
    source_ref: Option<String>,
}
fn manager_exe() -> Result<Option<PathBuf>, String> {
    let home = env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" })
        .ok_or("无法确定用户目录")?;
    let bin = PathBuf::from(home).join(".skills-manager/bin");
    let exe = bin.join(if cfg!(windows) {
        "skills-manager-cli.exe"
    } else {
        "skills-manager-cli"
    });
    let stamp = bin.join(".version");
    if exe.is_file() && fs::metadata(&stamp).is_ok_and(|m| m.len() > 0) {
        Ok(Some(exe))
    } else if exe.exists() || stamp.exists() {
        Err("Skills Manager 命令行组件不完整，请先打开 Skills Manager 一次".into())
    } else {
        Ok(None)
    }
}
fn manager(args: &[&str]) -> Result<serde_json::Value, String> {
    let exe = manager_exe()?.ok_or("未找到 Skills Manager 命令行组件")?;
    let mut command = Command::new(exe);
    command.arg("--json").args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let output = command
        .output()
        .map_err(|e| format!("启动 Skills Manager 失败：{e}"))?;
    if !output.status.success() {
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap_or_default();
        return Err(error
            .get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("Skills Manager 操作失败")
            .to_owned());
    }
    serde_json::from_slice(&output.stdout).map_err(|_| "Skills Manager 返回格式无效".into())
}
fn managed(id: &CustomizationId) -> Result<Option<ManagerSkill>, String> {
    let (_, path) = resolve(id)?;
    let target = path.parent().ok_or("无效的 skill 路径")?;
    if manager_exe()?.is_none() {
        return Ok(None);
    }
    let items: Vec<ManagerSkill> = serde_json::from_value(manager(&["skills", "list"])?)
        .map_err(|_| "Skills Manager 列表格式无效")?;
    Ok(items
        .into_iter()
        .find(|s| fs::canonicalize(&s.path).is_ok_and(|p| p == target)))
}
fn read_meta(path: &Path) -> Result<Option<Metadata>, String> {
    match fs::read(path.join(META)) {
        Ok(data) if data.len() <= 1024 * 1024 => serde_json::from_slice(&data)
            .map(Some)
            .map_err(|_| "来源记录损坏".into()),
        Ok(_) => Err("来源记录过大".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("读取来源失败：{e}")),
    }
}
fn check(id: &CustomizationId) -> Result<UpdateInfo, String> {
    if let Some(skill) = managed(id)? {
        let report = manager(&["skills", "check", &skill.id])?;
        let row = report
            .as_array()
            .and_then(|rows| rows.first())
            .ok_or("Skills Manager 检查结果为空")?;
        let status = row
            .get("update_status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let detail = match status {
            "available" | "update_available" => "Skills Manager 检测到更新",
            "up_to_date" | "current" => "Skills Manager 未发现更新",
            "source_missing" => "Skills Manager 找不到原始来源",
            _ => "Skills Manager 无法确认是否有更新，可尝试手动更新",
        };
        return Ok(UpdateInfo {
            kind: "manager".into(),
            source: skill.source_ref,
            status: status.into(),
            detail: format!("{detail}（来源类型：{}）", skill.source_type),
        });
    }
    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效路径")?;
    let Some(meta) = read_meta(dir)? else {
        return Ok(UpdateInfo {
            kind: "unknown".into(),
            source: None,
            status: "unsupported".into(),
            detail: "没有可验证的来源记录，无法自动更新".into(),
        });
    };
    let changed = snapshot(dir)? != meta.files;
    let (_tmp, _repo, latest) = clone_source(&meta.source)?;
    Ok(UpdateInfo {
        kind: "independent".into(),
        source: Some(meta.source.url),
        status: if changed {
            "local_changes"
        } else if latest == meta.revision {
            "current"
        } else {
            "available"
        }
        .into(),
        detail: if changed {
            "本地文件已修改，更新已锁定"
        } else if latest == meta.revision {
            "当前已是最新版本"
        } else {
            "检测到仓库更新"
        }
        .into(),
    })
}
fn update(id: &CustomizationId) -> Result<String, String> {
    if let Some(skill) = managed(id)? {
        let report = manager(&["skills", "update", &skill.id])?;
        if report.to_string().contains("held_back_removals") {
            return Err(format!("Skills Manager 保留了更新以保护文件：{report}"));
        }
        let refreshed = report.get("refreshed").and_then(|v| v.as_bool()) == Some(true)
            || report.as_array().is_some_and(|rows| {
                rows.iter()
                    .any(|r| r.get("refreshed").and_then(|v| v.as_bool()) == Some(true))
            });
        return Ok(if refreshed {
            "Skills Manager 已更新 skill".into()
        } else {
            "Skills Manager 未应用更新，请检查来源或稍后重试".into()
        });
    }
    let root = skill_root(id)?;
    let target = root.join(&id.name);
    if fs::symlink_metadata(&target)
        .map_err(|_| "skill 不存在")?
        .file_type()
        .is_symlink()
    {
        return Err("独立更新不支持软链接，请通过原管理工具更新".into());
    }
    let meta = read_meta(&target)?.ok_or("没有可验证的来源记录，无法自动更新")?;
    if snapshot(&target)? != meta.files {
        return Err("本地文件已修改，请先备份或还原后再更新".into());
    }
    let (_tmp, repo, revision) = clone_source(&meta.source)?;
    if revision == meta.revision {
        return Ok("当前已是最新版本".into());
    }
    if !discover(&repo, &meta.source)?
        .iter()
        .any(|c| c.path == meta.skill_path)
    {
        return Err("来源中找不到原 skill".into());
    }
    let from = repo.join(&meta.skill_path);
    let files = snapshot(&from)?;
    let staging = stage(&root)?;
    let backup = stage(&root)?.with_extension("backup");
    let result = (|| {
        copy_skill(&from, &staging, &files)?;
        write_meta(
            &staging,
            &Metadata {
                source: meta.source,
                revision,
                skill_path: meta.skill_path,
                files,
            },
        )?;
        if snapshot(&target)? != meta.files {
            return Err("文件在更新期间发生改变，已取消".into());
        }
        fs::rename(&target, &backup).map_err(|e| format!("准备更新失败：{e}"))?;
        if let Err(e) = fs::rename(&staging, &target) {
            if fs::rename(&backup, &target).is_err() {
                return Err(format!("更新失败，原文件保留在 {}", display_path(&backup)));
            }
            return Err(format!("更新失败，原文件已恢复：{e}"));
        }
        fs::remove_dir_all(&backup)
            .map_err(|_| format!("更新完成，原文件备份保留在 {}", display_path(&backup)))?;
        Ok("skill 已更新".into())
    })();
    if staging.exists() {
        let _ = fs::remove_dir_all(staging);
    }
    result
}

#[tauri::command]
pub(crate) async fn inspect_skill_source(input: String) -> Result<Inspection, String> {
    tauri::async_runtime::spawn_blocking(move || inspect(&input))
        .await
        .map_err(|_| "预览任务失败")?
}
#[tauri::command]
pub(crate) async fn install_skill_source(
    id: CustomizationId,
    source: Source,
    revision: String,
    skill_path: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || install(&id, source, &revision, &skill_path))
        .await
        .map_err(|_| "安装任务失败")?
}
#[tauri::command]
pub(crate) async fn check_skill_update(id: CustomizationId) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || check(&id))
        .await
        .map_err(|_| "检查任务失败")?
}
#[tauri::command]
pub(crate) async fn update_skill_source(id: CustomizationId) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || update(&id))
        .await
        .map_err(|_| "更新任务失败")?
}

fn detect_source(id: &CustomizationId) -> Result<Option<String>, String> {
    if let Some(skill) = managed(id)? {
        if let Some(s) = skill.source_ref {
            return Ok(Some(s));
        }
    }
    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效路径")?;
    let git_dir = dir.join(".git");
    if git_dir.exists() {
        if let Ok(url) = git(&["config", "--get", "remote.origin.url"], Some(dir)) {
            let trimmed = url.trim();
            if !trimmed.is_empty() {
                let https_url = if let Some(rem) = trimmed.strip_prefix("git@github.com:") {
                    format!("https://github.com/{}", rem.trim_end_matches(".git"))
                } else if trimmed.starts_with("https://") {
                    trimmed.trim_end_matches(".git").to_string()
                } else {
                    trimmed.to_string()
                };
                return Ok(Some(https_url));
            }
        }
    }
    Ok(None)
}

fn adopt(
    id: &CustomizationId,
    source_input: &str,
    candidate_path: Option<&str>,
) -> Result<UpdateInfo, String> {
    let source = source_from(source_input)?;
    let (_tmp, repo, latest) = clone_source(&source)?;
    let candidates = discover(&repo, &source)?;

    let matched = if let Some(cp) = candidate_path {
        candidates
            .iter()
            .find(|c| c.path == cp)
            .cloned()
            .ok_or("指定的子目录不是有效的 skill")?
    } else if candidates.len() == 1 {
        candidates[0].clone()
    } else if let Some(m) = candidates.iter().find(|c| c.name == id.name) {
        m.clone()
    } else {
        return Err("来源包含多个 skill，请指定具体子目录".into());
    };

    let (_, path) = resolve(id)?;
    let dir = path.parent().ok_or("无效路径")?;
    let canonical_dir = if dir.is_symlink() {
        fs::canonicalize(dir).map_err(|e| format!("解析符号链接失败：{e}"))?
    } else {
        dir.to_path_buf()
    };

    let files = snapshot(&canonical_dir)?;
    write_meta(
        &canonical_dir,
        &Metadata {
            source,
            revision: latest,
            skill_path: matched.path,
            files,
        },
    )?;

    check(id)
}

#[tauri::command]
pub(crate) async fn detect_skill_source(id: CustomizationId) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || detect_source(&id))
        .await
        .map_err(|_| "检测来源任务失败")?
}

#[tauri::command]
pub(crate) async fn adopt_skill_source(
    id: CustomizationId,
    source_input: String,
    candidate_path: Option<String>,
) -> Result<UpdateInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        adopt(&id, &source_input, candidate_path.as_deref())
    })
    .await
    .map_err(|_| "纳管任务失败")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_sources_and_rejects_untrusted_urls() {
        let source =
            source_from("https://skills.sh/vercel-labs/agent-skills/react-best-practices").unwrap();
        assert_eq!(
            source.url,
            "https://github.com/vercel-labs/agent-skills.git"
        );
        assert_eq!(source.skill.as_deref(), Some("react-best-practices"));
        assert_eq!(
            source_from("vercel-labs/agent-skills@react-best-practices")
                .unwrap()
                .skill,
            source.skill
        );
        let tree = source_from("https://github.com/owner/repo/tree/main/skills/demo").unwrap();
        assert_eq!(tree.branch.as_deref(), Some("main"));
        assert_eq!(tree.subpath.as_deref(), Some("skills/demo"));
        for bad in [
            "https://evil.example/owner/repo",
            "https://github.com/a/../repo",
            "https://github.com/user:pass@host/repo",
            "--upload-pack=evil/repo",
            "https://github.com/owner/repo?x=1",
        ] {
            assert!(source_from(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn discovery_and_snapshot_keep_all_skill_files_and_detect_changes() {
        let temp = Temp::new().unwrap();
        let repo = temp.0.join("repo");
        let skill = repo.join("skills/example");
        fs::create_dir_all(skill.join("references")).unwrap();
        fs::write(skill.join("SKILL.md"), "name: example").unwrap();
        fs::write(skill.join("references/guide.md"), "guide").unwrap();
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/config"), "not part of a skill").unwrap();
        let source = source_from("https://github.com/owner/repo").unwrap();
        let candidates = discover(&repo, &source).unwrap();
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].path, "skills/example");
        let before = snapshot(&skill).unwrap();
        assert_eq!(before.len(), 2);
        let staged = temp.0.join("staged");
        copy_skill(&skill, &staged, &before).unwrap();
        assert_eq!(snapshot(&staged).unwrap(), before);
        fs::write(staged.join("references/guide.md"), "changed").unwrap();
        assert_ne!(snapshot(&staged).unwrap(), before);
        assert!(!snapshot(&repo).unwrap().contains_key(".git/config"));
        fs::remove_file(staged.join("references/guide.md")).unwrap();
        assert_ne!(snapshot(&staged).unwrap(), before);
    }
}
