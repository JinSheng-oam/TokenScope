use tokenscope_core_domain::{
    account_key, aggregate_device_local, payload_hash, project_key, session_key,
    stable_record_id, CanonicalRecord, DomainError, Precision, RevisionedRecord,
    TokenBreakdown, UsageScope,
};

fn sample() -> CanonicalRecord {
    CanonicalRecord {
        record_id: stable_record_id("claude-code", b"source-a", b"response-42").unwrap(),
        owner_device_id: "device-01".to_string(),
        tool_id: "claude-code".to_string(),
        scope: UsageScope::DeviceLocal,
        precision: Precision::Exact,
        account_id: Some(account_key("claude-code", b"user-5").unwrap()),
        model_id: Some("claude-sonnet".to_string()),
        project_id: Some(project_key("claude-code", b"/repo/secret").unwrap()),
        session_id: Some(session_key("claude-code", b"session-5").unwrap()),
        event_at_ms: Some(1_700_000_000_000),
        usage: TokenBreakdown {
            input_uncached_tokens: 100,
            cache_read_tokens: 20,
            cache_write_tokens: 10,
            output_tokens: 50,
            unclassified_tokens: 0,
            reasoning_tokens: Some(15),
        },
        estimated_cost_microusd: Some(125),
        price_version: Some("2026-11".to_string()),
        is_deleted: false,
    }
}

