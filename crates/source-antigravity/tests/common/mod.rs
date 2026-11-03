#![allow(dead_code)]

use rusqlite::{params, Connection};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use tokenscope_local_store::{LocalIdentity, LocalStore};

pub struct Fixture {
    pub root: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tokenscope-ag-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }

    pub fn file(&self, dir: &str, stem: &str) -> PathBuf {
        let dir = self.root.join(dir).join("conversations");
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{stem}.db"))
    }

    pub fn write(&self, dir: &str, stem: &str, models: &[Option<&str>]) -> PathBuf {
        let path = self.file(dir, stem);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS gen_metadata(data BLOB NOT NULL); DELETE FROM gen_metadata;",
        )
        .unwrap();
        for model in models {
            let data = model.map(blob).unwrap_or_else(|| vec![0x08, 0x01]);
            conn.execute("INSERT INTO gen_metadata(data) VALUES(?1)", params![data])
                .unwrap();
        }
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn varint(mut n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(byte);
            break;
        }
        out.push(byte | 0x80);
    }
    out
}

pub fn field(num: usize, bytes: &[u8]) -> Vec<u8> {
    let mut out = varint(num * 8 + 2);
    out.extend(varint(bytes.len()));
    out.extend(bytes);
    out
}

pub fn blob(model: &str) -> Vec<u8> {
    field(1, &field(19, model.as_bytes()))
}

pub fn scalar(num: usize, value: usize) -> Vec<u8> {
    let mut out = varint(num * 8);
    out.extend(varint(value));
    out
}

pub fn usage_blob(model: &str, response_id: &str, input: usize) -> Vec<u8> {
    let mut usage = Vec::new();
    usage.extend(scalar(1, 1132));
    usage.extend(scalar(2, input));
    usage.extend(scalar(3, 340));
    usage.extend(scalar(5, 1200));
    usage.extend(scalar(9, 300));
    usage.extend(scalar(10, 40));
    usage.extend(field(11, response_id.as_bytes()));
    let mut chat_model = field(4, &usage);
    chat_model.extend(field(19, model.as_bytes()));
    let mut generation = field(1, &chat_model);
    generation.extend(scalar(2, 7));
    generation
}

pub fn store() -> LocalStore {
    let mut store = LocalStore::open_in_memory().unwrap();
    store
        .init_identity(&LocalIdentity {
            device_id: "test-device".into(),
            display_name: "Test".into(),
            time_zone: "UTC".into(),
        })
        .unwrap();
    store
}

pub fn active(s: &LocalStore) -> Vec<tokenscope_core_domain::CanonicalRecord> {
    s.pending_outbox(200)
        .unwrap()
        .into_iter()
        .map(|row| row.envelope.record)
        .filter(|record| !record.is_deleted)
        .collect()
}
