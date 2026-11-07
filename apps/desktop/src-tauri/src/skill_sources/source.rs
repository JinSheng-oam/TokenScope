use super::{Candidate, Inspection, Source};
use crate::agent_customizations::valid_skill_name;
use std::{
    env, fs,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn safe_part(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.starts_with('-')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
}
pub(super) fn source_from(input: &str) -> Result<Source, String> {
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

pub(super) struct Temp(pub(super) PathBuf);
impl Temp {
    pub(super) fn new() -> Result<Self, String> {
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
pub(super) fn git(args: &[&str], cwd: Option<&Path>) -> Result<String, String> {
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
pub(super) fn clone_source(source: &Source) -> Result<(Temp, PathBuf, String), String> {
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
pub(super) fn valid_relative(path: &str) -> bool {
    path.is_empty()
        || (path.len() <= 1024
            && Path::new(path)
                .components()
                .all(|c| matches!(c, Component::Normal(_)))
            && !path.contains('\\'))
}
pub(super) fn discover(repo: &Path, source: &Source) -> Result<Vec<Candidate>, String> {
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
pub(super) fn inspect(input: &str) -> Result<Inspection, String> {
    let source = source_from(input)?;
    let (_tmp, repo, revision) = clone_source(&source)?;
    let candidates = discover(&repo, &source)?;
    Ok(Inspection {
        source,
        revision,
        candidates,
    })
}
