//! Cooperative daemon shutdown signal used by the desktop process supervisor.
use std::{
    fs, io,
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    thread,
};

pub(super) fn stop_flag(watch_stdin: bool) -> io::Result<Arc<AtomicBool>> {
    let stop = Arc::new(AtomicBool::new(false));
    if watch_stdin {
        let stop = Arc::clone(&stop);
        thread::Builder::new()
            .name("desktop-lifetime".into())
            .spawn(move || {
                // Only the desktop holds the write end. Blocking here avoids
                // PID reuse, platform-specific process polling and wakeups.
                if let Err(error) = io::copy(&mut io::stdin().lock(), &mut io::sink()) {
                    // An unusable pipe can no longer prove the owner is alive.
                    if error.kind() != io::ErrorKind::BrokenPipe {
                        eprintln!("TokenScope desktop lifetime pipe closed: {error}");
                    }
                }
                stop.store(true, Ordering::Relaxed);
            })?;
    }
    Ok(stop)
}

/// Consume a shutdown marker exactly once. Missing paths are normal for CLI use.
pub fn consume(path: Option<&Path>) -> bool {
    let Some(path) = path else {
        return false;
    };
    match fs::remove_file(path) {
        Ok(()) => true,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::consume;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_path() -> PathBuf {
        let n = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("tokenscope-shutdown-{n}.signal"))
    }

    #[test]
    fn missing_marker_is_not_a_shutdown_request() {
        assert!(!consume(None));
        assert!(!consume(Some(std::path::Path::new(
            "/definitely/not/a/tokenscope-marker"
        ))));
    }

    #[test]
    fn marker_is_consumed_once() {
        let path = temp_path();
        fs::write(&path, b"shutdown").unwrap();
        assert!(consume(Some(&path)));
        assert!(!path.exists());
        assert!(!consume(Some(&path)));
    }
}
