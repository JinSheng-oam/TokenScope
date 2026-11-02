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

export type ModelActivityBreakdown = {
  tool_id: string;
  model_id: string;
  response_count: number;
  time_scope: 'selected_range' | 'all_time';
};

export type SessionUsage = {
  session_id: string;
  tool_id: string;
  total_tokens: number;
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
};

export type RemoteUsageBreakdown = {
  owner_device_id: string;
  key: string;
  label: string;
  total_tokens: number;
  record_count: number;
  estimated_cost_microusd: number | null;
};

export type DashboardSnapshot = {
  schema_version: number;
  generated_at_ms: number;
  range: 'today' | '7d' | '30d' | 'all';
  range_start_ms: number | null;
  range_end_ms: number | null;
  time_zone: string;
  time_zone_valid: boolean;
  identity: { device_id: string; display_name: string; time_zone: string } | null;
  local: UsageTotals;
  remote: {
    total_tokens: number; input_tokens: number; cache_read_tokens: number; cache_write_tokens: number;
    output_tokens: number; unclassified_tokens: number; record_count: number; priced_records: number; unpriced_records: number;
    exact_records: number; aggregate_records: number; estimated_records: number; unknown_precision_records: number; estimated_cost_microusd: number | null;
  };
  remote_by_device: { owner_device_id: string; total_tokens: number; record_count: number; estimated_cost_microusd: number | null }[];
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

export type DashboardRange = 'today' | '7d' | '30d' | 'all';

export type DashboardResponse = {
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
