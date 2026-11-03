mod query;
mod watch;

use crate::state::DashboardWatch;
use query::{query_agent, query_agent_session_analysis, request_agent_session_delete};
use tauri::{AppHandle, Manager};
use tokenscope_local_ipc::{DashboardResponse, SessionAnalysisResponse, SessionDeleteResponse};

pub(crate) use query::query_agent_fresh;
pub(crate) use watch::{spawn_dashboard_warmup, spawn_dashboard_watch};
#[tauri::command]
pub(crate) async fn delete_session(
    app: AppHandle,
    tool_id: String,
    session_id: String,
) -> Result<SessionDeleteResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let response = request_agent_session_delete(&app, &tool_id, &session_id)?;
        if let Ok(mut cache) = app.state::<DashboardWatch>().cache.lock() {
            cache.clear();
        }
        Ok(response)
    })
    .await
    .map_err(|e| format!("会话删除任务失败：{e}"))?
}

#[tauri::command]
pub(crate) async fn get_dashboard_snapshot(
    app: AppHandle,
    range: String,
    session_offset: u64,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<DashboardResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        query_agent(
            &app,
            &range,
            session_offset,
            start_date.as_deref(),
            end_date.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("Dashboard 查询任务失败：{e}"))?
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub(crate) async fn get_session_analysis(
    app: AppHandle,
    range: String,
    session_offset: u64,
    search: Option<String>,
    tool_id: Option<String>,
    model_id: Option<String>,
    project_id: Option<String>,
    start_date: Option<String>,
    end_date: Option<String>,
) -> Result<SessionAnalysisResponse, String> {
    tauri::async_runtime::spawn_blocking(move || {
        query_agent_session_analysis(
            &app,
            &range,
            session_offset,
            search.as_deref(),
            tool_id.as_deref(),
            model_id.as_deref(),
            project_id.as_deref(),
            start_date.as_deref(),
            end_date.as_deref(),
        )
    })
    .await
    .map_err(|e| format!("会话查询任务失败：{e}"))?
}
