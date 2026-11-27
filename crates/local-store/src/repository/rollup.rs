//! Derived dashboard counters. Triggers maintain them in the same transaction
//! as the authoritative rows, including external sync and price corrections.
use crate::error::Result;
use rusqlite::Transaction;

const HOUR_MS: i64 = 3_600_000;
pub(super) const UNTIMED: i64 = i64::MIN;
const KEYS: &str = "is_remote,hour,owner_device_id,scope,tool_id,model_key,project_key";
const METRICS: [&str; 14] = [
    "input_uncached_tokens",
    "cache_read_tokens",
    "cache_write_tokens",
    "output_tokens",
    "unclassified_tokens",
    "reasoning_tokens",
    "reasoning_known",
    "records",
    "priced",
    "exact",
    "aggregate",
    "estimated",
    "unknown",
    "cost",
];

#[derive(Clone, Copy)]
pub(super) enum Source {
    Local,
    Remote,
}

impl Source {
    fn table(self) -> &'static str {
        match self {
            Self::Local => "usage_records",
            Self::Remote => "remote_usage_cache",
        }
    }

    fn id(self) -> i64 {
        match self {
            Self::Local => 0,
            Self::Remote => 1,
        }
    }

    fn field(self, alias: &str, field: &str) -> String {
        match self {
            Self::Local => format!("{alias}.{field}"),
            Self::Remote if field == "owner_device_id" => format!("{alias}.{field}"),
            Self::Remote => {
                let path = if field.ends_with("_tokens") {
                    "$.usage."
                } else {
                    "$."
                };
                format!("json_extract({alias}.payload_json,'{path}{field}')")
            }
        }
    }

    fn active(self, alias: &str) -> String {
        match self {
            Self::Local => format!("{alias}.is_deleted=0"),
            Self::Remote => format!(
                "{alias}.is_deleted=0 AND {}='device_local'",
                self.field(alias, "scope")
            ),
        }
    }

    fn expressions(self, alias: &str) -> (Vec<String>, Vec<String>) {
        let field = |name| self.field(alias, name);
        let event = field("event_at_ms");
        let model = field("model_id");
        let project = field("project_id");
        let reason = field("reasoning_tokens");
        let cost = field("estimated_cost_microusd");
        let precision = field("precision");
        let bucket = format!("COALESCE({event}/{HOUR_MS}-({event}%{HOUR_MS}<0),{UNTIMED})");
        let keys = vec![
            self.id().to_string(),
            bucket,
            field("owner_device_id"),
            field("scope"),
            field("tool_id"),
            format!("COALESCE({model},char(0))"),
            format!("COALESCE({project},char(0))"),
        ];
        let mut metrics: Vec<_> = METRICS[..5]
            .iter()
            .map(|name| format!("COALESCE({},0)", field(name)))
            .collect();
        metrics.extend([
            format!("COALESCE({reason},0)"),
            format!("({reason} IS NOT NULL)"),
            "1".into(),
            format!("({cost} IS NOT NULL)"),
            format!("COALESCE({precision}='exact',0)"),
            format!("COALESCE({precision}='aggregate',0)"),
            format!("COALESCE({precision}='estimated',0)"),
            format!("COALESCE({precision}='unknown',0)"),
            format!("COALESCE({cost},0)"),
        ]);
        (keys, metrics)
    }
}

