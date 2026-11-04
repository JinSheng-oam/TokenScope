use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
};

use tokenscope_core_domain::RevisionedRecord;
use tokenscope_local_store::LocalStore;
use tokenscope_sync_client::webdav::{
    sync_webdav_cycle, WebDavBatchFile, WebDavClient, WebDavCredentials, WebDavDeviceInfo,
    WebDavUrl,
};
use zeroize::Zeroizing;

#[test]
fn webdav_sync_cycle_roundtrip_with_mock_server() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx_done, rx_done) = mpsc::channel();

    // Prepare a valid remote record
    let fixture = include_str!("../../../fixtures/domain/canonical-record-v1.json");
    let mut canonical: RevisionedRecord = serde_json::from_str(fixture).unwrap();
    canonical.record.owner_device_id = "dev-remote".into();
    let valid_remote_record =
        RevisionedRecord::from_store(canonical.record, 1).unwrap();

    let batch_file = WebDavBatchFile {
        protocol_version: 1,
        batch_id: "batch-1".into(),
        device_id: "dev-remote".into(),
        batch_seq: 1,
        records: vec![valid_remote_record],
    };
    let batch_json = serde_json::to_string(&batch_file).unwrap();

    thread::spawn(move || {
        // Handle mock requests from the client
        for stream in listener.incoming() {
            let mut stream = match stream {
                Ok(s) => s,
                Err(_) => break,
            };
            let mut buf = [0u8; 4096];
            let n = stream.read(&mut buf).unwrap_or(0);
            let req = String::from_utf8_lossy(&buf[..n]);

            if req.starts_with("MKCOL") {
                let _ = stream.write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 0\r\n\r\n");
            } else if req.starts_with("PUT") {
                let _ = stream.write_all(b"HTTP/1.1 204 No Content\r\nContent-Length: 0\r\n\r\n");
            } else if req.starts_with("GET /dav/sync/devices/dev-local.json")
                || req.starts_with("GET /dav/sync/data/dev-local/manifest.json")
            {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            } else if req.starts_with("PROPFIND /dav/sync/devices/") {
                let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/sync/devices/</D:href>
    <D:propstat><D:prop><D:resourcetype><D:collection/></D:resourcetype></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/sync/devices/dev-remote.json</D:href>
    <D:propstat><D:prop><D:resourcetype/></D:prop><D:status>HTTP/1.1 200 OK</D:status></D:propstat>
  </D:response>
</D:multistatus>"#;
                let resp = format!(
                    "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml\r\nContent-Length: {}\r\n\r\n{}",
                    xml.len(),
                    xml
                );
                let _ = stream.write_all(resp.as_bytes());
            } else if req.starts_with("GET /dav/sync/devices/dev-remote.json") {
                let info = r#"{
  "device_id": "dev-remote",
  "display_name": "MacBook Pro",
  "os_name": "macos",
  "app_version": "0.1.0",
  "time_zone": "Asia/Shanghai",
  "created_at_ms": 1000,
  "last_seen_ms": 2000
}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    info.len(),
                    info
                );
                let _ = stream.write_all(resp.as_bytes());
            } else if req.starts_with("GET /dav/sync/data/dev-remote/manifest.json") {
                let manifest = r#"{
  "protocol_version": 1,
  "device_id": "dev-remote",
  "last_batch_seq": 1,
  "updated_at_ms": 2000,
  "batches": [
    {
      "batch_seq": 1,
      "batch_id": "batch-1",
      "count": 1,
      "file_name": "batch_000001.json",
      "created_at_ms": 2000
    }
  ]
}"#;
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    manifest.len(),
                    manifest
                );
                let _ = stream.write_all(resp.as_bytes());
            } else if req.starts_with("GET /dav/sync/data/dev-remote/batch_000001.json") {
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    batch_json.len(),
                    batch_json
                );
                let _ = stream.write_all(resp.as_bytes());
            } else {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n");
            }

            if rx_done.try_recv().is_ok() {
                break;
            }
        }
    });

    let mut store = LocalStore::open_in_memory().unwrap();
    store
        .init_identity(&tokenscope_local_store::LocalIdentity {
            device_id: "dev-local".into(),
            display_name: "Desktop-PC".into(),
            time_zone: "UTC".into(),
        })
        .unwrap();

    let raw_url = format!("http://127.0.0.1:{port}/dav/sync");
    let url = WebDavUrl::parse(&raw_url).unwrap();
    let credentials = WebDavCredentials {
        username: "testuser".into(),
        password: Zeroizing::new("testpass".into()),
    };
    let client = WebDavClient::new(url, credentials).unwrap();

    let dev_info = WebDavDeviceInfo {
        device_id: "dev-local".into(),
        display_name: "Desktop-PC".into(),
        os_name: "windows".into(),
        app_version: "0.1.0".into(),
        time_zone: "UTC".into(),
        created_at_ms: 1000,
        last_seen_ms: 1000,
    };

    let (report, devices) = sync_webdav_cycle(&client, &mut store, &dev_info).unwrap();
    assert_eq!(report.pulled_records, 1);
    assert_eq!(report.pulled_batches, 1);
    assert_eq!(devices.len(), 1);
    assert_eq!(devices[0].device_id, "dev-remote");
    assert_eq!(devices[0].display_name, "MacBook Pro");

    // Remote record should now be in local store!
    let totals = store.remote_usage_totals(Some("dev-remote"), None, None).unwrap();
    assert_eq!(totals.record_count, 1);
    assert_eq!(totals.input_tokens, 100);
    assert_eq!(totals.output_tokens, 50);

    let _ = tx_done.send(());
}
