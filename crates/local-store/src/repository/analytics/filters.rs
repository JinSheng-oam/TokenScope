use super::super::models::{tool_display_name, SessionFilterOption, SessionFilterOptions};
use super::super::LocalStore;
use super::SESSION_LINKS_CTE;
use crate::error::{pos_u64, Result};
use rusqlite::params;
impl LocalStore {
    pub(super) fn session_filter_options(
        &self,
        owner_device_id: &str,
        from_ms: Option<i64>,
        to_ms: Option<i64>,
    ) -> Result<SessionFilterOptions> {
        let sql = format!(
            r#"{SESSION_LINKS_CTE}
            SELECT dimension,facet_key,COUNT(*) FROM (
                SELECT 'tool' AS dimension,tool_id AS facet_key,tool_id,session_id
                FROM (SELECT DISTINCT tool_id,session_id FROM session_links)
                UNION ALL
                SELECT 'model',model_key,tool_id,session_id
                FROM (
                    SELECT DISTINCT tool_id,session_id,
                        COALESCE(model_id,'unknown-model') AS model_key
                    FROM session_links
                )
                UNION ALL
                SELECT 'project',project_key,tool_id,session_id
                FROM (
                    SELECT DISTINCT tool_id,session_id,project_id AS project_key
                    FROM session_links WHERE project_id IS NOT NULL
                    UNION ALL
                    SELECT tool_id,session_id,'no-project'
                    FROM session_links
                    GROUP BY tool_id,session_id
                    HAVING COUNT(project_id)=0
                )
            ) AS facets
            GROUP BY dimension,facet_key
            ORDER BY dimension,3 DESC,2"#
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(params![owner_device_id, from_ms, to_ms], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let mut options = SessionFilterOptions::default();
        for row in rows {
            let (dimension, key, session_count) = row?;
            let session_count = pos_u64(session_count)?;
            let option = match dimension.as_str() {
                "tool" => SessionFilterOption {
                    label: tool_display_name(&key),
                    key,
                    session_count,
                },
                "model" => SessionFilterOption {
                    label: if key == "unknown-model" {
                        "来源未提供模型".into()
                    } else {
                        key.clone()
                    },
                    key,
                    session_count,
                },
                "project" => SessionFilterOption {
                    label: if key == "no-project" {
                        "未关联项目".into()
                    } else {
                        self.project_display_name(&key)?
                            .unwrap_or_else(|| "项目名称未提供".into())
                    },
                    key,
                    session_count,
                },
                _ => continue,
            };
            match dimension.as_str() {
                "tool" => options.tools.push(option),
                "model" => options.models.push(option),
                "project" => options.projects.push(option),
                _ => {}
            }
        }
        Ok(options)
    }
}
