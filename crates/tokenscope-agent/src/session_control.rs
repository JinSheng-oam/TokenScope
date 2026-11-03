use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use tokenscope_local_ipc::{SessionDeleteRequest, SessionDeleteResponse, AGENT_CONTROL_DIRECTORY};
use tokenscope_local_store::LocalStore;

const MAX_REQUEST_BYTES: u64 = 8192;

fn control_directory(db_path: &Path) -> Result<PathBuf, &'static str> {
    db_path
        .parent()
        .map(|parent| parent.join(AGENT_CONTROL_DIRECTORY))
        .ok_or("invalid database path")
}

fn request_id_from_path(path: &Path) -> Option<&str> {
    let name = path.file_name()?.to_str()?;
    let id = name.strip_prefix("request-")?.strip_suffix(".json")?;
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

fn write_response(directory: &Path, response: &SessionDeleteResponse) -> std::io::Result<()> {
    let destination = directory.join(format!("response-{}.json", response.request_id));
    if destination.exists() {
        return Ok(());
    }
    let temporary = directory.join(format!("response-{}.tmp", response.request_id));
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

/// Handles UI deletion requests inside the already-locked Agent process.
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
    let mut requests = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| request_id_from_path(path).is_some())
        .collect::<Vec<_>>();
    requests.sort();

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
    Ok(processed)
}
