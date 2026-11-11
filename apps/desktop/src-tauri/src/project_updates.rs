use semver::Version;
use serde::{Deserialize, Serialize};
use std::time::Duration;

const REPOSITORY: &str = "https://github.com/JinSheng-oam/TokenScope";
const RELEASES: &str = "https://github.com/JinSheng-oam/TokenScope/releases";
const API_REPOSITORY: &str = "https://api.github.com/repos/JinSheng-oam/TokenScope";
const TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Deserialize)]
struct Release {
    tag_name: String,
}

#[derive(Serialize)]
pub(crate) struct ProjectUpdate {
    latest_version: String,
    update_available: bool,
}

impl Release {
    fn compare(&self, current: &Version) -> Result<ProjectUpdate, String> {
        let tag = self.tag_name.strip_prefix('v').unwrap_or(&self.tag_name);
        let latest = Version::parse(tag).map_err(|_| "GitHub 发布标签不是有效的版本号")?;
        Ok(ProjectUpdate {
            latest_version: latest.to_string(),
            update_available: latest.cmp_precedence(current).is_gt(),
        })
    }
}

#[tauri::command]
pub(crate) async fn check_project_update(
    app: tauri::AppHandle,
) -> Result<Option<ProjectUpdate>, String> {
    let client = reqwest::Client::builder()
        .user_agent("TokenScope")
        .default_headers(reqwest::header::HeaderMap::from_iter([(
            reqwest::header::ACCEPT,
            reqwest::header::HeaderValue::from_static("application/vnd.github+json"),
        )]))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(TIMEOUT)
        .build()
        .map_err(|_| "无法初始化更新检查")?;
    let response = client
        .get(format!("{API_REPOSITORY}/releases/latest"))
        .send()
        .await
        .map_err(|_| "无法连接 GitHub，请检查网络后重试")?;
    let release = match response.status().as_u16() {
        200 => response
            .json::<Release>()
            .await
            .map_err(|_| "无法读取 GitHub 发布信息")?,
        404 => {
            // GitHub also returns 404 when the repository is not publicly accessible.
            let repository = client
                .get(API_REPOSITORY)
                .send()
                .await
                .map_err(|_| "无法连接 GitHub，请检查网络后重试")?;
            return match repository.status().as_u16() {
                200 => Ok(None),
                404 => Err("GitHub 仓库目前无法公开访问".into()),
                status => Err(format!("GitHub 更新查询失败（HTTP {status}）")),
            };
        }
        status => return Err(format!("GitHub 更新查询失败（HTTP {status}）")),
    };
    release.compare(&app.package_info().version).map(Some)
}

#[tauri::command]
pub(crate) async fn open_project_page(releases: bool) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        open::that_detached(if releases { RELEASES } else { REPOSITORY })
            .map_err(|_| "无法打开默认浏览器".to_owned())
    })
    .await
    .map_err(|_| "无法打开默认浏览器".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_release_tags_using_semver_precedence() {
        let cases = [
            ("v0.10.0", "0.9.9", true),
            ("v0.10.0", "0.10.0-rc.1", true),
            ("0.10.0", "0.10.0+local", false),
            ("v0.9.9", "0.10.0", false),
        ];
        for (tag_name, current, update_available) in cases {
            let update = Release {
                tag_name: tag_name.into(),
            }
            .compare(&Version::parse(current).unwrap())
            .unwrap();
            assert_eq!(update.update_available, update_available);
        }
        assert!(Release {
            tag_name: "nightly".into()
        }
        .compare(&Version::new(0, 1, 0))
        .is_err());
    }
}