#[test] fn contract_module_exposes_canonical_model() { assert_eq!(tokenscope_core_domain::MODULE_ID, "core-domain"); }
#[test] fn five_disjoint_buckets_sum_to_180() { assert_eq!(sample().usage.total().unwrap(), 180); }
#[test] fn reasoning_is_not_double_counted() { let mut r=sample(); r.usage.reasoning_tokens=Some(50); assert_eq!(r.usage.total().unwrap(),180); }
#[test] fn unknown_breakdown_is_unclassified() { let x=TokenBreakdown::aggregate_only(1000); assert_eq!(x.unclassified_tokens,1000); assert_eq!(x.total().unwrap(),1000); }
#[test] fn zero_usage_is_valid() { assert_eq!(TokenBreakdown::default().total(), Ok(0)); }
#[test] fn overflowing_usage_is_rejected() { let x=TokenBreakdown{input_uncached_tokens:u64::MAX,output_tokens:1,..Default::default()}; assert_eq!(x.total(),Err(DomainError::TokenOverflow)); }
#[test] fn reasoning_greater_than_output_rejected() { let x=TokenBreakdown{output_tokens:50,reasoning_tokens:Some(60),..Default::default()}; assert_eq!(x.validate(),Err(DomainError::ReasoningExceedsOutput)); }
#[test] fn reasoning_equal_to_output_is_valid() { let x=TokenBreakdown{output_tokens:60,reasoning_tokens:Some(60),..Default::default()}; assert!(x.validate().is_ok()); }
#[test] fn checked_add_usage() { let a=TokenBreakdown{output_tokens:5,reasoning_tokens:Some(2),..Default::default()}; let b=TokenBreakdown{output_tokens:7,reasoning_tokens:Some(3),..Default::default()}; let x=a.checked_add(b).unwrap(); assert_eq!(x.output_tokens,12); assert_eq!(x.reasoning_tokens,Some(5)); }
#[test] fn checked_add_rejects_overflow() { let a=TokenBreakdown{output_tokens:u64::MAX,..Default::default()}; let b=TokenBreakdown{output_tokens:1,..Default::default()}; assert_eq!(a.checked_add(b),Err(DomainError::TokenOverflow)); }
#[test] fn unknown_reasoning_provenance_is_not_zero() { let a=TokenBreakdown{output_tokens:5,reasoning_tokens:Some(2),..Default::default()}; let b=TokenBreakdown{output_tokens:7,..Default::default()}; assert_eq!(a.checked_add(b).unwrap().reasoning_tokens,None); }
#[test] fn records_validate() { assert!(sample().validate().is_ok()); }
#[test] fn malformed_record_id_rejected() { let mut r=sample(); r.record_id="abc".into(); assert_eq!(r.validate(), Err(DomainError::InvalidField("record_id"))); }
#[test] fn uppercase_hash_rejected() { let mut r=sample(); r.record_id=r.record_id.to_uppercase(); assert!(r.validate().is_err()); }
#[test] fn empty_device_rejected() { let mut r=sample(); r.owner_device_id=" ".into(); assert!(r.validate().is_err()); }
#[test] fn non_ascii_device_rejected() { let mut r=sample(); r.owner_device_id="设备01".into(); assert!(r.validate().is_err()); }
#[test] fn spaces_in_device_rejected() { let mut r=sample(); r.owner_device_id="device 01".into(); assert!(r.validate().is_err()); }
#[test] fn invalid_tool_slug_rejected() { let mut r=sample(); r.tool_id="Claude Code".into(); assert!(r.validate().is_err()); }
#[test] fn raw_project_path_rejected() { let mut r=sample(); r.project_id=Some("/private/source".into()); assert_eq!(r.validate(),Err(DomainError::InvalidField("project_id"))); }
#[test] fn raw_account_email_rejected() { let mut r=sample(); r.account_id=Some("person@example.com".into()); assert_eq!(r.validate(),Err(DomainError::InvalidField("account_id"))); }
#[test] fn raw_session_title_rejected() { let mut r=sample(); r.session_id=Some("my private prompt".into()); assert!(r.validate().is_err()); }
#[test] fn oversized_model_rejected() { let mut r=sample(); r.model_id=Some("a".repeat(257)); assert_eq!(r.validate(),Err(DomainError::TooLong("model_id"))); }
#[test] fn control_in_model_rejected() { let mut r=sample(); r.model_id=Some("foo\nbar".into()); assert!(r.validate().is_err()); }
#[test] fn negative_cost_rejected() { let mut r=sample(); r.estimated_cost_microusd=Some(-1); assert_eq!(r.validate(),Err(DomainError::NegativeCost)); }
#[test] fn deleted_with_usage_rejected() { let mut r=sample(); r.is_deleted=true; assert_eq!(r.validate(),Err(DomainError::DeletedRecordContainsUsage)); }
#[test] fn deleted_with_zero_is_valid() { let mut r=sample(); r.is_deleted=true; r.usage=TokenBreakdown::default(); r.estimated_cost_microusd=None; assert!(r.validate().is_ok()); }
#[test] fn serde_scope_uses_snake_case() { assert_eq!(serde_json::to_string(&UsageScope::AccountGlobal).unwrap(),r#""account_global""#); }
#[test] fn serde_precision_uses_snake_case() { assert_eq!(serde_json::to_string(&Precision::Estimated).unwrap(),r#""estimated""#); }
#[test] fn device_aggregation_excludes_account_global() { let mut b=sample(); b.scope=UsageScope::AccountGlobal; let a=sample(); let list=[a,b]; let t=aggregate_device_local(&list).unwrap(); assert_eq!(t.usage.total().unwrap(),180); assert_eq!(t.record_count,1); }
#[test] fn device_aggregation_excludes_unknown() { let mut r=sample(); r.scope=UsageScope::Unknown; let list=[r]; assert_eq!(aggregate_device_local(&list).unwrap().record_count,0); }
#[test] fn device_aggregation_excludes_tombstones() { let mut r=sample(); r.is_deleted=true; r.usage=TokenBreakdown::default(); r.estimated_cost_microusd=None; let list=[r]; assert_eq!(aggregate_device_local(&list).unwrap().record_count,0); }
#[test] fn duplicate_record_rejected_in_aggregation() { let r=sample(); let list=[r.clone(),r]; assert_eq!(aggregate_device_local(&list),Err(DomainError::DuplicateRecordId)); }
#[test] fn same_record_id_different_device_is_distinct() { let a=sample(); let mut b=sample(); b.owner_device_id="device-02".into(); let list=[a,b]; assert_eq!(aggregate_device_local(&list).unwrap().record_count,2); }
#[test] fn partial_cost_coverage_is_recorded() { let a=sample(); let mut b=sample(); b.record_id=stable_record_id("claude-code",b"source-a",b"response-43").unwrap(); b.estimated_cost_microusd=None; let list=[a,b]; let t=aggregate_device_local(&list).unwrap(); assert_eq!((t.known_cost_microusd,t.priced_records,t.unpriced_records),(125,1,1)); }
#[test] fn zero_cost_is_known_free_not_unknown() { let mut a=sample(); a.estimated_cost_microusd=Some(0); let list=[a]; let t=aggregate_device_local(&list).unwrap(); assert_eq!((t.priced_records,t.unpriced_records),(1,0)); }
#[test] fn cost_sum_is_checked() { let mut a=sample(); a.estimated_cost_microusd=Some(i64::MAX); let mut b=sample(); b.record_id=stable_record_id("claude-code",b"src",b"different").unwrap(); b.estimated_cost_microusd=Some(1); let list=[a,b]; assert_eq!(aggregate_device_local(&list),Err(DomainError::CostOverflow)); }
#[test] fn record_id_replay_is_stable() { let a=stable_record_id("codex",b"src",b"record").unwrap(); let b=stable_record_id("codex",b"src",b"record").unwrap(); assert_eq!(a,b); assert_eq!(a.len(),64); }
#[test] fn identity_prefix_ambiguity_eliminated() { let a=stable_record_id("codex",b"ab",b"c").unwrap(); let b=stable_record_id("codex",b"a",b"bc").unwrap(); assert_ne!(a,b); }
#[test] fn different_source_changes_record_id() { assert_ne!(stable_record_id("codex",b"a",b"id"),stable_record_id("codex",b"b",b"id")); }
#[test] fn different_native_identity_changes_record_id() { assert_ne!(stable_record_id("codex",b"a",b"id-a"),stable_record_id("codex",b"a",b"id-b")); }
#[test] fn identity_disallows_empty_components() { assert_eq!(stable_record_id("codex",b"",b"id"),Err(DomainError::MissingField("source_identity"))); }
#[test] fn identity_disallows_large_components() { assert_eq!(stable_record_id("codex",&vec![b'a';16385],b"id"),Err(DomainError::TooLong("source_identity"))); }
#[test] fn hashed_opaque_keys_hide_native_names() { let p=project_key("codex",b"/home/user/secret").unwrap(); let a=account_key("codex",b"private-email@example.com").unwrap(); let s=session_key("codex",b"private-title").unwrap(); assert_eq!(p.len(),68); assert_eq!(a.len(),69); assert_eq!(s.len(),69); assert!(!p.contains("secret")); assert!(!a.contains('@')); }
#[test] fn identity_domains_are_separated() { let x=b"same"; assert_ne!(account_key("codex",x).unwrap(),session_key("codex",x).unwrap()); }
#[test] fn same_record_hash_is_stable() { let a=sample(); assert_eq!(payload_hash(&a).unwrap(),payload_hash(&a).unwrap()); }
#[test] fn revised_cost_changes_hash() { let a=sample(); let mut b=a.clone(); b.estimated_cost_microusd=Some(126); assert_ne!(payload_hash(&a).unwrap(),payload_hash(&b).unwrap()); }
#[test] fn revised_timestamp_changes_hash() { let a=sample(); let mut b=a.clone(); b.event_at_ms=None; assert_ne!(payload_hash(&a).unwrap(),payload_hash(&b).unwrap()); }
#[test] fn nfc_and_nfd_metadata_hash_equal() { let mut a=sample(); a.model_id=Some("caf\u{e9}".into()); let mut b=sample(); b.model_id=Some("cafe\u{301}".into()); assert_eq!(payload_hash(&a).unwrap(),payload_hash(&b).unwrap()); assert_eq!(b.normalized().unwrap().model_id,Some("caf\u{e9}".into())); }
#[test] fn record_validation_rejects_blank_price_version() { let mut r=sample(); r.price_version=Some(" ".into()); assert!(r.validate().is_err()); }
#[test] fn revision_zero_rejected() { assert_eq!(RevisionedRecord::from_store(sample(),0),Err(DomainError::RevisionMustBePositive)); }
#[test] fn revisioned_record_roundtrip() { let a=RevisionedRecord::from_store(sample(),4).unwrap(); let json=serde_json::to_string(&a).unwrap(); let b:RevisionedRecord=serde_json::from_str(&json).unwrap(); assert_eq!(a,b); assert!(b.validate().is_ok()); }
#[test] fn tampered_payload_hash_rejected() { let mut a=RevisionedRecord::from_store(sample(),4).unwrap(); a.record.usage.output_tokens=51; assert_eq!(a.validate(),Err(DomainError::PayloadHashMismatch)); }
#[test] fn wire_payload_contains_no_observation_time() { let json=serde_json::to_string(&sample()).unwrap(); assert!(!json.contains("observed_at")); assert!(!json.contains("source_path")); assert!(!json.contains("prompt")); }
#[test] fn version_changes_do_not_change_payload_hash() { let a=RevisionedRecord::from_store(sample(),1).unwrap(); let b=RevisionedRecord::from_store(sample(),100).unwrap(); assert_eq!(a.payload_hash,b.payload_hash); }
#[test] fn fixed_cross_language_vector() { let fixture=include_str!("../../../fixtures/domain/canonical-record-v1.json"); let r:RevisionedRecord=serde_json::from_str(fixture).unwrap(); assert!(r.validate().is_ok()); assert_eq!(payload_hash(&r.record).unwrap(),r.payload_hash); }
#[test] fn arithmetic_property_1000_cases() { for i in 0..1000u64 { let x=TokenBreakdown{input_uncached_tokens:i,cache_read_tokens:2*i,cache_write_tokens:3*i,output_tokens:4*i,unclassified_tokens:5*i,reasoning_tokens:Some(i)}; assert_eq!(x.total().unwrap(),15*i); x.validate().unwrap(); } }

#[test] fn global_scope_requires_an_opaque_account() { let mut r=sample(); r.scope=UsageScope::AccountGlobal; r.account_id=None; assert_eq!(r.validate(),Err(DomainError::MissingField("account_id"))); }
#[test] fn exact_precision_cannot_hide_unclassified_tokens() { let mut r=sample(); r.usage.unclassified_tokens=5; assert_eq!(r.validate(),Err(DomainError::InvalidField("precision"))); }
#[test] fn aggregate_precision_can_preserve_reliable_total_without_components() { let mut r=sample(); r.precision=Precision::Aggregate; r.usage=TokenBreakdown::aggregate_only(1000); assert!(r.validate().is_ok()); assert_eq!(r.usage.total().unwrap(),1000); }
#[test] fn unnormalized_wire_record_is_rejected() { let mut r=RevisionedRecord::from_store(sample(),1).unwrap(); r.record.model_id=Some("cafe\u{301}".into()); r.payload_hash=payload_hash(&r.record).unwrap(); assert_eq!(r.validate(),Err(DomainError::NonCanonicalRecord)); }
