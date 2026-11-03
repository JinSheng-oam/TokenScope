use super::endpoint::Endpoint;
use super::error::RpcError;
use super::process::{classify_process, ProcessKind};
use super::quota::{fraction, local_source_id, parse_grouped_quota, parse_legacy_quota};
use super::transport::{build_client, call_rpc};
use serde_json::json;

#[test]
fn shell_tokenizer_preserves_quoted_windows_paths() {
    let line = r#""C:\Program Files\Antigravity\language_server.exe" --app_data_dir antigravity --csrf_token win-token"#;
    let p = classify_process(&format!("7777 {line}")).unwrap();
    assert_eq!(p.pid, 7777);
    assert_eq!(p.csrf, "win-token");
}

#[test]
fn parses_antigravity_app_process_and_never_requires_token_in_output() {
    let line = "53602 /Applications/Antigravity.app/Contents/Resources/bin/language_server --csrf_token secret-value --app_data_dir antigravity --extension_server_port 12345 --extension_server_csrf_token ext-secret";
    let p = classify_process(line).unwrap();
    assert_eq!(p.pid, 53602);
    assert_eq!(p.kind, ProcessKind::App);
    assert_eq!(p.csrf, "secret-value");
    assert_eq!(p.extension_port, Some(12345));
}

#[test]
fn rejects_unrelated_and_tokenless_desktop_processes() {
    assert!(classify_process("123 /bin/bash --login").is_none());
    assert!(classify_process("53602 /Applications/Antigravity.app/Contents/Resources/bin/language_server --app_data_dir antigravity").is_none());
}

#[test]
fn allows_standalone_cli_without_csrf_but_not_hub_mode() {
    let cli =
        classify_process("60123 /Users/x/.antigravity/bin/agy language-server --stdio").unwrap();
    assert_eq!(cli.kind, ProcessKind::Cli);
    assert!(cli.csrf.is_empty());
    assert!(classify_process(
        "60123 /Users/x/.antigravity/bin/agy --hub --hub-port=55555 --app_data_dir=antigravity"
    )
    .is_none());
}

#[test]
fn parses_grouped_windows_without_confusing_zero_and_unknown() {
    let payload = json!({"groups":[
        {"displayName":"Gemini Models","buckets":[
            {"window":"5h","remainingFraction":0.0,"resetTime":"2026-11-02T12:00:00Z"},
            {"window":"weekly","remainingFraction":1.0}
        ]},
        {"displayName":"Claude and GPT models","buckets":[{"window":"weekly","disabled":true,"remainingFraction":1.0}]},
        {"displayName":"Gemini 4 Argon","buckets":[{"window":"weekly","remainingFraction":0.5}]}
    ]});
    let rows = parse_grouped_quota(&payload, "local_anonymous", 1_800_000_000_000);
    assert_eq!(rows.len(), 4);
    let gemini_5h = rows
        .iter()
        .find(|r| r.pool_id == "gemini" && r.window_id == "five_hour")
        .unwrap();
    let gemini_weekly = rows
        .iter()
        .find(|r| r.pool_id == "gemini" && r.window_id == "weekly")
        .unwrap();
    let claude_weekly = rows.iter().find(|r| r.pool_id == "claude_gpt").unwrap();
    let argon_weekly = rows.iter().find(|r| r.pool_id == "gemini_4_argon").unwrap();
    assert_eq!(gemini_5h.remaining_basis_points, Some(0));
    assert_eq!(gemini_weekly.remaining_basis_points, Some(10000));
    assert_eq!(claude_weekly.remaining_basis_points, None);
    assert_eq!(argon_weekly.window_id, "weekly");
}

#[test]
fn grouped_duplicate_pool_keeps_lowest_remaining_quota() {
    let payload = json!({"groups":[
        {"displayName":"Gemini Models","buckets":[{"window":"weekly","remainingFraction":0.8}]},
        {"displayName":"Gemini Models","buckets":[{"window":"weekly","remainingFraction":0.4}]}
    ]});
    let rows = parse_grouped_quota(&payload, "local_antigravity", 1_800_000_000_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].remaining_basis_points, Some(4000));
}

#[test]
fn unknown_group_keeps_a_safe_slug_and_invalid_fraction_is_skipped() {
    let payload = json!({"groups":[
        {"displayName":"Future Provider Plus","buckets":[{"window":"weekly","remainingFraction":0.5}]},
        {"displayName":"Gemini Models","buckets":[{"window":"weekly","remainingFraction":1.5}]}
    ]});
    let rows = parse_grouped_quota(&payload, "local_antigravity", 1_800_000_000_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].pool_id, "future_provider_plus");
}

#[test]
fn parses_legacy_model_config_as_other_window_without_inventing_period() {
    let payload = json!({"userStatus":{"cascadeModelConfigData":{"clientModelConfigs":[
        {"label":"Gemini Pro","modelOrAlias":{"model":"gemini-pro"},"quotaInfo":{"remainingFraction":0.42,"resetTime":"2026-11-03T00:00:00Z"}},
        {"label":"Placeholder","modelOrAlias":{"model":"MODEL_PLACEHOLDER_M9"},"quotaInfo":{"remainingFraction":1.0}}
    ]}}});
    let rows = parse_legacy_quota(&payload, "local_anonymous", 1_800_000_000_000);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].window_id, "other");
    assert_eq!(rows[0].remaining_basis_points, Some(4200));
}

#[test]
fn quota_source_identity_is_device_local_and_contains_no_account_data() {
    assert_eq!(local_source_id(), "local_antigravity");
    assert!(!local_source_id().contains('@'));
}

#[test]
fn rejects_fraction_outside_range() {
    assert_eq!(fraction(&json!(1.2)), None);
    assert_eq!(fraction(&json!(-0.1)), None);
}

#[test]
fn local_rpc_does_not_follow_redirects() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let redirect_target = TcpListener::bind("127.0.0.1:0").unwrap();
    let redirect_port = redirect_target.local_addr().unwrap().port();
    redirect_target.set_nonblocking(true).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 2048];
        let _ = stream.read(&mut request).unwrap();
        write!(stream, "HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:{redirect_port}/steal\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
    });
    let client = build_client().unwrap();
    let result = call_rpc(
        &client,
        &Endpoint {
            port,
            csrf: String::new(),
        },
        "http",
        "GetUserStatus",
        &json!({}),
    );
    assert!(matches!(result, Err(RpcError::Network)));
    server.join().unwrap();
    assert!(
        redirect_target.accept().is_err(),
        "redirect target must never be contacted"
    );
}

#[test]
fn local_rpc_posts_to_loopback_with_csrf_and_parses_json() {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = [0_u8; 4096];
        let n = stream.read(&mut request).unwrap();
        let text = String::from_utf8_lossy(&request[..n]);
        assert!(text.starts_with(
            "POST /exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary HTTP/1.1"
        ));
        assert!(text
            .to_ascii_lowercase()
            .contains("x-codeium-csrf-token: test-csrf"));
        let body = r#"{"groups":[]}"#;
        write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body).unwrap();
    });
    let client = build_client().unwrap();
    let response = call_rpc(
        &client,
        &Endpoint {
            port,
            csrf: "test-csrf".into(),
        },
        "http",
        "RetrieveUserQuotaSummary",
        &json!({"forceRefresh":true}),
    )
    .unwrap();
    assert_eq!(response["groups"].as_array().unwrap().len(), 0);
    server.join().unwrap();
}
