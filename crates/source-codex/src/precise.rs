//! M0-05 precision collector: per-response native usage is authoritative.
//! Legacy sessions use monotonic cumulative deltas. A schema switch triggers
//! a full, source-scoped reconciliation with explicit tombstones.
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufReader, Seek, SeekFrom};
use std::path::Path;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokenscope_core_domain::{
    stable_record_id, session_key, CanonicalRecord, Precision, TokenBreakdown, UsageScope,
};
use tokenscope_local_store::{LocalStore, SourceCheckpoint};
use super::{bounded_line, discover_codex_files, event_ms, hash_path, mtime_ns, now_ms,
    read_session_header, sha_tail, CodexError, Counters, Result, MAX_FILES};

const VERSION: u32 = 2;
const MAX_BATCH: usize = 50_000; // explicit upper bound pending true streaming decoder

#[derive(Clone, Debug, Serialize, Deserialize)]
struct V2Cursor {
    path_hash: String,
    tail_hash: String,
    mtime_ns: u128,
    mode: String, // "legacy" or "modern"
    model: Option<String>,
    cumulative: Option<Counters>,
    legacy_seq: u64,
    response_seq: u64,
}

#[derive(Debug)]
pub struct PreciseScan {
    pub next: SourceCheckpoint,
    pub records: Vec<CanonicalRecord>,
    pub full_reconcile: bool,
    pub parsed_lines: usize,
    pub pending_line: bool,
    pub mode: String,
}

fn usage(v: &Value) -> Result<TokenBreakdown> {
    let number = |name: &str, required: bool| -> Result<u64> {
        match v.get(name) {
            None if !required => Ok(0),
            Some(x) => x.as_u64().ok_or(CodexError::Invalid("invalid usage counter")),
            None => Err(CodexError::Invalid("missing usage counter")),
        }
    };
    let input = number("input_tokens", true)?;
    let cache_read = number("cached_input_tokens", false)?;
    let cache_write = number("cache_write_input_tokens", false)?;
    let output = number("output_tokens", true)?;
    let sum = input.checked_add(output).ok_or(CodexError::Invalid("usage overflow"))?;
    let total = match v.get("total_tokens") {
        Some(x) => x.as_u64().ok_or(CodexError::Invalid("invalid total_tokens"))?,
        None => sum,
    };
    if total < sum || total > i64::MAX as u64 ||
       cache_read.checked_add(cache_write).is_none_or(|c| c > input) {
        return Err(CodexError::Invalid("usage buckets inconsistent"));
    }
    let reasoning = v.get("reasoning_output_tokens")
        .map(|x| x.as_u64().ok_or(CodexError::Invalid("invalid reasoning")))
        .transpose()?;
    if reasoning.is_some_and(|r| r > output) { return Err(CodexError::Invalid("reasoning exceeds output")); }
    Ok(TokenBreakdown {
        input_uncached_tokens: input-cache_read-cache_write,
        cache_read_tokens: cache_read, cache_write_tokens: cache_write,
        output_tokens: output, unclassified_tokens: total-sum, reasoning_tokens: reasoning,
    })
}

fn legacy_difference(current: Counters, previous: Option<Counters>) -> Result<TokenBreakdown> {
    let p=previous.unwrap_or_default();
    if !current.monotonic_from(&p) {return Err(CodexError::CounterRegression)}
    let input=current.input-p.input;
    let cached=current.cached-p.cached;
    let output=current.output-p.output;
    let total=current.total-p.total;
    if cached > input || total < input.checked_add(output).ok_or(CodexError::Invalid("delta overflow"))? {
        return Err(CodexError::Invalid("cumulative delta inconsistent"));
    }
    let reasoning=match (current.reasoning,previous.and_then(|x|x.reasoning)) {
        (Some(n),Some(old))=>Some(n.checked_sub(old).ok_or(CodexError::CounterRegression)?),
        (Some(n),None) if previous.is_none()=>Some(n),
        _=>None,
    };
    if reasoning.is_some_and(|r|r>output) {return Err(CodexError::Invalid("delta reasoning"))}
    Ok(TokenBreakdown {input_uncached_tokens:input-cached,cache_read_tokens:cached,
        cache_write_tokens:0,output_tokens:output,
        unclassified_tokens:total-input-output,reasoning_tokens:reasoning})
}

