use std::{
    fs::OpenOptions,
    io,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct RunningChild(Child);

impl Drop for RunningChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_until<T>(mut check: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(result) = check() {
            return result;
        }
        assert!(Instant::now() < deadline, "timed out waiting for Agent");
        thread::sleep(Duration::from_millis(25));
    }
}

fn start_agent(root: &Path, supervised: bool) -> RunningChild {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tokenscope-agent"));
    command
        .args(["--daemon", "--interval-seconds", "120", "--sync", "--db"])
        .arg(root.join("usage.db"))
        .arg("--sync-config")
        .arg(root.join("absent-hub-config"))
        .arg("--shutdown-file")
        .arg(root.join("stop"));
    // An absent sync config exercises the real daemon without touching
    // live source logs, fetching prices, or making a network request.
    if supervised {
        command.arg("--shutdown-on-stdin-eof").stdin(Stdio::piped());
    } else {
        command.stdin(Stdio::null());
    }
    let mut child = RunningChild(command.spawn().unwrap());
    wait_until(|| {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "Agent exited during startup"
        );
        // The daemon takes the lock before creating its SQLite database.
        if !root.join("usage.db").is_file() {
            return None;
        }
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .open(root.join("usage.db.lock"));
        match lock.and_then(|file| fs2::FileExt::try_lock_exclusive(&file)) {
            Ok(()) => None,
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err)
                if err.kind() == io::ErrorKind::WouldBlock || err.raw_os_error() == Some(33) =>
            {
                Some(())
            }
            Err(err) => panic!("could not check Agent lock: {err}"),
        }
    });
    child
}

#[test]
fn desktop_pipe_eof_releases_database_for_next_agent() {
    let root = tempfile::tempdir().unwrap();
    let mut agent = start_agent(root.path(), true);
    drop(agent.0.stdin.take().unwrap());
    let status = wait_until(|| agent.0.try_wait().unwrap());
    assert!(
        status.success(),
        "Agent did not shut down cleanly: {status}"
    );
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.path().join("usage.db.lock"))
        .unwrap();
    fs2::FileExt::try_lock_exclusive(&lock).unwrap();
    let store =
        tokenscope_local_store::LocalStore::open_readonly(&root.path().join("usage.db")).unwrap();
    store.quick_integrity_check().unwrap();
}

#[test]
fn standalone_daemon_accepts_closed_stdin_and_stops_by_marker() {
    let root = tempfile::tempdir().unwrap();
    let mut agent = start_agent(root.path(), false);
    std::fs::write(root.path().join("stop"), b"shutdown").unwrap();
    assert!(wait_until(|| agent.0.try_wait().unwrap()).success());
    assert!(!root.path().join("stop").exists());
}
