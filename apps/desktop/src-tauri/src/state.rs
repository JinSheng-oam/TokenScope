use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    process::Child,
    sync::{
        atomic::{AtomicBool, AtomicU64},
        Mutex,
    },
    time::Instant,
};
use tokenscope_local_ipc::DashboardResponse;
use tokenscope_local_store::SessionFilterOptions;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AgentStatus {
    pub(crate) running: bool,
    pub(crate) binary_found: bool,
    pub(crate) database_ready: bool,
    pub(crate) message: String,
}

/// Owns only the Agent process started by this desktop instance.
/// An independently launched CLI Agent is never killed by the desktop.
#[derive(Default)]
pub(crate) struct AgentProcess {
    pub(crate) child: Mutex<Option<Child>>,
}

#[derive(Default)]
pub(crate) struct AppLifecycle {
    pub(crate) quit_requested: AtomicBool,
    pub(crate) keep_alive_after_window_close: AtomicBool,
}

/// Remembers which range the UI is showing so background refreshes never
/// replace data from a different selection.
#[derive(Clone, Default)]
pub(crate) struct DashboardRequest {
    pub(crate) range: String,
    pub(crate) start_date: Option<String>,
    pub(crate) end_date: Option<String>,
}

#[derive(Default)]
pub(crate) struct DashboardWatch {
    pub(crate) revision: AtomicU64,
    pub(crate) request: Mutex<DashboardRequest>,
    pub(crate) cache: Mutex<HashMap<String, (DashboardResponse, Instant)>>,
    pub(crate) session_filter_options: Mutex<HashMap<String, SessionFilterOptions>>,
    pub(crate) refreshing: Mutex<HashSet<String>>,
}
