//! Cooperative daemon shutdown signal used by the desktop process supervisor.
use std::{fs, path::Path};

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
