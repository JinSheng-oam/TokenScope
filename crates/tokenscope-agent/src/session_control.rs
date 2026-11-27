use crate::pricing;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokenscope_local_ipc::{
    PriceCatalogRefreshRequest, PriceCatalogRefreshResponse, SessionDeleteRequest,
    SessionDeleteResponse, AGENT_CONTROL_DIRECTORY,
};
use tokenscope_local_store::LocalStore;

const MAX_REQUEST_BYTES: u64 = 8192;

fn control_directory(db_path: &Path) -> Result<PathBuf, &'static str> {
    db_path
        .parent()
        .map(|parent| parent.join(AGENT_CONTROL_DIRECTORY))
        .ok_or("invalid database path")
}

fn prefixed_request_id<'a>(path: &'a Path, prefix: &str) -> Option<&'a str> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_prefix(prefix)?.strip_suffix(".json")?;
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return None;
    }
    Some(id)
}

fn request_id_from_path(path: &Path) -> Option<&str> {
    prefixed_request_id(path, "request-")
}

fn price_refresh_request_id(path: &Path) -> Option<&str> {
    prefixed_request_id(path, "price-refresh-request-")
}

fn write_json_response<T: serde::Serialize>(
    directory: &Path,
    prefix: &str,
    request_id: &str,
    response: &T,
) -> std::io::Result<()> {
    let destination = directory.join(format!("{prefix}{request_id}.json"));
    if destination.exists() {
        return Ok(());
    }
    let temporary = directory.join(format!("{prefix}{request_id}.tmp"));
    let _ = fs::remove_file(&temporary);
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    let body = serde_json::to_vec(response).map_err(std::io::Error::other)?;
    file.write_all(&body)?;
    file.sync_all()?;
    fs::rename(temporary, destination)
}

fn write_response(directory: &Path, response: &SessionDeleteResponse) -> std::io::Result<()> {
    write_json_response(directory, "response-", &response.request_id, response)
}

/// Handles UI control requests (session deletion & price catalog refresh) inside the already-locked Agent process.
/// Each request is a small atomically-renamed JSON file in the app data folder.
pub fn process_pending_requests(
    store: &mut LocalStore,
    db_path: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    let directory = control_directory(db_path)?;
    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(error) => return Err(Box::new(error)),
    };
    let all_paths = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .collect::<Vec<_>>();
    let mut requests = all_paths
        .iter()
        .filter(|path| request_id_from_path(path).is_some())
        .cloned()
        .collect::<Vec<_>>();
    requests.sort();

    let mut price_requests = all_paths
        .into_iter()
        .filter(|path| price_refresh_request_id(path).is_some())
        .collect::<Vec<_>>();
    price_requests.sort();

    let mut processed = 0usize;
    for path in requests {
        let Some(request_id) = request_id_from_path(&path).map(str::to_owned) else {
            continue;
        };
        let parsed = fs::File::open(&path)
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(MAX_REQUEST_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_REQUEST_BYTES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "request exceeds size limit",
                    ));
                }
                Ok(bytes)
            })
            .and_then(|bytes| {
                serde_json::from_slice::<SessionDeleteRequest>(&bytes)
                    .map_err(std::io::Error::other)
            });
        let response = match parsed {
            Ok(request) if request.request_id == request_id => {
                match store.delete_session(
                    &request.tool_id,
                    &request.session_id,
                    chrono::Utc::now().timestamp_millis(),
                ) {
                    Ok(deleted_records) => SessionDeleteResponse {
                        request_id,
                        deleted_records,
                        error: None,
                    },
                    Err(error) => SessionDeleteResponse {
                        request_id,
                        deleted_records: 0,
                        error: Some(error.to_string()),
                    },
                }
            }
            Ok(_) => SessionDeleteResponse {
                request_id,
                deleted_records: 0,
                error: Some("session delete request id mismatch".into()),
            },
            Err(_) => SessionDeleteResponse {
                request_id,
                deleted_records: 0,
                error: Some("session delete request is invalid".into()),
            },
        };
        write_response(&directory, &response)?;
        let _ = fs::remove_file(path);
        processed += 1;
    }

    for path in price_requests {
        let Some(request_id) = price_refresh_request_id(&path).map(str::to_owned) else {
            continue;
        };
        let parsed = fs::File::open(&path)
            .and_then(|file| {
                let mut bytes = Vec::new();
                file.take(MAX_REQUEST_BYTES + 1).read_to_end(&mut bytes)?;
                if bytes.len() as u64 > MAX_REQUEST_BYTES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        "request exceeds size limit",
                    ));
                }
                Ok(bytes)
            })
            .and_then(|bytes| {
                serde_json::from_slice::<PriceCatalogRefreshRequest>(&bytes)
                    .map_err(std::io::Error::other)
            });
        let response = match parsed {
            Ok(request) if request.request_id == request_id => {
                match pricing::force_refresh(store, db_path) {
                    Ok((updated_at_ms, model_count, backfilled_records)) => {
                        PriceCatalogRefreshResponse {
                            request_id,
                            updated_at_ms,
                            model_count,
                            backfilled_records,
                            error: None,
                        }
                    }
                    Err(error) => PriceCatalogRefreshResponse {
                        request_id,
                        updated_at_ms: None,
                        model_count: 0,
                        backfilled_records: 0,
                        error: Some(error),
                    },
                }
            }
            Ok(_) => PriceCatalogRefreshResponse {
                request_id,
                updated_at_ms: None,
                model_count: 0,
                backfilled_records: 0,
                error: Some("price refresh request id mismatch".into()),
            },
            Err(_) => PriceCatalogRefreshResponse {
                request_id,
                updated_at_ms: None,
                model_count: 0,
                backfilled_records: 0,
                error: Some("price refresh request is invalid".into()),
            },
        };
        write_json_response(
            &directory,
            "price-refresh-response-",
            &response.request_id,
            &response,
        )?;
        let _ = fs::remove_file(path);
        processed += 1;
    }

    Ok(processed)
}
