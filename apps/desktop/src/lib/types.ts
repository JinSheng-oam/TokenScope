export type UsageTotals = {
  total_tokens: number;
  input_tokens: number;
  cache_read_tokens: number;
  cache_write_tokens: number;
  output_tokens: number;
  unclassified_tokens: number;
  reasoning_tokens: number;
  reasoning_known_records: number;
  record_count: number;
  estimated_cost_microusd: number | null;
  priced_records: number;
  unpriced_records: number;
  exact_records: number;
  aggregate_records: number;
  estimated_records: number;
  unknown_precision_records: number;
};

export type UsageBreakdown = {
  key: string;
  label: string;
  total_tokens: number;
  record_count: number;
  estimated_cost_microusd: number | null;
  exact_records: number;
  aggregate_records: number;
  estimated_records: number;
  unknown_precision_records: number;
  untimed_records: number;
};

export type CodexQuota = {
  source_id: string;
  pool_id: string;
  window_duration_minutes: number;
  remaining_basis_points: number | null;
  reset_at_ms: number | null;
  observed_at_ms: number;
};

export type CodexQuotaRefreshResult = {
  snapshots: CodexQuota[];
  persisted: boolean;
  warning: string | null;
};

export type SessionActivity = {
  tool_id: string;
  session_id: string;
  state: 'generating' | 'recent';
  observed_at_ms: number;
  display_name: string | null;
  model_id: string | null;
};

export type ModelActivityBreakdown = {
  tool_id: string;
  model_id: string;
  response_count: number;
  time_scope: 'selected_range' | 'all_time';
};

export type RecentSessionActivity = {
  sessions: SessionActivity[];
  sampled_at_ms: number;
  rates: { tool_id: string; session_id: string; tokens: number }[];
};

export type SessionUsage = {
  session_id: string;
  tool_id: string;
  total_tokens: number;
  input_uncached_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  usage_record_count: number;
  exact_records: number;
  aggregate_records: number;
  estimated_records: number;
  unknown_precision_records: number;
  untimed_records: number;
  model_count: number;
  known_response_count: number | null;
  unknown_response_count: number | null;
  last_event_at_ms: number | null;
  activity_observed_at_ms: number | null;
  display_name: string | null;
  project_ids: string[];
  models: SessionModelUsage[];
};

export type SessionModelUsage = {
  model_id: string | null;
  total_tokens: number;
  record_count: number;
};

export type SessionFilterOption = {
  key: string;
  label: string;
  session_count: number;
};

export type SessionFilterOptions = {
  tools: SessionFilterOption[];
  models: SessionFilterOption[];
  projects: SessionFilterOption[];
};

export type SessionUsagePage = {
  sessions: SessionUsage[];
  total: number;
  offset: number;
  limit: number;
};

export type SessionAnalysisPage = {
  page: SessionUsagePage;
  filter_options: SessionFilterOptions;
};

export type SessionAnalysisResponse = {
  protocol_version: number;
  analysis: SessionAnalysisPage;
};

export type RemoteUsageBreakdown = UsageBreakdown & { owner_device_id: string };
export type RemoteUsageTotals = UsageTotals;
export type RemoteDeviceUsage = {
  owner_device_id: string;
  total_tokens: number;
  record_count: number;
  estimated_cost_microusd: number | null;
  usage: RemoteUsageTotals;
};

