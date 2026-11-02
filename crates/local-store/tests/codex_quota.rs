use tokenscope_local_store::{CodexQuota, LocalIdentity, LocalStore};

fn store() -> LocalStore {
    let mut store = LocalStore::open_in_memory().unwrap();
    store
        .init_identity(&LocalIdentity {
            device_id: "device-a".into(),
            display_name: "Office PC".into(),
            time_zone: "UTC".into(),
        })
        .unwrap();
    store
}

fn quota(duration: u32, remaining: Option<u16>, observed: i64) -> CodexQuota {
    CodexQuota {
        source_id: "local_codex".into(),
        pool_id: "codex".into(),
        window_duration_minutes: duration,
        remaining_basis_points: remaining,
        reset_at_ms: Some(2_000),
        observed_at_ms: observed,
    }
}

#[test]
fn stores_zero_and_unknown_quota_separately_and_never_creates_usage() {
    let mut store = store();
    store
        .upsert_codex_quotas(
            &[quota(300, Some(0), 1_000), quota(10_080, None, 1_000)],
            1_000,
        )
        .unwrap();
    let rows = store.codex_quota_snapshots(32).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].remaining_basis_points, Some(0));
    assert_eq!(rows[1].remaining_basis_points, None);
    assert!(store.pending_outbox(10).unwrap().is_empty());
}

#[test]
fn old_observation_cannot_replace_new_codex_quota() {
    let mut store = store();
    assert_eq!(
        store
            .upsert_codex_quotas(&[quota(300, Some(7_500), 2_000)], 2_000)
            .unwrap(),
        1
    );
    assert_eq!(
        store
            .upsert_codex_quotas(&[quota(300, Some(9_000), 1_000)], 2_000)
            .unwrap(),
        0
    );
    assert_eq!(
        store.codex_quota_snapshots(32).unwrap()[0].remaining_basis_points,
        Some(7_500)
    );
}

#[test]
fn rejects_duplicate_windows_and_invalid_percentages() {
    let mut store = store();
    assert!(store
        .upsert_codex_quotas(&[quota(300, Some(1), 1), quota(300, Some(2), 1)], 1)
        .is_err());
    assert!(store
        .upsert_codex_quotas(&[quota(300, Some(10_001), 1)], 1)
        .is_err());
}
