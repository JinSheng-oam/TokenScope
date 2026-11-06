use super::super::models::{
    SessionAnalysisPage, SessionModelUsage, SessionUsage, SessionUsagePage,
};
use super::super::LocalStore;
use super::SESSION_LINKS_CTE;
use crate::error::{pos_u64, Result, StoreError};
use rusqlite::params;
use std::time::Instant;
impl LocalStore {
    pub(super) fn session_usage_page(
        &self,
        owner_device_id: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        offset: u64,
        limit: u32,
    ) -> Result<SessionUsagePage> {
        self.session_usage_page_filtered(
            owner_device_id,
            from_ms,
            to_ms,
            offset,
            limit,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn session_analysis_page(
        &self,
        owner_device_id: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        offset: u64,
        limit: u32,
        search: Option<&str>,
        tool_id: Option<&str>,
        model_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<SessionAnalysisPage> {
        let page = self.session_usage_page_filtered(
            owner_device_id,
            from_ms,
            to_ms,
            offset,
            limit,
            search,
            tool_id,
            model_id,
            project_id,
        )?;
        let filter_options = self.session_filter_options(owner_device_id, from_ms, to_ms)?;
        Ok(SessionAnalysisPage {
            page,
            filter_options,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn session_usage_page_filtered(
        &self,
        owner_device_id: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
        offset: u64,
        limit: u32,
        search: Option<&str>,
        tool_id: Option<&str>,
        model_id: Option<&str>,
        project_id: Option<&str>,
    ) -> Result<SessionUsagePage> {
        if limit == 0 || limit > 200 || offset > i64::MAX as u64 {
            return Err(StoreError::InvalidInput("session page bounds"));
        }
        let search = search.map(str::trim).filter(|value| !value.is_empty());
        if search
            .is_some_and(|value| value.chars().count() > 120 || value.chars().any(char::is_control))
        {
            return Err(StoreError::InvalidInput("session search"));
        }
        let cte = format!(
            r#"{SESSION_LINKS_CTE}, modern_codex_records AS (
                SELECT DISTINCT rs.record_id
                FROM record_sources rs
                JOIN source_checkpoints cp
                    ON cp.tool_id=rs.tool_id AND cp.source_key=rs.source_key
                WHERE rs.tool_id='codex'
                    AND cp.source_fingerprint LIKE '%"mode":"modern"%'
            ),
            usage_sessions AS (
                SELECT u.session_id,u.tool_id,
                    COALESCE(SUM(u.input_uncached_tokens+u.cache_read_tokens+u.cache_write_tokens+u.output_tokens+u.unclassified_tokens),0) AS total_tokens,
                    COALESCE(SUM(u.input_uncached_tokens),0) AS input_uncached_tokens,
                    COALESCE(SUM(u.cache_read_tokens),0) AS cache_read_tokens,
                    COALESCE(SUM(u.cache_write_tokens),0) AS cache_write_tokens,
                    COUNT(*) AS usage_record_count,
                    SUM(CASE WHEN u.precision='exact' THEN 1 ELSE 0 END) AS exact_records,
                    SUM(CASE WHEN u.precision='aggregate' THEN 1 ELSE 0 END) AS aggregate_records,
                    SUM(CASE WHEN u.precision='estimated' THEN 1 ELSE 0 END) AS estimated_records,
                    SUM(CASE WHEN u.precision='unknown' THEN 1 ELSE 0 END) AS unknown_precision_records,
                    SUM(CASE WHEN u.event_at_ms IS NULL THEN 1 ELSE 0 END) AS untimed_records,
                    COUNT(DISTINCT u.model_id) AS model_count,
                    CASE WHEN SUM(CASE WHEN mcr.record_id IS NOT NULL THEN 1 ELSE 0 END)>0
                        THEN SUM(CASE WHEN mcr.record_id IS NOT NULL AND u.model_id IS NOT NULL THEN 1 ELSE 0 END) ELSE NULL END AS known_response_count,
                    CASE WHEN SUM(CASE WHEN mcr.record_id IS NOT NULL THEN 1 ELSE 0 END)>0
                        THEN SUM(CASE WHEN mcr.record_id IS NOT NULL AND u.model_id IS NULL THEN 1 ELSE 0 END) ELSE NULL END AS unknown_response_count,
                    MAX(u.event_at_ms) AS last_event_at_ms,NULL AS activity_observed_at_ms
                FROM usage_records u
                LEFT JOIN modern_codex_records mcr ON mcr.record_id=u.record_id
                WHERE u.owner_device_id=?1 AND u.scope='device_local' AND u.is_deleted=0
                    AND u.session_id IS NOT NULL AND u.tool_id<>'antigravity'
                    AND (?2 IS NULL OR u.event_at_ms >= ?2) AND (?3 IS NULL OR u.event_at_ms < ?3)
                GROUP BY u.tool_id,u.session_id
            ),
            ag_usage AS (
                SELECT rs.source_key,
                    SUM(u.input_uncached_tokens+u.cache_read_tokens+u.cache_write_tokens+u.output_tokens+u.unclassified_tokens) AS total_tokens,
                    COALESCE(SUM(u.input_uncached_tokens),0) AS input_uncached_tokens,
                    COALESCE(SUM(u.cache_read_tokens),0) AS cache_read_tokens,
                    COALESCE(SUM(u.cache_write_tokens),0) AS cache_write_tokens,
                    COUNT(*) AS usage_record_count,
                    SUM(CASE WHEN u.precision='exact' THEN 1 ELSE 0 END) AS exact_records,
                    SUM(CASE WHEN u.precision='aggregate' THEN 1 ELSE 0 END) AS aggregate_records,
                    SUM(CASE WHEN u.precision='estimated' THEN 1 ELSE 0 END) AS estimated_records,
                    SUM(CASE WHEN u.precision='unknown' THEN 1 ELSE 0 END) AS unknown_precision_records,
                    SUM(CASE WHEN u.event_at_ms IS NULL THEN 1 ELSE 0 END) AS untimed_records,
                    COUNT(DISTINCT u.model_id) AS model_count,MAX(u.event_at_ms) AS last_event_at_ms
                FROM record_sources rs JOIN usage_records u ON u.record_id=rs.record_id
                WHERE rs.tool_id='antigravity' AND u.owner_device_id=?1
                    AND u.scope='device_local' AND u.is_deleted=0
                    AND (?2 IS NULL OR u.event_at_ms >= ?2) AND (?3 IS NULL OR u.event_at_ms < ?3)
                GROUP BY rs.source_key
            ),
            ag_models AS (
                SELECT source_key,COUNT(*) AS model_count
                FROM antigravity_model_activity GROUP BY source_key
            ),
            session_projects AS (
                SELECT tool_id,session_id,group_concat(project_id) AS project_ids
                FROM (
                    SELECT DISTINCT tool_id,session_id,project_id
                    FROM session_links WHERE project_id IS NOT NULL
                )
                GROUP BY tool_id,session_id
            ),
            all_sessions AS (
                SELECT session_id,tool_id,total_tokens,input_uncached_tokens,cache_read_tokens,cache_write_tokens,usage_record_count,exact_records,aggregate_records,
                    estimated_records,unknown_precision_records,untimed_records,model_count,
                    known_response_count,unknown_response_count,last_event_at_ms,activity_observed_at_ms
                FROM usage_sessions
                UNION ALL
                SELECT a.source_key,'antigravity',COALESCE(u.total_tokens,0),COALESCE(u.input_uncached_tokens,0),COALESCE(u.cache_read_tokens,0),COALESCE(u.cache_write_tokens,0),COALESCE(u.usage_record_count,0),
                    COALESCE(u.exact_records,0),COALESCE(u.aggregate_records,0),COALESCE(u.estimated_records,0),
                    COALESCE(u.unknown_precision_records,0),COALESCE(u.untimed_records,0),COALESCE(u.model_count,m.model_count,0),
                    a.known_turns,a.unknown_turns,u.last_event_at_ms,a.observed_at_ms
                FROM antigravity_activity_sources a
                LEFT JOIN ag_usage u ON u.source_key=a.source_key
                LEFT JOIN ag_models m ON m.source_key=a.source_key
                -- Only surface conversations that have usage inside the
                -- selected range; otherwise every old conversation appears
                -- with a zero total and an empty model split.
                WHERE u.source_key IS NOT NULL
            )
        "#
        );
        let page_started = Instant::now();
        let page_sql = format!(
            r#"{cte}
            SELECT s.session_id,s.tool_id,s.total_tokens,s.input_uncached_tokens,s.cache_read_tokens,s.cache_write_tokens,
                s.usage_record_count,s.exact_records,s.aggregate_records,
                s.estimated_records,s.unknown_precision_records,s.untimed_records,s.model_count,
                s.known_response_count,s.unknown_response_count,s.last_event_at_ms,s.activity_observed_at_ms,n.name,
                COALESCE(p.project_ids,''),
                COUNT(*) OVER () AS total_rows
            FROM all_sessions s
            LEFT JOIN session_names n ON n.session_id=s.session_id AND n.tool_id=s.tool_id
            LEFT JOIN session_projects p ON p.tool_id=s.tool_id AND p.session_id=s.session_id
            WHERE (?4 IS NULL OR
                instr(lower(COALESCE(n.name,'未命名会话')),lower(?4))>0 OR
                instr(lower(s.session_id),lower(?4))>0 OR
                instr(lower(s.tool_id),lower(?4))>0 OR
                EXISTS (
                    SELECT 1 FROM session_links l
                    WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                        AND (instr(lower(COALESCE(l.model_id,'')),lower(?4))>0
                            OR instr(lower(COALESCE(l.project_id,'')),lower(?4))>0)
                ))
                AND (?5 IS NULL OR s.tool_id=?5)
                AND (?6 IS NULL OR EXISTS (
                    SELECT 1 FROM session_links l
                    WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                        AND COALESCE(l.model_id,'unknown-model')=?6
                ))
                AND (?7 IS NULL OR
                    (?7='no-project' AND NOT EXISTS (
                        SELECT 1 FROM session_links l
                        WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                            AND l.project_id IS NOT NULL
                    )) OR EXISTS (
                        SELECT 1 FROM session_links l
                        WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                            AND l.project_id=?7
                    ))
            ORDER BY COALESCE(s.last_event_at_ms,s.activity_observed_at_ms,0) DESC,
                s.tool_id,s.session_id
            LIMIT ?8 OFFSET ?9"#
        );
        let mut stmt = self.conn.prepare(&page_sql)?;
        let rows = stmt.query_map(
            params![
                owner_device_id,
                from_ms,
                to_ms,
                search,
                tool_id,
                model_id,
                project_id,
                i64::from(limit),
                i64::try_from(offset).map_err(|_| StoreError::IntegerOutOfRange)?
            ],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, i64>(5)?,
                    r.get::<_, i64>(6)?,
                    r.get::<_, i64>(7)?,
                    r.get::<_, i64>(8)?,
                    r.get::<_, i64>(9)?,
                    r.get::<_, i64>(10)?,
                    r.get::<_, i64>(11)?,
                    r.get::<_, i64>(12)?,
                    r.get::<_, Option<i64>>(13)?,
                    r.get::<_, Option<i64>>(14)?,
                    r.get::<_, Option<i64>>(15)?,
                    r.get::<_, Option<i64>>(16)?,
                    r.get::<_, Option<String>>(17)?,
                    r.get::<_, String>(18)?,
                    r.get::<_, i64>(19)?,
                ))
            },
        )?;
        let mut total_rows: Option<i64> = None;
        let mut sessions = rows
            .map(|row| {
                let (
                    session_id,
                    tool_id,
                    total_tokens,
                    input_uncached_tokens,
                    cache_read_tokens,
                    cache_write_tokens,
                    usage_record_count,
                    exact_records,
                    aggregate_records,
                    estimated_records,
                    unknown_precision_records,
                    untimed_records,
                    model_count,
                    known_response_count,
                    unknown_response_count,
                    last_event_at_ms,
                    activity_observed_at_ms,
                    display_name,
                    project_ids,
                    page_total,
                ) = row?;
                if total_rows.is_none() {
                    total_rows = Some(page_total);
                }
                Ok(SessionUsage {
                    session_id,
                    tool_id,
                    total_tokens: pos_u64(total_tokens)?,
                    input_uncached_tokens: pos_u64(input_uncached_tokens)?,
                    cache_read_tokens: pos_u64(cache_read_tokens)?,
                    cache_write_tokens: pos_u64(cache_write_tokens)?,
                    usage_record_count: pos_u64(usage_record_count)?,
                    exact_records: pos_u64(exact_records)?,
                    aggregate_records: pos_u64(aggregate_records)?,
                    estimated_records: pos_u64(estimated_records)?,
                    unknown_precision_records: pos_u64(unknown_precision_records)?,
                    untimed_records: pos_u64(untimed_records)?,
                    model_count: pos_u64(model_count)?,
                    known_response_count: known_response_count.map(pos_u64).transpose()?,
                    unknown_response_count: unknown_response_count.map(pos_u64).transpose()?,
                    last_event_at_ms,
                    activity_observed_at_ms,
                    display_name,
                    project_ids: project_ids
                        .split(',')
                        .filter(|value| !value.is_empty())
                        .map(str::to_owned)
                        .collect(),
                    models: Vec::new(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let total = match total_rows {
            Some(value) => value,
            None if offset == 0 => 0,
            None => {
                let count_sql = format!(
                    r#"{cte} SELECT COUNT(*) FROM all_sessions s
                    LEFT JOIN session_names n ON n.session_id=s.session_id AND n.tool_id=s.tool_id
                    WHERE (?4 IS NULL OR
                        instr(lower(COALESCE(n.name,'未命名会话')),lower(?4))>0 OR
                        instr(lower(s.session_id),lower(?4))>0 OR instr(lower(s.tool_id),lower(?4))>0 OR
                        EXISTS (SELECT 1 FROM session_links l
                            WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                                AND (instr(lower(COALESCE(l.model_id,'')),lower(?4))>0
                                    OR instr(lower(COALESCE(l.project_id,'')),lower(?4))>0)))
                        AND (?5 IS NULL OR s.tool_id=?5)
                        AND (?6 IS NULL OR EXISTS (SELECT 1 FROM session_links l
                            WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                                AND COALESCE(l.model_id,'unknown-model')=?6))
                        AND (?7 IS NULL OR
                            (?7='no-project' AND NOT EXISTS (SELECT 1 FROM session_links l
                                WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                                    AND l.project_id IS NOT NULL))
                            OR EXISTS (SELECT 1 FROM session_links l
                                WHERE l.tool_id=s.tool_id AND l.session_id=s.session_id
                                    AND l.project_id=?7))"#
                );
                self.conn.query_row(
                    &count_sql,
                    params![
                        owner_device_id,
                        from_ms,
                        to_ms,
                        search,
                        tool_id,
                        model_id,
                        project_id
                    ],
                    |r| r.get(0),
                )?
            }
        };
        if std::env::var_os("TOKENSCOPE_DASHBOARD_PROFILE").is_some() {
            eprintln!("session page: {:?}", page_started.elapsed());
        }
        if !sessions.is_empty() {
            let page_keys = serde_json::to_string(
                &sessions
                    .iter()
                    .map(|s| (&s.tool_id, &s.session_id))
                    .collect::<Vec<_>>(),
            )
            .map_err(|_| StoreError::InvalidInput("session page keys"))?;
            let mut models = self.conn.prepare(
                "SELECT tool_id,session_id,model_id,
                        SUM(input_uncached_tokens+cache_read_tokens+cache_write_tokens+output_tokens+unclassified_tokens),
                        COUNT(*)
                 FROM usage_records
                 WHERE owner_device_id=?1 AND scope='device_local' AND is_deleted=0
                   AND session_id IS NOT NULL AND tool_id<>'antigravity'
                   AND (?2 IS NULL OR event_at_ms >= ?2) AND (?3 IS NULL OR event_at_ms < ?3)
                   AND (tool_id,session_id) IN (
                     SELECT json_extract(value,'$[0]'),json_extract(value,'$[1]') FROM json_each(?4))
                 GROUP BY tool_id,session_id,model_id
                 ORDER BY 4 DESC",
            )?;
            let mut by_session: std::collections::HashMap<
                (String, String),
                Vec<SessionModelUsage>,
            > = std::collections::HashMap::new();
            let mut rows = models.query(params![owner_device_id, from_ms, to_ms, page_keys])?;
            while let Some(row) = rows.next()? {
                let tool_id: String = row.get(0)?;
                let session_id: String = row.get(1)?;
                let model_id: Option<String> = row.get(2)?;
                let tokens: i64 = row.get(3)?;
                let count: i64 = row.get(4)?;
                by_session
                    .entry((tool_id, session_id))
                    .or_default()
                    .push(SessionModelUsage {
                        model_id,
                        total_tokens: pos_u64(tokens)?,
                        record_count: pos_u64(count)?,
                    });
            }
            // Antigravity rows carry their conversation identity in
            // record_sources instead of usage_records.session_id.
            let mut ag_models = self.conn.prepare(
                "SELECT rs.source_key,u.model_id,
                        SUM(u.input_uncached_tokens+u.cache_read_tokens+u.cache_write_tokens+u.output_tokens+u.unclassified_tokens),
                        COUNT(*)
                 FROM record_sources rs JOIN usage_records u ON u.record_id=rs.record_id
                 WHERE rs.tool_id='antigravity' AND u.owner_device_id=?1
                   AND u.scope='device_local' AND u.is_deleted=0
                   AND (?2 IS NULL OR u.event_at_ms >= ?2) AND (?3 IS NULL OR u.event_at_ms < ?3)
                   AND rs.source_key IN (SELECT json_extract(value,'$[1]') FROM json_each(?4)
                     WHERE json_extract(value,'$[0]')='antigravity')
                 GROUP BY rs.source_key,u.model_id
                 ORDER BY 3 DESC",
            )?;
            let mut ag_rows =
                ag_models.query(params![owner_device_id, from_ms, to_ms, page_keys])?;
            while let Some(row) = ag_rows.next()? {
                let session_id: String = row.get(0)?;
                let model_id: Option<String> = row.get(1)?;
                let tokens: i64 = row.get(2)?;
                let count: i64 = row.get(3)?;
                by_session
                    .entry(("antigravity".into(), session_id))
                    .or_default()
                    .push(SessionModelUsage {
                        model_id,
                        total_tokens: pos_u64(tokens)?,
                        record_count: pos_u64(count)?,
                    });
            }
            for session in &mut sessions {
                if let Some(list) =
                    by_session.remove(&(session.tool_id.clone(), session.session_id.clone()))
                {
                    session.models = list;
                }
            }
        }
        Ok(SessionUsagePage {
            sessions,
            total: pos_u64(total)?,
            offset,
            limit,
        })
    }
}