pub(super) fn install(tx: &Transaction<'_>) -> Result<()> {
    let columns = METRICS
        .map(|name| format!("{name} INTEGER NOT NULL"))
        .join(",");
    tx.execute_batch(&format!(
        "CREATE TABLE dashboard_hourly_usage (
            is_remote INTEGER NOT NULL,hour INTEGER NOT NULL,owner_device_id TEXT NOT NULL,
            scope TEXT NOT NULL,tool_id TEXT NOT NULL,model_key TEXT NOT NULL,project_key TEXT NOT NULL,
            {columns}, CHECK(records>=0), PRIMARY KEY({KEYS})
         ) STRICT, WITHOUT ROWID;"
    ))?;
    let metrics_columns = METRICS.join(",");
    let additions = METRICS
        .map(|name| format!("{name}={name}+excluded.{name}"))
        .join(",");
    for source in [Source::Local, Source::Remote] {
        let table = source.table();
        let (keys, metrics) = source.expressions("r");
        let grouped = metrics
            .iter()
            .map(|m| format!("SUM({m})"))
            .collect::<Vec<_>>()
            .join(",");
        let active = source.active("r");
        tx.execute_batch(&format!(
            "INSERT INTO dashboard_hourly_usage ({KEYS},{metrics_columns})
             SELECT {},{grouped} FROM {table} r WHERE {active} GROUP BY 1,2,3,4,5,6,7;",
            keys.join(",")
        ))?;
        let (new_keys, new_metrics) = source.expressions("NEW");
        let new_values = new_metrics.join(",");
        let new_active = source.active("NEW");
        let insert = format!(
            "INSERT INTO dashboard_hourly_usage ({KEYS},{metrics_columns})
             SELECT {},{new_values} WHERE {new_active}
             ON CONFLICT ({KEYS}) DO UPDATE SET {additions};",
            new_keys.join(",")
        );
        let (old_keys, old_metrics) = source.expressions("OLD");
        // Subtract only from the existing bucket. An absent bucket is a
        // consistency failure and must never silently change the projection.
        let key_match = KEYS
            .split(',')
            .zip(&old_keys)
            .map(|(key, old)| format!("{key}=({old})"))
            .collect::<Vec<_>>()
            .join(" AND ");
        let subtractions = METRICS
            .iter()
            .zip(&old_metrics)
            .map(|(name, value)| format!("{name}={name}-({value})"))
            .collect::<Vec<_>>()
            .join(",");
        let old_active = source.active("OLD");
        let remove = format!(
            "UPDATE dashboard_hourly_usage SET {subtractions} WHERE {key_match} AND {old_active};
             SELECT CASE WHEN ({old_active}) AND changes()=0
                     THEN RAISE(ABORT,'missing dashboard usage bucket') END;
             DELETE FROM dashboard_hourly_usage WHERE {key_match} AND records=0;"
        );
        let changed = new_keys
            .iter()
            .chain(&new_metrics)
            .zip(old_keys.iter().chain(&old_metrics))
            .map(|(new, old)| format!("({new}) IS NOT ({old})"))
            .collect::<Vec<_>>()
            .join(" OR ");
        tx.execute_batch(&format!(
            "CREATE TRIGGER dashboard_hourly_{table}_insert AFTER INSERT ON {table}
               BEGIN {insert} END;
             CREATE TRIGGER dashboard_hourly_{table}_delete AFTER DELETE ON {table}
               BEGIN {remove} END;
             CREATE TRIGGER dashboard_hourly_{table}_update AFTER UPDATE ON {table}
               WHEN ({new_active}) IS NOT ({old_active}) OR
                    (({new_active}) AND ({old_active}) AND ({changed}))
               BEGIN {remove} {insert} END;"
        ))?;
    }
    Ok(())
}

