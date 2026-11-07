//! GitHub-backed skill discovery and updates; no downloaded code is executed.
use crate::agent_customizations::CustomizationId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

mod manager;
mod source;
mod storage;
mod updates;
use source::inspect;
use storage::install;
use updates::{adopt, check, detect_source, update};

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
    use super::source::{discover, source_from, Temp};
    use super::storage::{copy_skill, snapshot};
    use std::fs;

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
