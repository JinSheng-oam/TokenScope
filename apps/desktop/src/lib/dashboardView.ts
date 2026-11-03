import { formatNumber } from './format';
import { buildHeatmapCalendar, formatWeekday } from './heatmap';
import type { DashboardRange, DashboardSnapshot, UsageBreakdown, UsageTotals } from './types';

export type TrendViewMode = 'curve' | 'heatmap';
export type ModelSortBy = 'tokens' | 'cost';

export type DashboardProjectionOptions = {
  snapshot: DashboardSnapshot | null;
  heatmapDaily: DashboardSnapshot['daily'] | null;
  todayIso: string;
  hoveredCurveIndex: number | null;
  remoteSelectedDevice: string;
  modelSortBy: ModelSortBy;
  range: DashboardRange;
  customStart: string;
  customEnd: string;
};

export type ChartPoint = DashboardSnapshot['daily'][number] & {
  cache_tokens: number;
  weekdayLabel: string;
  x: number;
  y: number;
};

export function buildDashboardProjection(options: DashboardProjectionOptions) {
  const { snapshot } = options;
  const local = snapshot?.local;
  const chartValues = snapshot?.daily ?? [];
  const chartPoints = buildChartPoints(chartValues);
  const tokenShares = getTokenShares(local);
  const remoteFilter = (row: { owner_device_id: string }) =>
    options.remoteSelectedDevice === 'all' || row.owner_device_id === options.remoteSelectedDevice;
  const sessionRangeLabel = buildRangeLabel(options.range, options.customStart, options.customEnd);

  return {
    local,
    chartValues,
    chartMax: Math.max(1, ...chartValues.map((item) => item.total_tokens)),
    chartPoints,
    chartPath: buildChartPath(chartPoints),
    hoveredCurvePoint:
      options.hoveredCurveIndex !== null && chartPoints[options.hoveredCurveIndex]
        ? chartPoints[options.hoveredCurveIndex]
        : null,
    heatmapCalendar: buildHeatmapCalendar(
      options.heatmapDaily ?? chartValues,
      options.todayIso || chartValues[chartValues.length - 1]?.date || '',
    ),
    topToolTotal: Math.max(1, ...(snapshot?.by_tool ?? []).map((item) => item.total_tokens)),
    topModelTotal: Math.max(1, ...(snapshot?.by_model ?? []).map((item) => item.total_tokens)),
    antigravityUsage: (snapshot?.by_tool ?? []).find((item) => item.key === 'antigravity'),
    remoteToolRows: (snapshot?.remote_by_tool ?? []).filter(remoteFilter),
    remoteModelRows: (snapshot?.remote_by_model ?? []).filter(remoteFilter),
    sortedModels: sortModelsBy(snapshot?.by_model ?? [], options.modelSortBy),
    tokenShares,
    exactRatio: local?.record_count
      ? ((local.exact_records / local.record_count) * 100).toFixed(1)
      : '—',
    rangeLabel: sessionRangeLabel,
  };
}

export function buildChartPoints(daily: DashboardSnapshot['daily']): ChartPoint[] {
  const chartMax = Math.max(1, ...daily.map((item) => item.total_tokens));
  return daily.map((item, index) => ({
    ...item,
    cache_tokens: Math.max(0, item.total_tokens - item.input_tokens - item.output_tokens),
    weekdayLabel: formatWeekday(item.date),
    x: daily.length <= 1 ? 420 : 64 + (index / (daily.length - 1)) * 708,
    y: 156 - (item.total_tokens / chartMax) * 124,
  }));
}

export function buildChartPath(points: ChartPoint[]): string {
  return points.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x} ${point.y}`).join(' ');
}

export function formatCompactTokens(value: number): string {
  if (value >= 1_000_000_000) return `${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(Math.round(value));
}

export function buildRangeLabel(
  range: DashboardRange,
  customStart: string,
  customEnd: string
): string {
  return range === 'all'
    ? '全部'
    : range === 'today'
      ? '今天'
      : range === '7d'
        ? '7 天'
        : range === '30d'
          ? '30 天'
          : `${customStart} 至 ${customEnd}`;
}

export function sortModelsBy(
  models: UsageBreakdown[],
  sortBy: ModelSortBy
): UsageBreakdown[] {
  return [...models].sort((a, b) => {
    if (sortBy === 'cost') {
      return (b.estimated_cost_microusd ?? 0) - (a.estimated_cost_microusd ?? 0);
    }
    return b.total_tokens - a.total_tokens;
  });
}

export function getTokenShares(local: UsageTotals | undefined): {
  cacheTokens: number;
  cacheHitRate: string;
  inputShareRate: string;
  outputShareRate: string;
  cacheShareRate: string;
  reasoningShareRate: string;
} {
  const cacheTokens = (local?.cache_read_tokens ?? 0) + (local?.cache_write_tokens ?? 0);
  const cacheInputTokens =
    (local?.input_tokens ?? 0) +
    (local?.cache_read_tokens ?? 0) +
    (local?.cache_write_tokens ?? 0);
  const total = local?.total_tokens ?? 0;
  return {
    cacheTokens,
    cacheHitRate: cacheInputTokens
      ? (((local?.cache_read_tokens ?? 0) / cacheInputTokens) * 100).toFixed(1)
      : '0.0',
    inputShareRate: percentShare(local?.input_tokens ?? 0, total),
    outputShareRate: percentShare(local?.output_tokens ?? 0, total),
    cacheShareRate: percentShare(cacheTokens, total),
    reasoningShareRate: percentShare(local?.reasoning_tokens ?? 0, local?.output_tokens ?? 0),
  };
}

function percentShare(value: number, total: number): string {
  return total ? ((value / total) * 100).toFixed(1) : '0.0';
}

export function quotaWindowLabel(durationMinutes: number): string {
  if (durationMinutes === 300) return '5 小时周期';
  if (durationMinutes === 10_080) return '每周周期';
  if (durationMinutes === 1_440) return '每日周期';
  return `${formatNumber(durationMinutes)} 分钟周期`;
}

export function antigravityWindowLabel(windowId: string): string {
  if (windowId === 'five_hour') return '5 小时周期';
  if (windowId === 'weekly') return '每周周期';
  if (windowId === 'daily') return '每日周期';
  return '其他周期';
}

export function quotaIsStale(observedAtMs: number, now = Date.now()): boolean {
  return now - observedAtMs > 15 * 60_000;
}

export function formatCountdown(resetAtMs: number | null, now = Date.now()): string {
  if (resetAtMs === null) return '未提供重置时间';
  const diff = resetAtMs - now;
  if (diff <= 0) return '即将重置';
  const hours = Math.floor(diff / 3_600_000);
  const mins = Math.floor((diff % 3_600_000) / 60_000);
  const secs = Math.floor((diff % 60_000) / 1000);
  return `${hours.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')} 后重置`;
}