/// Whole UTC hours use materialized totals. Exact millisecond boundaries,
/// including partial hours and local timezone offsets, use indexed raw rows.
pub(super) fn usage_rows(source: Source, from_ms: Option<i64>, to_ms: Option<i64>) -> String {
    let mut parts = Vec::new();
    let first_hour = from_ms.map(|from| from / HOUR_MS + i64::from(from % HOUR_MS != 0));
    let end_hour = to_ms.map(|to| to / HOUR_MS);
    let has_whole_hours = first_hour.zip(end_hour).is_none_or(|(a, b)| a < b);
    if has_whole_hours {
        let mut sql = format!(
            "SELECT {KEYS},{} FROM dashboard_hourly_usage WHERE is_remote={}",
            METRICS.join(","),
            source.id()
        );
        if let Some(first) = first_hour {
            sql.push_str(&format!(" AND hour>={first}"));
        } else if to_ms.is_some() {
            sql.push_str(&format!(" AND hour>{UNTIMED}"));
        }
        if let Some(end) = end_hour {
            sql.push_str(&format!(" AND hour<{end}"));
        }
        parts.push(sql);
    }

    let (keys, metrics) = source.expressions("r");
    let table = source.table();
    let event = source.field("r", "event_at_ms");
    let active = source.active("r");
    let mut edge = |from: Option<i64>, to: Option<i64>| {
        let mut sql = format!(
            "SELECT {},{} FROM {table} r WHERE {active}",
            keys.join(","),
            metrics.join(",")
        );
        if let Some(from) = from {
            sql.push_str(&format!(" AND {event}>={from}"));
        }
        if let Some(to) = to {
            sql.push_str(&format!(" AND {event}<{to}"));
        }
        parts.push(sql);
    };
    if !has_whole_hours {
        edge(from_ms, to_ms);
    } else {
        if let Some(from) = from_ms.filter(|v| v % HOUR_MS != 0) {
            edge(
                Some(from),
                first_hour.and_then(|hour| hour.checked_mul(HOUR_MS)),
            );
        }
        if let Some(to) = to_ms.filter(|v| v % HOUR_MS != 0) {
            edge(end_hour.map(|hour| hour * HOUR_MS), Some(to));
        }
    }
    // The first arm may be raw for a range within one hour. Supply stable
    // column names independently of the selected arms.
    format!(
        "WITH usage_rows ({KEYS},{}) AS ({})",
        METRICS.join(","),
        parts.join(" UNION ALL ")
    )
}

