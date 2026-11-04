//! Device identity lives in two places: the SQLite row and a tiny marker file
//! beside the database. The marker is the guard that makes a silent database
//! replacement visible: a missing database, or a database carrying a different
//! identity than the marker, is refused instead of being adopted as a new
//! device.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(super) fn marker_path(db: &Path) -> PathBuf {
    let mut name = db.as_os_str().to_os_string();
    name.push(".device-id");
    PathBuf::from(name)
}

fn valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.is_ascii()
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

pub(super) fn read(path: &Path) -> io::Result<Option<String>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    let id = text.trim();
    if !valid(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid device identity marker: {}", path.display()),
        ));
    }
    Ok(Some(id.to_owned()))
}

pub(super) fn write(path: &Path, id: &str) -> io::Result<()> {
    if !valid(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid device identity",
        ));
    }
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "marker path"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = path.as_os_str().to_os_string();
    temporary.push(format!(".tmp-{}", std::process::id()));
    let temporary = PathBuf::from(temporary);
    {
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&temporary)?;
        file.write_all(id.as_bytes())?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    // Windows rename refuses to replace an existing destination.
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
    }
    match fs::rename(&temporary, path) {
        Ok(()) => Ok(()),
        Err(error) => {
            let _ = fs::remove_file(&temporary);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marker_roundtrip_and_validation() {
        let dir = std::env::temp_dir().join(format!("tokenscope-marker-{}", std::process::id()));
        let db = dir.join("tokenscope.db");
        let path = marker_path(&db);
        assert!(read(&path).unwrap().is_none());
        write(&path, "device-abc_123").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some("device-abc_123"));
        write(&path, "device-xyz").unwrap();
        assert_eq!(read(&path).unwrap().as_deref(), Some("device-xyz"));
        assert!(write(&path, "bad id").is_err());
        let _ = fs::remove_dir_all(&dir);
    }
}