fn record(session:&str,device:&str,native_key:&[u8],model:Option<String>,at:i64,
    tokens:TokenBreakdown)->Result<CanonicalRecord> {
    let rec=CanonicalRecord {
        record_id:stable_record_id("codex",session.as_bytes(),native_key)?,
        owner_device_id:device.to_owned(),tool_id:"codex".into(),
        scope:UsageScope::DeviceLocal,
        precision:if tokens.unclassified_tokens == 0 {Precision::Exact}else{Precision::Aggregate},
        account_id:None,model_id:model,project_id:None,
        session_id:Some(session_key("codex",session.as_bytes())?),event_at_ms:Some(at),
        usage:tokens,estimated_cost_microusd:None,price_version:None,is_deleted:false,
    };
    Ok(rec.normalized()?)
}

pub fn scan_precise(path:&Path,device_id:&str,prior:Option<&SourceCheckpoint>,observed_ms:i64)->Result<PreciseScan> {
    let session=read_session_header(path)?;
    let key=session_key("codex",session.as_bytes())?;
    if let Some(cp)=prior {
        if cp.tool_id!="codex" || cp.source_key!=key {return Err(CodexError::Invalid("checkpoint mismatch"))}
    }
    let meta=fs::metadata(path)?;
    if !meta.is_file(){return Err(CodexError::Invalid("not a regular file"))}
    let size=meta.len();let modified=mtime_ns(&meta)?;let path_identity=hash_path(path)?;
    let old=prior.filter(|x|x.parser_version==VERSION)
        .and_then(|x|serde_json::from_str::<V2Cursor>(&x.source_fingerprint).ok());
    let reuse=match (prior,old.as_ref()) {
        (Some(cp),Some(saved))=>cp.read_offset<=size && saved.path_hash==path_identity &&
            (cp.read_offset!=size || saved.mtime_ns==modified) &&
            saved.tail_hash==sha_tail(path,cp.read_offset)?,
        _=>false,
    };
    let mut offset=if reuse {prior.map_or(0,|x|x.read_offset)} else {0};
    let mut old_mode=if reuse {old.as_ref().map(|x|x.mode.clone()).unwrap_or_default()} else {String::new()};
    let mut model=if reuse {old.as_ref().and_then(|x|x.model.clone())}else{None};
    let mut last=if reuse {old.as_ref().and_then(|x|x.cumulative)} else {None};
    let mut legacy_seq=if reuse {old.as_ref().map_or(0,|x|x.legacy_seq)}else{0};
    let mut response_seq=if reuse {old.as_ref().map_or(0,|x|x.response_seq)}else{0};
    let mut rd=BufReader::new(File::open(path)?);rd.seek(SeekFrom::Start(offset))?;
    let mut legacy=Vec::new();let mut modern=Vec::new();
    let mut ids=HashSet::new();let mut first_legacy_before_modern=false;
    let mut saw_positive_legacy=false;let mut legacy_error=None;
    let mut parsed=0;let mut pending=false;
    loop {
        let start=rd.stream_position()?;
        let (line,done)=bounded_line(&mut rd)?;
        if line.is_empty(){break}
        if !done {pending=true;offset=start;break}
        parsed+=1;
        let v:Value=serde_json::from_slice(&line)?;
        let typ=v.get("type").and_then(Value::as_str);
        let payload=v.get("payload");
        if typ==Some("turn_context") {
            if let Some(s)=payload.and_then(|p|p.get("model")).and_then(Value::as_str) {
                if s.len()>256 || s.trim().is_empty() || s.chars().any(char::is_control) {
                    return Err(CodexError::Invalid("model metadata"));
                }
                model=Some(s.to_owned());
            }
        }
        if typ==Some("token_usage_record") {
            let p=payload.ok_or(CodexError::Invalid("response payload"))?;
            let usage_value=p.get("usage").ok_or(CodexError::Invalid("response usage"))?;
            let token=usage(usage_value)?;
            let ts=event_ms(&v)?.ok_or(CodexError::Invalid("missing response timestamp"))?;
            response_seq=response_seq.checked_add(1).ok_or(CodexError::Invalid("response overflow"))?;
            let native=match p.get("response_id").and_then(Value::as_str) {
                Some(id) if !id.is_empty() && id.len()<=512 => format!("response:{id}"),
                _=>format!("response-offset:{start}"),
            };
            let r=record(&session,device_id,native.as_bytes(),model.clone(),ts,token)?;
            if !ids.insert(r.record_id.clone()) {return Err(CodexError::Invalid("duplicate response identity"))}
            if modern.len()>=MAX_BATCH {return Err(CodexError::Limit("response batch > 50000"))}
            modern.push(r);
        }
        if typ==Some("event_msg") && payload.and_then(|p|p.get("type")).and_then(Value::as_str)==Some("token_count") &&
            old_mode!="modern" && modern.is_empty() {
            if let Some(info)=payload.and_then(|p|p.get("info")) {
                if let Some(next)=Counters::from_info(info)? {
                    if last!=Some(next) {
                        match legacy_difference(next,last) {
                            Ok(delta)=>{
                                if delta.total()? > 0 {
                                    if modern.is_empty(){saw_positive_legacy=true;}
                                    legacy_seq=legacy_seq.checked_add(1).ok_or(CodexError::Invalid("legacy ordinal overflow"))?;
                                    let ts=event_ms(&v)?.ok_or(CodexError::Invalid("missing legacy timestamp"))?;
                                    let r=record(&session,device_id,format!("legacy:{legacy_seq}").as_bytes(),model.clone(),ts,delta)?;
                                    if legacy.len()>=MAX_BATCH {return Err(CodexError::Limit("legacy batch > 50000"))}
                                    legacy.push(r);
                                }
                                last=Some(next);
                            }
                            Err(e)=>{legacy_error=Some(e);}
                        }
                    }
                }
            }
        }
        if !modern.is_empty() && saw_positive_legacy {first_legacy_before_modern=true;}
        offset=rd.stream_position()?;
    }
    let after=fs::metadata(path)?;
    if after.len()!=size || mtime_ns(&after)?!=modified || offset>size {
        return Err(CodexError::Invalid("file changed during scan"));
    }
    // An incomplete, rewritten source is not authoritative for deletion.
    // Preserve the prior snapshot and retry once the writer completes the line.
    if pending && !reuse && prior.is_some() {
        return Err(CodexError::PendingHeader);
    }
    // New modern records in a previously legacy-tracked file require complete
    // replacement of the old delta projection, not a mixture of both schemas.
    if reuse && old_mode=="legacy" && !modern.is_empty() {
        let rebuilt=scan_precise(path,device_id,None,observed_ms)?;
        if rebuilt.pending_line {return Err(CodexError::PendingHeader)}
        return Ok(rebuilt);
    }
    let mode=if old_mode=="modern" || !modern.is_empty(){"modern"}else{"legacy"};
    if mode=="legacy" {
        if let Some(err)=legacy_error {return Err(err)}
    } else if first_legacy_before_modern {
        // A mixed-format rollout with nonzero usage before the first response
        // cannot be proven complete by simply ignoring cumulative snapshots.
        return Err(CodexError::Invalid("mixed legacy/modern coverage"));
    }
    let records=if mode=="modern" {modern}else{legacy};
    // A newly replaced header-only file is not proof that previously measured
    // responses were deleted. Keep last-known-good until usage is observable.
    if !reuse && prior.is_some() && records.is_empty() {
        return Err(CodexError::PendingHeader);
    }
    if records.len()>MAX_BATCH {return Err(CodexError::Limit("batch too large"))}
    old_mode=mode.to_owned();
    let next_time=prior.map_or(observed_ms,|p|observed_ms.max(p.last_scan_ms.saturating_add(1)));
    let cursor=V2Cursor{path_hash:path_identity,tail_hash:sha_tail(path,offset)?,mtime_ns:modified,
        mode:old_mode.clone(),model,cumulative:if mode=="modern"{None}else{last},legacy_seq,response_seq};
    let fingerprint=serde_json::to_string(&cursor)?;
    if fingerprint.len()>1024 {return Err(CodexError::Limit("checkpoint fingerprint"))}
    let next=SourceCheckpoint{tool_id:"codex".into(),source_key:key,
        source_fingerprint:fingerprint,read_offset:offset,parser_version:VERSION,last_scan_ms:next_time,
        last_complete_scan_ms:if pending {prior.and_then(|x|x.last_complete_scan_ms)}else{Some(next_time)},
        last_status:if pending {"partial"}else{"complete"}.into()};
    Ok(PreciseScan{next,records,full_reconcile:!reuse,parsed_lines:parsed,pending_line:pending,mode:old_mode})
}

