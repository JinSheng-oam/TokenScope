//! The advisory OS lock is held for the entire lifetime of the collector.
//! A crash releases it automatically (unlike PID-only lock files).
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

pub struct AgentLock {
    #[allow(dead_code)]
    file: File,
}

impl AgentLock {
    pub fn acquire(db: &Path) -> io::Result<Self> {
        // Canonicalize the parent; relative aliases must resolve to the same
        // lock. Existing symlinked database files resolve to the same target.
        let absolute = if db.exists() {
            db.canonicalize()?
        } else {
            let parent = db.parent().filter(|p| !p.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            let name = db.file_name().ok_or_else(|| io::Error::new(
                io::ErrorKind::InvalidInput, "database must include a filename"
            ))?;
            parent.canonicalize()?.join(name)
        };
        let mut lock_name = absolute.as_os_str().to_os_string();
        lock_name.push(".lock");
        let lock_path = PathBuf::from(lock_name);
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let file = options.open(lock_path)?;
        file.try_lock_exclusive().map_err(|err| {
            let is_blocked = err.kind() == io::ErrorKind::WouldBlock
                || (cfg!(windows) && err.raw_os_error() == Some(33));
            if is_blocked {
                io::Error::new(io::ErrorKind::WouldBlock, "another TokenScope agent owns this database")
            } else { err }
        })?;
        Ok(Self { file })
    }
}

impl Drop for AgentLock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.file);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn same_database_rejects_second_owner_and_releases_on_drop() {
        let dir = std::env::temp_dir().join(format!("tokenscope-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("lock-test.db");
        let lock = AgentLock::acquire(&db).unwrap();
        let err = AgentLock::acquire(&db).err().expect("second process/file handle must fail");
        assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
        drop(lock);
        let lock = AgentLock::acquire(&db).unwrap();
        drop(lock);
        let _ = std::fs::remove_file(dir.join("lock-test.db.lock"));
        let _ = std::fs::remove_dir(dir);
    }
}
