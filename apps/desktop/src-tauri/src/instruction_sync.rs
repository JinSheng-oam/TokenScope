use crate::agent_customizations::{self as agent, CustomizationId, Root};
use crate::webdav::WebDavConfigFile;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tauri::AppHandle;
use tokenscope_sync_client::webdav::{
    KeyringWebDavStore, WebDavClient, WebDavCredentialKey, WebDavCredentials, WebDavSecretStore,
    WebDavUrl,
};

const LIMIT: usize = 128 * 1024;
const START: &str = "<!-- tokenscope:shared-instructions:start -->";
const END: &str = "<!-- tokenscope:shared-instructions:end -->";
mod remote;
static LOCK: Mutex<()> = Mutex::new(());

#[derive(Default, Serialize, Deserialize)]
struct State {
    draft: Option<String>,
    enabled: bool,
    auto_apply: bool,
    selected: Vec<String>,
    base: Option<String>,
    endpoint: Option<String>,
    applied: BTreeMap<String, String>,
    #[serde(default)]
    last_results: Vec<String>,
}

#[derive(Serialize)]
struct Target {
    key: String,
    label: &'static str,
    path: String,
    initialized: bool,
}

#[derive(Serialize)]
struct Change {
    key: String,
    path: String,
    before: Option<String>,
    after: Option<String>,
    error: Option<String>,
    token: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct View {
    draft: Option<String>,
    enabled: bool,
    auto_apply: bool,
    selected: Vec<String>,
    targets: Vec<Target>,
    changes: Vec<Change>,
    results: Vec<String>,
    conflict: Option<String>,
    conflict_token: Option<String>,
    message: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum Action {
    Load,
    Poll,
    Save {
        markdown: String,
        expected: Option<String>,
    },
    Configure {
        enabled: bool,
        auto_apply: bool,
        selected: Vec<String>,
    },
    Sync {
        resolve: Option<String>,
    },
    Preview,
    QuickApply,
    Apply {
        approved: BTreeMap<String, String>,
    },
    Restore {
        key: String,
    },
}

fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

fn validate(text: &str) -> Result<(), String> {
    if text.len() > LIMIT || text.contains(START) || text.contains(END) || text.contains('\0') {
        return Err("主稿超过 128 KiB 或包含保留标记".into());
    }
    Ok(())
}

fn entries() -> Result<Vec<(Target, CustomizationId)>, String> {
    let mut targets = Vec::new();
    for (provider, root, name, label) in [
        ("codex", "codex-instructions", "AGENTS.md", "Codex"),
        ("claude", "claude-instructions", "CLAUDE.md", "Claude Code"),
        (
            "antigravity",
            "gemini-instructions",
            "GEMINI.md",
            "Gemini / Antigravity",
        ),
        (
            "antigravity",
            "gemini-config-instructions",
            "GEMINI.md",
            "Antigravity 配置目录",
        ),
        (
            "deepseek",
            "dsh-instructions",
            "AGENTS.md",
            "DeepSeek Harness",
        ),
        (
            "deepseek",
            "dsh-shared-instructions",
            "AGENTS.md",
            "共享 Agents 目录",
        ),
    ] {
        let mut id = CustomizationId {
            provider: provider.into(),
            root: root.into(),
            name: name.into(),
        };
        let (config, _) = agent::resolve(&id)?;
        let mut path = config.path.join(&id.name);
        if provider == "codex" {
            let override_path = config.path.join("AGENTS.override.md");
            match fs::symlink_metadata(&override_path) {
                Ok(_) => {
                    id.name = "AGENTS.override.md".into();
                    path = override_path;
                }
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        let path = agent::display_path(&path);
        targets.push((
            Target {
                key: format!("{root}/{}", id.name),
                label,
                path,
                initialized: false,
            },
            id,
        ));
    }
    Ok(targets)
}

fn replace_block(
    old: &str,
    draft: &str,
    applied: Option<&str>,
) -> Result<(String, String), String> {
    let newline = if old.contains("\r\n") { "\r\n" } else { "\n" };
    let content = draft.replace("\r\n", "\n").replace('\n', newline);
    let block = format!(
        "{START}{newline}{}{newline}{END}",
        content.trim_end_matches(['\r', '\n'])
    );
    match (old.find(START), old.find(END)) {
        (None, None) if applied.is_none() => {
            let gap = if old.is_empty() {
                String::new()
            } else {
                newline.repeat(2)
            };
            Ok((format!("{old}{gap}{block}{newline}"), hash(&block)))
        }
        (Some(start), Some(end))
            if start < end && old.matches(START).count() == 1 && old.matches(END).count() == 1 =>
        {
            let end = end + END.len();
            let old_block = &old[start..end];
            if old_block != block && applied != Some(hash(old_block).as_str()) {
                return Err("统一指令区块已被手动修改，请在原文件中检查".into());
            }
            Ok((
                format!("{}{block}{}", &old[..start], &old[end..]),
                hash(&block),
            ))
        }
        _ => Err("统一指令标记缺失、重复或损坏".into()),
    }
}

fn data_root(path: &Path) -> Root {
    Root {
        id: "instruction-sync",
        path: path.to_owned(),
        kind: "instruction",
        names: &[],
    }
}

fn read_json<T: serde::de::DeserializeOwned>(
    root: &Root,
    path: &Path,
) -> Result<Option<T>, String> {
    agent::read_path(root, path)?
        .map(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
        .transpose()
}

fn write_json(root: &Root, path: &Path, data: &impl Serialize) -> Result<(), String> {
    agent::save_path(
        root,
        path,
        agent::read_path(root, path)?,
        serde_json::to_string(data).map_err(|e| e.to_string())?,
    )
}

fn checked_path(id: &CustomizationId) -> Result<(Root, PathBuf), String> {
    let (root, _) = agent::resolve(id)?;
    let path = root.path.join(&id.name);
    if path.ancestors().any(agent::is_link) {
        return Err("目标或上级目录是链接，已跳过".into());
    }
    Ok((root, path))
}

#[derive(Serialize, Deserialize)]
struct Backup {
    before: Option<String>,
    after_hash: String,
    prior_block: Option<String>,
}

fn backup_path(root: &Root, target: &Target) -> PathBuf {
    root.path
        .join(format!("backup-{}.json", hash(&target.path)))
}

fn plan(target: &Target, id: &CustomizationId, state: &State) -> Result<Change, String> {
    let (root, path) = checked_path(id)?;
    let before = agent::read_path(&root, &path)?;
    let draft = state.draft.as_deref().ok_or("请先保存主稿")?;
    let (after, _) = replace_block(
        before.as_deref().unwrap_or(""),
        draft,
        state.applied.get(&target.path).map(String::as_str),
    )?;
    Ok(Change {
        key: target.key.clone(),
        path: target.path.clone(),
        before,
        after: Some(after),
        error: None,
        token: None,
    })
}

impl Change {
    fn approval(&self) -> String {
        hash(&serde_json::to_string(&(&self.path, &self.before, &self.after)).unwrap_or_default())
    }
}

fn apply_one(
    data: &Root,
    state: &mut State,
    target: &Target,
    id: &CustomizationId,
    approved: Option<&str>,
    automatic: bool,
) -> Result<&'static str, String> {
    let change = plan(target, id, state)?;
    if !automatic && approved != Some(change.approval().as_str()) {
        return Err("预览已过期，请重新预览".into());
    }
    let before = change.before;
    let after = change.after.ok_or("没有应用内容")?;
    let (_, block) = replace_block(
        before.as_deref().unwrap_or(""),
        state.draft.as_deref().ok_or("请先保存主稿")?,
        state.applied.get(&target.path).map(String::as_str),
    )?;
    if before.as_deref() == Some(&after) {
        state.applied.insert(target.path.clone(), block);
        return Ok("已是最新");
    }
    let (root, path) = checked_path(id)?;
    write_json(
        data,
        &backup_path(data, target),
        &Backup {
            before: before.clone(),
            after_hash: hash(&after),
            prior_block: state.applied.get(&target.path).cloned(),
        },
    )?;
    agent::save_path(&root, &path, before, after)?;
    state.applied.insert(target.path.clone(), block);
    Ok("已应用")
}

fn run(app: &AppHandle, action: Action) -> Result<View, String> {
    let _guard = LOCK.lock().map_err(|_| "统一指令正在恢复")?;
    let dir = crate::paths::webdav_config_path(app)?.with_file_name("instruction-sync");
    let data = data_root(&dir);
    if dir.ancestors().any(agent::is_link) {
        return Err("统一指令数据目录包含链接".into());
    }
    let path = dir.join("state.json");
    let mut state: State = read_json(&data, &path)?.unwrap_or_default();
    let entries = entries()?;
    let mut changes = Vec::new();
    let mut results = Vec::new();
    let mut conflict = None;
    let mut message = String::new();
    let mut apply = None;
    match action {
        Action::Load => {}
        Action::Poll if state.enabled => {
            let prior = (state.endpoint.clone(), state.base.clone());
            let (_, _, downloaded) = remote::sync(app, &mut state, None, true)?;
            if downloaded || prior != (state.endpoint.clone(), state.base.clone()) {
                write_json(&data, &path, &state)?;
            }
            if downloaded && state.auto_apply {
                apply = Some((true, BTreeMap::new()));
            }
        }
        Action::Poll => {}
        Action::Save { markdown, expected } => {
            if expected != state.draft {
                return Err("主稿已更新，请复制编辑内容后重新打开统一指令".into());
            }
            validate(&markdown)?;
            state.draft = Some(markdown);
            message = "主稿已保存".into();
        }
        Action::Configure {
            enabled,
            auto_apply,
            selected,
        } => {
            state.enabled = enabled;
            state.auto_apply = auto_apply && enabled;
            state.selected = entries
                .iter()
                .filter(|(t, _)| selected.contains(&t.key))
                .map(|(t, _)| t.key.clone())
                .collect();
            message = "本机设置已保存".into();
        }
        Action::Sync { resolve } => {
            let downloaded;
            (message, conflict, downloaded) = remote::sync(app, &mut state, resolve, false)?;
            if state.auto_apply && downloaded {
                write_json(&data, &path, &state)?;
                apply = Some((true, BTreeMap::new()));
            }
        }
        Action::Preview => {
            for (target, id) in entries
                .iter()
                .filter(|(t, _)| state.selected.contains(&t.key))
            {
                let mut change = plan(target, id, &state).unwrap_or_else(|error| Change {
                    key: target.key.clone(),
                    path: target.path.clone(),
                    before: None,
                    after: None,
                    error: Some(error),
                    token: None,
                });
                if change.error.is_none() {
                    change.token = Some(change.approval());
                }
                changes.push(change);
            }
        }
        Action::QuickApply => apply = Some((true, BTreeMap::new())),
        Action::Apply { approved } => apply = Some((false, approved)),
        Action::Restore { key } => {
            let (target, id) = entries
                .iter()
                .find(|(t, _)| t.key == key)
                .ok_or("目标已变化")?;
            let (root, target_path) = checked_path(id)?;
            let backup_file = backup_path(&data, target);
            let backup: Backup = read_json(&data, &backup_file)?.ok_or("没有可恢复的备份")?;
            let current = agent::read_path(&root, &target_path)?.ok_or("目标已被删除")?;
            if hash(&current) != backup.after_hash {
                return Err("应用后文件又被修改，无法安全恢复".into());
            }
            if let Some(before) = backup.before {
                agent::save_path(&root, &target_path, Some(current), before)?;
            } else {
                if agent::read_path(&root, &target_path)?.as_deref() != Some(&current) {
                    return Err("目标已变化".into());
                }
                fs::remove_file(&target_path).map_err(|e| e.to_string())?;
            }
            if let Some(prior) = backup.prior_block {
                state.applied.insert(target.path.clone(), prior);
            } else {
                state.applied.remove(&target.path);
            }
            message = "已恢复应用前的文件".into();
        }
    }
    if let Some((automatic, approved)) = apply {
        for (target, id) in &entries {
            if !state.selected.contains(&target.key) {
                continue;
            }
            if automatic && !state.applied.contains_key(&target.path) {
                results.push(format!("{}：请先预览并手动应用一次", target.label));
                continue;
            }
            if !automatic && !approved.contains_key(&target.key) {
                continue;
            }
            let result = apply_one(
                &data,
                &mut state,
                target,
                id,
                approved.get(&target.key).map(String::as_str),
                automatic,
            );
            results.push(format!(
                "{}：{}",
                target.label,
                result.map(str::to_owned).unwrap_or_else(|e| e)
            ));
        }
    }
    if !results.is_empty() {
        state.last_results.clone_from(&results);
    }
    if !message.is_empty() || !results.is_empty() {
        write_json(&data, &path, &state)?;
    }
    let conflict_token = conflict.as_deref().map(hash);
    Ok(View {
        draft: state.draft,
        enabled: state.enabled,
        auto_apply: state.auto_apply,
        selected: state.selected,
        targets: entries
            .into_iter()
            .map(|(mut t, _)| {
                t.initialized = state.applied.contains_key(&t.path);
                t
            })
            .collect(),
        changes,
        results: state.last_results,
        conflict,
        conflict_token,
        message,
    })
}

pub(crate) fn start_polling(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(std::time::Duration::from_secs(60));
        let _ = run(&app, Action::Poll);
    });
}

#[tauri::command]
pub(crate) async fn shared_instruction(app: AppHandle, action: Action) -> Result<View, String> {
    tauri::async_runtime::spawn_blocking(move || run(&app, action))
        .await
        .map_err(|_| "统一指令任务失败")?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_local_text_and_rejects_modified_managed_block() {
        let (initial, digest) = replace_block("local\r\n", "first", None).unwrap();
        let (updated, _) = replace_block(&initial, "next", Some(&digest)).unwrap();
        assert!(updated.starts_with("local\r\n"));
        assert!(updated.contains("\r\nnext\r\n"));
        assert!(
            replace_block(&initial.replace("first", "external"), "next", Some(&digest)).is_err()
        );
        assert!(replace_block(&initial, "next", None).is_err());
    }
}