pub(super) fn daily_rows(source: Source) -> String {
    let table = source.table();
    let event = source.field("r", "event_at_ms");
    let active = source.active("r");
    let (_, metrics) = source.expressions("r");
    let owner_filter = match source {
        Source::Local => "AND r.owner_device_id=?2",
        Source::Remote => "",
    };
    let tokens = METRICS[..5].join("+");
    let raw_tokens = metrics[..5].join("+");
    let id = source.id();
    format!(
        "WITH ranges AS (
             SELECT key AS day_idx,json_extract(value,'$[0]') AS day,
                    json_extract(value,'$[1]') AS starts,json_extract(value,'$[2]') AS ends
             FROM json_each(?1)
         ), bounds AS (
             SELECT *, starts/{HOUR_MS}+(starts%{HOUR_MS}<>0) AS first_hour,
                       ends/{HOUR_MS} AS end_hour
             FROM ranges
         ), edges AS (
             SELECT day_idx,starts AS lo,
                    starts+MIN(ends-starts,({HOUR_MS}-starts%{HOUR_MS})%{HOUR_MS}) AS hi
             FROM bounds
             UNION ALL
             SELECT day_idx,end_hour*{HOUR_MS},ends FROM bounds WHERE end_hour>=first_hour
         ), points(day_idx,owner,tokens,input,output) AS (
             SELECT b.day_idx,r.owner_device_id,{tokens},r.input_uncached_tokens,r.output_tokens
             FROM bounds b CROSS JOIN dashboard_hourly_usage r
             WHERE r.is_remote={id} AND r.scope='device_local' {owner_filter}
               AND r.hour>=b.first_hour AND r.hour<b.end_hour
             UNION ALL
             SELECT e.day_idx,r.owner_device_id,{raw_tokens},{},{}
             FROM edges e CROSS JOIN {table} r
             WHERE e.lo<e.hi AND {active} {owner_filter}
               AND {}='device_local' AND {event}>=e.lo AND {event}<e.hi
         )",
        metrics[0],
        metrics[3],
        source.field("r", "scope")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::LocalStore;
    use rusqlite::params;

    fn totals(
        conn: &rusqlite::Connection,
        source: Source,
        from: Option<i64>,
        to: Option<i64>,
    ) -> (i64, i64, i64) {
        let rows = usage_rows(source, from, to);
        conn.query_row(
            &format!(
                "{rows} SELECT COALESCE(SUM(records),0),
            COALESCE(SUM(input_uncached_tokens),0),COALESCE(SUM(priced),0) FROM usage_rows"
            ),
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap()
    }

    #[test]
    fn local_deltas_follow_bucket_changes_and_transactions() {
        let mut store = LocalStore::open_in_memory().unwrap();
        for (id, event, tokens) in [
            ("edge", Some(HOUR_MS - 1), 7),
            ("whole", Some(HOUR_MS), 11),
            ("end", Some(2 * HOUR_MS), 13),
            ("untimed", None, 17),
        ] {
            store.conn.execute(
                "INSERT INTO usage_records(record_id,owner_device_id,tool_id,scope,event_at_ms,input_uncached_tokens,
                 precision,record_revision,payload_hash,observed_at_ms,updated_at_ms)
                 VALUES(?1,'device','codex','device_local',?2,?3,'exact',1,'hash',0,0)",
                params![id,event,tokens],
            ).unwrap();
        }
        assert_eq!(totals(&store.conn, Source::Local, None, None), (4, 48, 0));
        assert_eq!(
            totals(
                &store.conn,
                Source::Local,
                Some(HOUR_MS - 1),
                Some(2 * HOUR_MS)
            ),
            (2, 18, 0)
        );
        assert_eq!(
            totals(&store.conn, Source::Local, Some(HOUR_MS), Some(HOUR_MS + 1)),
            (1, 11, 0)
        );
        {
            let tx = store.conn.transaction().unwrap();
            tx.execute(
                "UPDATE usage_records SET is_deleted=1 WHERE record_id='whole'",
                [],
            )
            .unwrap();
            assert_eq!(totals(&tx, Source::Local, None, None), (3, 37, 0));
            tx.rollback().unwrap();
        }
        assert_eq!(totals(&store.conn, Source::Local, None, None), (4, 48, 0));
        store.conn.execute("UPDATE usage_records SET event_at_ms=?1,project_id='revised',estimated_cost_microusd=0
                            WHERE record_id='whole'", [2*HOUR_MS]).unwrap();
        store
            .conn
            .execute("DELETE FROM usage_records WHERE record_id='untimed'", [])
            .unwrap();
        assert_eq!(totals(&store.conn, Source::Local, None, None), (3, 31, 1));
        assert_eq!(
            totals(&store.conn, Source::Local, Some(HOUR_MS), Some(2 * HOUR_MS)),
            (0, 0, 0)
        );
    }

    #[test]
    fn remote_revisions_replace_totals_and_keep_zero_cost_distinct() {
        let store = LocalStore::open_in_memory().unwrap();
        let upsert = "INSERT INTO remote_usage_cache(
            owner_device_id,record_id,record_revision,payload_json,is_deleted,server_change_seq
         ) VALUES('remote','record',?1,?2,?3,?1)
         ON CONFLICT(owner_device_id,record_id) DO UPDATE SET
            record_revision=excluded.record_revision,payload_json=excluded.payload_json,is_deleted=excluded.is_deleted";
        for (revision, input, cost, deleted, expected) in [
            (1, 9, None, 0, (1, 9, 0)),
            (2, 20, Some(0), 0, (1, 20, 1)),
            (3, 0, None, 1, (0, 0, 0)),
        ] {
            let payload = serde_json::json!({
                "scope":"device_local","tool_id":"codex","event_at_ms":HOUR_MS,
                "precision":"exact","usage":{"input_uncached_tokens":input},"estimated_cost_microusd":cost
            });
            store
                .conn
                .execute(upsert, params![revision, payload.to_string(), deleted])
                .unwrap();
            assert_eq!(totals(&store.conn, Source::Remote, None, None), expected);
        }
    }
}
