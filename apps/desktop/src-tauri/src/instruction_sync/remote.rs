use super::*;
const REMOTE: &str = "tokenscope-shared-instructions-v1.json";
#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    markdown: String,
}

pub(super) fn sync(
    app: &AppHandle,
    state: &mut State,
    resolve: Option<String>,
    pull_only: bool,
) -> Result<(String, Option<String>, bool), String> {
    if !state.enabled {
        return Err("请先启用统一指令 WebDAV 同步".into());
    }
    let config: WebDavConfigFile = serde_json::from_str(
        &fs::read_to_string(crate::paths::webdav_config_path(app)?)
            .map_err(|_| "请先配置 WebDAV")?,
    )
    .map_err(|e| e.to_string())?;
    if !config.enabled {
        return Err("WebDAV 已关闭".into());
    }
    let url = WebDavUrl::parse(&config.url).map_err(|e| e.to_string())?;
    let endpoint = hash(&format!("{}\n{}", url.as_str(), config.username));
    let key = WebDavCredentialKey::for_endpoint(url.as_str(), &config.username);
    let password = KeyringWebDavStore
        .load(&key)
        .map_err(|e| e.to_string())?
        .ok_or("找不到 WebDAV 凭据")?;
    let client = WebDavClient::new(
        url,
        WebDavCredentials {
            username: config.username,
            password,
        },
    )
    .map_err(|e| e.to_string())?;
    let remote = client
        .read_versioned_document(REMOTE, LIMIT * 6 + 100)
        .map_err(|e| e.to_string())?;
    let base = if state.endpoint.as_ref() == Some(&endpoint) {
        state.base.as_deref()
    } else {
        None
    };
    let local = state.draft.as_deref();
    let mut downloaded = false;
    if let Some((body, tag)) = remote {
        let doc: Document = serde_json::from_str(&body).map_err(|_| "远端文档格式无效")?;
        if doc.version != 1 {
            return Err("远端文档版本不受支持".into());
        }
        validate(&doc.markdown)?;
        let remote_hash = hash(&doc.markdown);
        let local_hash = local.map(hash);
        if local == Some(doc.markdown.as_str()) {
            state.base = Some(remote_hash);
        } else if resolve.as_deref() == Some(remote_hash.as_str())
            || (local.is_some() && base == Some(remote_hash.as_str()))
        {
            if pull_only {
                return Ok((String::new(), None, false));
            }
            let markdown = local.ok_or("请先保存合并后的主稿")?.to_owned();
            let tag = tag.ok_or("服务器未提供可用于安全写入的 ETag")?;
            publish(&client, &markdown, Some(&tag))?;
            state.base = Some(hash(&markdown));
        } else if local.is_none() || local_hash.as_deref() == base {
            state.draft = Some(doc.markdown);
            state.base = Some(remote_hash);
            downloaded = true;
        } else {
            return Ok((
                "两端均有修改，请对照远端版本处理".into(),
                Some(doc.markdown),
                false,
            ));
        }
    } else if base.is_some() {
        return Err("远端文档缺失，已保留本机主稿".into());
    } else if let Some(markdown) = local {
        if pull_only {
            return Ok((String::new(), None, false));
        }
        publish(&client, markdown, None)?;
        state.base = Some(hash(markdown));
    } else {
        return Ok(("本机没有已保存的主稿，当前 WebDAV 目录也没有主稿。请在有主稿的设备保存并同步，并核对两台设备使用同一 WebDAV 目录。".into(), None, false));
    }
    state.endpoint = Some(endpoint);
    Ok(("设备同步完成".into(), None, downloaded))
}

fn publish(client: &WebDavClient, markdown: &str, tag: Option<&str>) -> Result<(), String> {
    let body = serde_json::to_string(&Document {
        version: 1,
        markdown: markdown.into(),
    })
    .map_err(|e| e.to_string())?;
    client
        .write_versioned_document(REMOTE, body, tag)
        .map_err(|e| e.to_string())
}