#[derive(Debug, Default)]
pub struct PreciseReport {
    pub files:usize,pub inserted:usize,pub revised:usize,pub unchanged:usize,
    pub lines:usize,pub pending:usize,pub reconciled:usize,
}

pub fn collect_codex_precise(home:&Path,store:&mut LocalStore,device_id:&str)->Result<PreciseReport> {
    let mut report=PreciseReport::default();let mut sessions=HashSet::new();
    let files=discover_codex_files(home)?;
    if files.len()>MAX_FILES {return Err(CodexError::Limit("too many files"))}
    for file in files {
        let session=match read_session_header(&file) {
            Ok(v)=>v,Err(CodexError::PendingHeader)=>{report.pending+=1;continue},Err(e)=>return Err(e),
        };
        let key=session_key("codex",session.as_bytes())?;
        if !sessions.insert(key.clone()) {return Err(CodexError::Invalid("duplicate session sources"))}
        let prior=store.checkpoint("codex",&key)?;
        let scan=match scan_precise(&file,device_id,prior.as_ref(),now_ms()?) {
            Ok(v)=>v,
            Err(CodexError::PendingHeader)=>{report.pending+=1;continue},
            Err(e)=>return Err(e),
        };
        // A large append-only delta cannot be sent to the 2,000-record
        // incremental API, and it must NOT be treated as a complete source.
        // Re-read the entire file before generating any missing-record tombstones.
        let scan = if !scan.full_reconcile && scan.records.len() > 2000 {
            let mut rebuilt = scan_precise(&file, device_id, None, now_ms()?)?;
            if rebuilt.pending_line { return Err(CodexError::PendingHeader); }
            if let Some(old) = prior.as_ref() {
                if rebuilt.next.last_scan_ms <= old.last_scan_ms {
                    rebuilt.next.last_scan_ms = old.last_scan_ms.saturating_add(1);
                    rebuilt.next.last_complete_scan_ms = Some(rebuilt.next.last_scan_ms);
                }
            }
            rebuilt
        } else { scan };
        let (inserted, revised, unchanged) = if scan.full_reconcile {
            if scan.pending_line && prior.is_some() { return Err(CodexError::PendingHeader); }
            if scan.pending_line {
                // Partial brand-new sources are append-only; no missing-record deletion.
                if scan.records.len() > 2000 { return Err(CodexError::PendingHeader); }
                let result = store.apply_scan(prior.as_ref(), &scan.next, &scan.records,
                    scan.next.last_scan_ms, false)?;
                (result.inserted, result.revised, result.unchanged)
            } else {
                let result = store.reconcile_complete_source(
                    prior.as_ref(), &scan.next, scan.records.into_iter(), scan.next.last_scan_ms,
                )?;
                report.reconciled += 1;
                (result.applied.inserted, result.applied.revised + result.tombstoned,
                    result.applied.unchanged)
            }
        } else {
            let result = store.apply_scan(prior.as_ref(), &scan.next,
                &scan.records, scan.next.last_scan_ms, false)?;
            (result.inserted, result.revised, result.unchanged)
        };
        report.files+=1;report.inserted+=inserted;report.revised+=revised;
        report.unchanged+=unchanged;report.lines+=scan.parsed_lines;
        if scan.pending_line {report.pending+=1;}
    }
    Ok(report)
}
