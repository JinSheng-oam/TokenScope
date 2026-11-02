use rusqlite::{params, Connection};
use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};
use tokenscope_core_domain::{aggregate_device_local, Precision};
use tokenscope_local_store::{LocalIdentity, LocalStore};
use tokenscope_source_antigravity::{
    collect_antigravity, discover_databases, extract_model_name,
};

struct Fixture { root: PathBuf }
impl Fixture {
    fn new() -> Self {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("tokenscope-ag-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        Self { root }
    }
    fn file(&self, dir: &str, stem: &str) -> PathBuf {
        let dir = self.root.join(dir).join("conversations");
        fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{stem}.db"))
    }
    fn write(&self, dir: &str, stem: &str, models: &[Option<&str>]) -> PathBuf {
        let path = self.file(dir, stem);
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE IF NOT EXISTS gen_metadata(data BLOB NOT NULL); DELETE FROM gen_metadata;").unwrap();
        for model in models {
            let b = model.map(blob).unwrap_or_else(|| vec![0x08, 0x01]);
            conn.execute("INSERT INTO gen_metadata(data) VALUES(?1)", params![b]).unwrap();
        }
        path
    }
}
impl Drop for Fixture { fn drop(&mut self) { let _ = fs::remove_dir_all(&self.root); } }
fn varint(mut n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let byte = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 { out.push(byte); break; }
        out.push(byte | 0x80);
    }
    out
}
fn field(num: usize, bytes: &[u8]) -> Vec<u8> {
    let mut out = varint(num * 8 + 2);
    out.extend(varint(bytes.len()));
    out.extend(bytes);
    out
}
fn blob(model: &str) -> Vec<u8> { field(1, &field(19, model.as_bytes())) }
fn store() -> LocalStore {
    let mut s = LocalStore::open_in_memory().unwrap();
    s.init_identity(&LocalIdentity { device_id:"test-device".into(), display_name:"Test".into(), time_zone:"UTC".into() }).unwrap();
    s
}
fn active(s: &LocalStore) -> Vec<tokenscope_core_domain::CanonicalRecord> {
    s.pending_outbox(200).unwrap().into_iter()
        .map(|r|r.envelope.record).filter(|r|!r.is_deleted).collect()
}
#[test]
fn protobuf_extracts_model_only() {
    assert_eq!(extract_model_name(&blob("gemini-3-flash-a")).unwrap().unwrap(),"gemini-3-flash-a");
    assert_eq!(extract_model_name(&[0x08,0x01]).unwrap(),None);
}
#[test]
fn truncated_protobuf_errors() {
    assert!(extract_model_name(&[0x0a,0xff]).is_err());
    assert!(extract_model_name(&[0x0a,0x02,0x9a]).is_err());
}
#[test]
fn protobuf_rejects_bad_model_name() {
    assert!(extract_model_name(&blob("secret private prompt!")).is_err());
}
#[test]
fn empty_root_does_not_modify_db() {
    let f=Fixture::new();let mut s=store();
    let report=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(report.source_files,0);
    assert!(active(&s).is_empty());
}
#[test]
fn discovered_db_does_not_follow_symlink() {
    let f=Fixture::new();f.write("antigravity","a",&[Some("gemini-pro")]);
    assert_eq!(discover_databases(&f.root).unwrap().len(),1);
}
#[test]
fn activity_only_is_default_and_never_persists_fake_tokens() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("gemini-pro"),Some("gemini-pro")]);
    let mut s=store();
    let r=collect_antigravity(&f.root,&mut s,"test-device",false).unwrap();
    assert_eq!(r.model_turns,2);
    assert_eq!(r.estimated_records,0);
    assert!(active(&s).is_empty());
}
#[test]
fn opt_in_creates_only_estimated_rows_without_fake_dates_or_cost() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("gemini-3-flash-a"),Some("gemini-3-flash-a")]);
    let mut s=store();
    let r=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(r.inserted,1);
    let records=active(&s);
    assert_eq!(records.len(),1);
    assert_eq!(records[0].precision,Precision::Estimated);
    assert_eq!(records[0].usage.unclassified_tokens,8000);
    assert_eq!(records[0].usage.output_tokens,1000);
    assert_eq!(records[0].event_at_ms,None);
    assert_eq!(records[0].estimated_cost_microusd,None);
}
#[test]
fn repeated_scans_are_idempotent() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("claude-sonnet-4")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    let again=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!((again.inserted,again.revised,again.unchanged),(0,0,1));
    assert_eq!(active(&s).len(),1);
}
#[test]
fn same_session_in_ide_and_cli_is_counted_once() {
    let f=Fixture::new();f.write("antigravity","same",&[Some("gemini-pro")]);
    f.write("antigravity-cli","same",&[Some("gemini-pro")]);
    let mut s=store();
    let report=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!((report.source_files,report.conversations,report.model_turns),(2,1,1));
    assert_eq!(active(&s).len(),1);
}
#[test]
fn larger_snapshot_preferred_over_older_cli_copy() {
    let f=Fixture::new();f.write("antigravity","same",&[Some("gemini-pro"),Some("gemini-pro")]);
    f.write("antigravity-cli","same",&[Some("gemini-pro")]);
    let mut s=store();
    let report=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(report.model_turns,2);
    assert_eq!(active(&s)[0].usage.unclassified_tokens,16000);
}
#[test]
fn divergent_duplicate_sources_are_not_summed() {
    let f=Fixture::new();f.write("antigravity","same",&[Some("gemini-pro")]);
    f.write("antigravity-cli","same",&[Some("claude-opus")]);
    let mut s=store();
    let r=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(r.ambiguous_conversations,1);
    assert!(active(&s).is_empty());
}
#[test]
fn append_same_model_revises_one_record() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    f.write("antigravity","s1",&[Some("gemini-pro"),Some("gemini-pro")]);
    let r=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(r.revised,1);
    assert_eq!(active(&s).len(),1);
}
#[test]
fn model_removed_creates_tombstone_not_lingering_estimate() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("gemini-pro"),Some("claude-opus")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    f.write("antigravity","s1",&[Some("gemini-pro")]);
    let r=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(r.tombstoned,1);
    assert_eq!(active(&s).len(),1);
}
#[test]
fn switching_estimation_off_tombstones_estimates() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    let r=collect_antigravity(&f.root,&mut s,"test-device",false).unwrap();
    assert_eq!(r.tombstoned,1);
    assert!(active(&s).is_empty());
}
#[test]
fn unknown_model_prevents_incomplete_estimate_and_preserves_history() {
    let f=Fixture::new();f.write("antigravity","s1",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    f.write("antigravity","s1",&[Some("gemini-pro"),None]);
    let r=collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(r.unknown_model_turns,1);
    assert_eq!(r.revised,0);
    assert_eq!(active(&s).len(),1);
}
#[test]
fn malformed_schema_never_advances_checkpoint_or_deletes_old_data() {
    let f=Fixture::new();let path=f.write("antigravity","s1",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    let conn=Connection::open(path).unwrap();
    conn.execute_batch("DROP TABLE gen_metadata").unwrap();
    assert!(collect_antigravity(&f.root,&mut s,"test-device",true).is_err());
    assert_eq!(active(&s).len(),1);
}
#[test]
fn aggregate_total_retains_estimated_precision_in_records() {
    let f=Fixture::new();f.write("antigravity","a",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    let rows=active(&s);
    assert_eq!(aggregate_device_local(rows.iter()).unwrap().usage.total().unwrap(),8800);
    assert!(rows.iter().all(|r|r.precision==Precision::Estimated));
}
#[test]
fn raw_metadata_does_not_leak_to_storage() {
    let f=Fixture::new();let path=f.write("antigravity","s1",&[Some("gemini-pro")]);
    let conn=Connection::open(&path).unwrap();
    conn.execute("INSERT INTO gen_metadata(data) VALUES(?1)",params![vec![0x0a_u8,0x00_u8]]).unwrap();
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",false).unwrap();
    assert!(active(&s).is_empty());
}
#[test]
fn too_large_blob_does_not_replace_last_good_record() {
    let f=Fixture::new();let path=f.write("antigravity","s1",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    Connection::open(path).unwrap().execute(
        "INSERT INTO gen_metadata(data) VALUES(?1)",params![vec![0u8; 300_000]]).unwrap();
    assert!(collect_antigravity(&f.root,&mut s,"test-device",true).is_err());
    assert_eq!(active(&s).len(),1);
}
#[test]
fn missing_source_does_not_delete_last_known_usage() {
    let f=Fixture::new();let path=f.write("antigravity","s1",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    fs::remove_file(path).unwrap();
    collect_antigravity(&f.root,&mut s,"test-device",true).unwrap();
    assert_eq!(active(&s).len(),1);
}

#[test]
fn default_activity_is_persisted_without_token_rows() {
    let f=Fixture::new(); f.write("antigravity","persist",&[Some("gemini-pro"),Some("claude-sonnet"),None]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",false).unwrap();
    let summary=s.antigravity_activity_summary().unwrap();
    assert_eq!((summary.conversations,summary.known_turns,summary.unknown_turns),(1,2,1));
    assert_eq!(summary.by_model.len(),2);
    assert!(active(&s).is_empty());
}
#[test]
fn default_activity_scan_is_idempotent() {
    let f=Fixture::new(); f.write("antigravity","persist",&[Some("gemini-pro")]);
    let mut s=store();
    collect_antigravity(&f.root,&mut s,"test-device",false).unwrap();
    collect_antigravity(&f.root,&mut s,"test-device",false).unwrap();
    assert_eq!(s.antigravity_activity_summary().unwrap().known_turns,1);
}
