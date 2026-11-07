use crate::agent_customizations::{resolve, CustomizationId};
use serde::Deserialize;
use std::{env, fs, path::PathBuf, process::Command};

#[derive(Deserialize)]
pub(super) struct ManagerSkill {
    pub(super) id: String,
    path: String,
    pub(super) source_type: String,
    pub(super) source_ref: Option<String>,
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
pub(super) fn manager(args: &[&str]) -> Result<serde_json::Value, String> {
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
pub(super) fn managed(id: &CustomizationId) -> Result<Option<ManagerSkill>, String> {
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