export type DashboardSnapshot = {
  data_revision: number;
  schema_version: number;
  generated_at_ms: number;
  range: 'today' | '7d' | '30d' | 'all' | 'custom';
  range_start_ms: number | null;
  range_end_ms: number | null;
  time_zone: string;
  time_zone_valid: boolean;
  identity: { device_id: string; display_name: string; time_zone: string } | null;
  local: UsageTotals;
  remote: RemoteUsageTotals;
  remote_by_device: RemoteDeviceUsage[];
  remote_by_tool: RemoteUsageBreakdown[];
  remote_by_model: RemoteUsageBreakdown[];
  remote_by_project: RemoteUsageBreakdown[];
  unassigned: UsageTotals;
  account_global: UsageTotals;
  by_tool: UsageBreakdown[];
  by_model: UsageBreakdown[];
  model_activity: ModelActivityBreakdown[];
  by_project: UsageBreakdown[];
  session_usage: {
    sessions: SessionUsage[];
    total: number;
    offset: number;
    limit: number;
  };
  daily: { date: string; total_tokens: number; input_tokens: number; output_tokens: number }[];
  remote_daily: (DashboardSnapshot['daily'][number] & { owner_device_id: string })[];
  source_health: {
    tool_id: string;
    last_scan_ms: number | null;
    last_complete_scan_ms: number | null;
    status: 'complete' | 'partial' | 'degraded' | 'unknown';
    source_count: number;
  }[];
  antigravity_quota: {
    source_id: string;
    pool_id: string;
    window_id: 'five_hour' | 'weekly' | 'daily' | 'other';
    remaining_basis_points: number | null;
    reset_at_ms: number | null;
    observed_at_ms: number;
  }[];
  codex_quota: CodexQuota[];
  antigravity_activity: {
    known_turns: number;
    unknown_turns: number;
    conversations: number;
    by_model: [string, number][];
  };
  sync_status: {
    pending: number;
    deferred: number;
    blocked: number;
    oldest_pending_at_ms: number | null;
    next_attempt_at_ms: number | null;
    last_attempt_ms: number | null;
    last_success_ms: number | null;
    last_pull_seq: number;
    last_pull_ms: number | null;
    last_status: 'never' | 'idle' | 'success' | 'deferred' | 'blocked' | 'auth_required' | 'error';
    last_error: string | null;
  };
};

export type AgentStatus = {
  running: boolean;
  binaryFound: boolean;
  databaseReady: boolean;
  message: string;
};

export type DiagnosticSeverity = 'ok' | 'info' | 'warning' | 'error';

export type DiagnosticCheck = {
  id: string;
  title: string;
  severity: DiagnosticSeverity;
  detail: string;
  action: string | null;
};

export type DiagnosticToolUsage = {
  tool_id: string;
  total_tokens: number;
  record_count: number;
  last_event_at_ms: number | null;
};

export type DiagnosticFacts = {
  database_path: string;
  database_bytes: number;
  wal_bytes: number;
  schema_version: number | null;
  expected_schema_version: number;
  device_id: string | null;
  marker_present: boolean;
  record_count: number;
  total_tokens: number;
  source_count: number;
  last_event_at_ms: number | null;
  last_scan_at_ms: number | null;
  free_disk_bytes: number | null;
  agent_running: boolean;
  agent_path: string | null;
  legacy_database_path: string | null;
  log_bytes: number;
  log_modified_at_ms: number | null;
  sync_pending: number;
  sync_deferred: number;
  sync_blocked: number;
  sync_last_status: string;
  tools: DiagnosticToolUsage[];
};

export type DiagnosticReport = {
  generated_at_ms: number;
  overall: DiagnosticSeverity;
  checks: DiagnosticCheck[];
  facts: DiagnosticFacts;
};

export type DashboardRange = 'today' | '7d' | '30d' | 'all' | 'custom';

export type DashboardResponse = {
  request_key: string;
  protocol_version: number;
  snapshot: DashboardSnapshot;
};


export type RemoteDevice = {
  device_id: string;
  display_name: string;
  os_name: 'windows' | 'macos' | 'linux' | 'other';
  app_version: string;
  time_zone: string;
  created_at_ms: number;
  last_seen_ms: number | null;
  revoked_at_ms: number | null;
};

export type RemoteDeviceList = {
  protocol_version: number;
  devices: RemoteDevice[];
};

export type PairHubResult = {
  hubOrigin: string;
  deviceId: string;
  createdAtMs: number;
};

export type DashboardObservations = Pick<DashboardSnapshot, 'source_health' | 'codex_quota' | 'antigravity_quota' | 'sync_status'> & {
  observed_at_ms: number;
};
