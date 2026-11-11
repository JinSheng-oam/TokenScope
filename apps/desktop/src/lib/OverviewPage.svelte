<script lang="ts">
  import { displayCurrency } from './displayCurrency';
import { formatCompactTokens, resolveSessionActivityState, findActivityForSession, type TrendViewMode } from '../lib/dashboardView';
  import UsageCurve from './UsageCurve.svelte';
  import UsageHeatmap from './UsageHeatmap.svelte';
  import { formatCost, formatNumber, formatToolLabel, percentOf, relativeTime } from '../lib/format';
  import type { ChartPoint } from '../lib/dashboardView';
  import type { HeatmapCalendar, HeatmapCell } from '../lib/heatmap';
  import { icons } from '../lib/icons';
  import RollingNumber from './RollingNumber.svelte';
  import { sessionRateLabel, sessionRateTitle, type SessionRates } from './sessionRates';
  import { liveNow } from '../lib/liveClock';
  import type { DashboardSnapshot, SessionActivity, SessionUsage, UsageBreakdown, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let sessionActivity: SessionActivity[] = [];
  export let sessionRates: SessionRates | null = null;
  export let local: UsageTotals | undefined;
  export let tools: UsageBreakdown[] | undefined = undefined;
  export let exactRatio: string;
  export let cacheHitRate: string;
  export let inputShareRate: string;
  export let outputShareRate: string;
  export let cacheShareRate: string;
  export let reasoningShareRate: string;
  export let cacheTokens: number;
  export let trendViewMode: TrendViewMode;
  export let hoveredCurveIndex: number | null;
  export let hoveredHeatmapCell: HeatmapCell | null;
  export let chartValues: DashboardSnapshot['daily'];
  export let chartMax: number;
  export let chartPoints: ChartPoint[];
  export let hoveredCurvePoint: ChartPoint | null;
  export let heatmapCalendar: HeatmapCalendar;
  export let topToolTotal: number;
  export let onSwitchTrendView: (mode: TrendViewMode) => void;
  export let onNavigateTools: () => void;
  export let onNavigateSessions: () => void;
  export let rollingNumbersEnabled = true;

  $: recentSessions = snapshot.session_usage.sessions.slice(0, 4);
  $: pendingActivity = sessionActivity.filter((item) => Boolean(item.display_name?.trim()) && $liveNow >= item.observed_at_ms && $liveNow - item.observed_at_ms <= 120_000 && !recentSessions.some((session) => findActivityForSession(session, sessionActivity)?.session_id === item.session_id)).slice(0, 4);
  $: visibleRecentSessions = recentSessions.slice(0, Math.max(0, 4 - pendingActivity.length));
  $: activeTools = tools ?? snapshot.by_tool;
  $: untimedRecordCount = activeTools.reduce((sum, tool) => sum + tool.untimed_records, 0);
  $: heatmapRangeLabel = `${heatmapCalendar.rangeStartDate} 至 ${heatmapCalendar.rangeEndDate}`;
</script>

<div class="overview-layout">
  <div class="page-header">
    <h1>使用总览</h1>
  </div>

  <!-- 4 Metric Cards -->
  <div class="metric-grid">
    <article class="metric-card hero-card">
      <div class="metric-head">
        <span class="metric-title">总 Token</span>
        <span class="precision-chip exact" title={`${exactRatio}% 的用量记录为精确精度`}>
          {@html icons.shield} 精确 {exactRatio}%
        </span>
      </div>
      <div class="metric-val">
        <RollingNumber value={local?.total_tokens} enabled={rollingNumbersEnabled} />
      </div>
      <div class="metric-split-bar" aria-label="Token 构成比例">
        <div class="metric-split-seg input" style={`width: ${inputShareRate}%`} title={`输入 ${inputShareRate}%`}></div>
        <div class="metric-split-seg output" style={`width: ${outputShareRate}%`} title={`输出 ${outputShareRate}%`}></div>
        <div class="metric-split-seg cache" style={`width: ${cacheShareRate}%`} title={`缓存 ${cacheShareRate}%`}></div>
      </div>
      <div class="metric-foot">
        <span>非缓存 <strong>{formatNumber(Math.max(0, (local?.total_tokens ?? 0) - cacheTokens))}</strong></span>
        {#if local?.estimated_cost_microusd != null}
          <span title="按公开基础费率估算。Antigravity 未分类 Token 按输入价；缺报价时参考 Gemini 3 Flash。">基础估算 <strong>{formatCost(local.estimated_cost_microusd, $displayCurrency)}</strong></span>
        {:else}
          <span class="metric-foot-sub">含缓存读写</span>
        {/if}
      </div>
    </article>

    <article class="metric-card">
      <div class="metric-head">
        <span class="metric-title">输入 Token</span>
        <span class="panel-tag tabular">占总量 {inputShareRate}%</span>
      </div>
      <div class="metric-val">
        <RollingNumber value={local?.input_tokens} enabled={rollingNumbersEnabled} />
      </div>
      <div class="metric-split-bar" aria-hidden="true">
        <div class="metric-split-seg input" style={`width: ${Math.min(100, Math.max(4, Number(inputShareRate) || 0))}%`}></div>
      </div>
      <div class="metric-foot">
        <span>净输入上下文</span>
        <span class="metric-foot-sub">不含缓存读写</span>
      </div>
    </article>

    <article class="metric-card">
      <div class="metric-head">
        <span class="metric-title">输出 Token</span>
        <span class="panel-tag tabular">推理 {reasoningShareRate}%</span>
      </div>
      <div class="metric-val">
        <RollingNumber value={local?.output_tokens} enabled={rollingNumbersEnabled} />
      </div>
      <div class="metric-split-bar" aria-hidden="true">
        <div class="metric-split-seg output" style={`width: ${Math.min(100, Math.max(4, Number(reasoningShareRate) || 0))}%`}></div>
      </div>
      <div class="metric-foot">
        <span>推理 <strong>{formatNumber(local?.reasoning_tokens)}</strong></span>
        <span class="metric-foot-sub">占总量 {outputShareRate}%</span>
      </div>
    </article>

    <article class="metric-card">
      <div class="metric-head">
        <span class="metric-title">缓存 Token</span>
        <span class="precision-chip exact" title={`输入缓存命中率 ${cacheHitRate}%`}>缓存率 {cacheHitRate}%</span>
      </div>
      <div class="metric-val">
        <RollingNumber value={cacheTokens} enabled={rollingNumbersEnabled} />
      </div>
      <div class="metric-split-bar" aria-hidden="true">
        <div class="metric-split-seg cache" style={`width: ${Math.min(100, Math.max(4, Number(cacheHitRate) || 0))}%`}></div>
      </div>
      <div class="metric-foot">
        <span>读取 <strong>{formatNumber(local?.cache_read_tokens)}</strong></span>
        <span>写入 <strong>{formatNumber(local?.cache_write_tokens)}</strong></span>
      </div>
    </article>
  </div>

  <!-- Data Precision Integrity Guardrail -->
  <div class="integrity-banner">
    <div class="integrity-left">
      <span class="integrity-item" class:is-zero={(local?.exact_records ?? 0) === 0}>
        <span class="integrity-dot exact"></span>精确记录 <strong>{formatNumber(local?.exact_records)}</strong> 笔
      </span>
      <span class="integrity-item" class:is-zero={(local?.aggregate_records ?? 0) === 0}>
        <span class="integrity-dot aggregate"></span>聚合粗粒度 <strong>{formatNumber(local?.aggregate_records)}</strong> 笔
      </span>
      <span class="integrity-item" class:is-zero={(local?.estimated_records ?? 0) === 0}>
        <span class="integrity-dot estimated"></span>显式估算 <strong>{formatNumber(local?.estimated_records)}</strong> 笔
      </span>
      {#if (local?.unknown_precision_records ?? 0) > 0}
        <span class="integrity-item">
          <span class="integrity-dot muted"></span>精度未知 <strong>{formatNumber(local?.unknown_precision_records)}</strong> 笔
        </span>
      {/if}
      {#if (local?.priced_records ?? 0) > 0}
        <span class="integrity-item">
          计价 <strong>{formatNumber(local?.priced_records)} / {formatNumber((local?.priced_records ?? 0) + (local?.unpriced_records ?? 0))}</strong> 笔
        </span>
      {/if}
    </div>
    <div class="integrity-right">
      {#if untimedRecordCount > 0}
        <span class="warning" title="已计入累计但不进入日趋势">
          时间未知 {formatNumber(untimedRecordCount)} 条（不入日趋势）
        </span>
      {/if}
      {#if snapshot.unassigned.total_tokens > 0}
        <span>未归属 {formatNumber(snapshot.unassigned.total_tokens)} Token</span>
      {:else if untimedRecordCount === 0}
        <span class="integrity-clean-badge">时间戳与归属校验完整 · 共 {formatNumber(local?.record_count)} 笔有效记录</span>
      {/if}
    </div>
  </div>

          <!-- Trend Chart & Tool Rankings (Two Column Grid) -->
          <div class="two-col-grid">
            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>Token 使用趋势</h2>
                  <p>
                    {trendViewMode === 'heatmap'
                      ? `完整历史 · ${heatmapCalendar.weeks.length} 周 · ${heatmapRangeLabel}`
                      : '按本机时区自然日汇集（悬浮或用方向键查看当日明细）'}
                  </p>
                </div>
                <div class="panel-head-actions">
                  <span class="panel-tag">
                    {trendViewMode === 'heatmap'
                      ? `活跃 ${heatmapCalendar.activeDays} 天`
                      : `${snapshot.daily.length} 天样本`}
                  </span>
                  <div class="trend-mode-switch" role="group" aria-label="趋势图显示模式">
                    <button
                      class="trend-mode-btn"
                      class:selected={trendViewMode === 'curve'}
                      aria-pressed={trendViewMode === 'curve'}
                      onclick={() => onSwitchTrendView('curve')}
                    >
                      曲线图
                    </button>
                    <button
                      class="trend-mode-btn"
                      class:selected={trendViewMode === 'heatmap'}
                      aria-pressed={trendViewMode === 'heatmap'}
                      onclick={() => onSwitchTrendView('heatmap')}
                    >
                      热力图
                    </button>
                  </div>
                </div>
              </div>

              {#if trendViewMode === 'curve'}
                <UsageCurve bind:hoveredCurveIndex {hoveredCurvePoint} {chartValues} {chartMax} {chartPoints} {inputShareRate} {outputShareRate} {cacheShareRate} />
              {:else}
                <UsageHeatmap {heatmapCalendar} bind:hoveredHeatmapCell />
              {/if}
            </section>

            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>工具用量分布</h2>
                </div>
                <button class="panel-tag" onclick={() => (onNavigateTools())}>查看明细 →</button>
              </div>

              {#if activeTools.length > 0}
                <div class="rank-list">
                  {#each activeTools.slice(0, 5) as tool, idx}
                    <div class="rank-item">
                      <div class="rank-row">
                        <span class="rank-title">
                          <span class="rank-num">0{idx + 1}</span>
                          {tool.label}
                        </span>
                        <span class="rank-meta">
                          <span class="rank-value tabular">{formatNumber(tool.total_tokens)}</span>
                          <span class="rank-share tabular">{percentOf(tool.total_tokens, local?.total_tokens ?? topToolTotal).toFixed(1)}%</span>
                        </span>
                      </div>
                      <div class="rank-track">
                        <div class="rank-fill" style={`width: ${percentOf(tool.total_tokens, topToolTotal)}%`}></div>
                      </div>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">暂无工具消耗记录</div>
              {/if}
            </section>
          </div>

          <div class="overview-support-grid">
            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>最近会话</h2>
                </div>
                <button class="panel-tag" onclick={onNavigateSessions}>查看全部 →</button>
              </div>

              {#if pendingActivity.length > 0 || visibleRecentSessions.length > 0}
                <div class="recent-session-list">
                  {#each pendingActivity as activity, index (`${activity.tool_id}:${activity.session_id}`)}
                    {@const rate = sessionRateLabel(sessionRates, activity, $liveNow)}
                    <div class="recent-session-row">
                      <span class="recent-session-rank tabular">{String(index + 1).padStart(2, '0')}</span>
                      <span class="recent-session-main">
                        <strong title={activity.display_name ?? '未命名会话'}>{activity.display_name || '未命名会话'}</strong>
                        <small>{[formatToolLabel(activity.tool_id), activity.model_id, relativeTime(activity.observed_at_ms)].filter(Boolean).join(' · ')}</small>
                      </span>
                      <span class="recent-session-right">
                        {#if activity.state === 'generating'}
                          <span class="recent-session-status generating" title="任务日志仍在更新，等待结束事件">
                            <span class="recent-session-spinner" aria-hidden="true"></span>进行中
                          </span>
                        {:else}
                          <span class="recent-session-status recent" title="最近检测到会话活动">
                            <span class="recent-session-live-dot" aria-hidden="true"></span>近期活跃
                          </span>
                        {/if}
                        <span class="recent-session-pending-value">
                          用量待更新
                          {#if rate}<small class="session-token-rate" title={sessionRateTitle}>{rate}</small>{/if}
                        </span>
                      </span>
                    </div>
                  {/each}
                  {#each visibleRecentSessions as session, index}
                    {@const actState = resolveSessionActivityState(session, sessionActivity, $liveNow)}
                    {@const rate = sessionRateLabel(sessionRates, session, $liveNow)}
                    <div class="recent-session-row">
                      <span class="recent-session-rank tabular">{String(index + pendingActivity.length + 1).padStart(2, '0')}</span>
                      <span class="recent-session-main">
                        <strong title={session.display_name ?? session.session_id}>
                          {session.display_name ?? '未命名会话'}
                        </strong>
                        <small>
                          {[
                            formatToolLabel(session.tool_id),
                            session.models[0]?.model_id,
                            relativeTime(session.last_event_at_ms ?? session.activity_observed_at_ms),
                          ]
                            .filter(Boolean)
                            .join(' · ')}
                        </small>
                      </span>
                      <span class="recent-session-right">
                        {#if actState === 'generating'}
                          <span class="recent-session-status generating" title="任务日志仍在更新，等待结束事件">
                            <span class="recent-session-spinner" aria-hidden="true"></span>进行中
                          </span>
                        {:else if actState === 'recent'}
                          <span class="recent-session-status recent" title="最近检测到会话活动">
                            <span class="recent-session-live-dot" aria-hidden="true"></span>近期活跃
                          </span>
                        {/if}
                        <span class="recent-session-value tabular">
                          {formatNumber(session.total_tokens)}
                          {#if rate}<small class="session-token-rate" title={sessionRateTitle}>{rate}</small>{/if}
                        </span>
                      </span>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">当前范围暂无会话记录</div>
              {/if}
            </section>

            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>采集源状态</h2>
                </div>
                <button class="panel-tag" onclick={onNavigateTools}>运行明细 →</button>
              </div>

              {#if snapshot.source_health.length > 0}
                <div class="source-health-list">
                  {#each snapshot.source_health as source}
                    <div class="source-health-row">
                      <span
                        class="source-status-dot"
                        class:complete={source.status === 'complete'}
                        class:partial={source.status === 'partial'}
                        class:degraded={source.status === 'degraded'}
                      ></span>
                      <span class="source-health-name">{formatToolLabel(source.tool_id)}</span>
                      <span class="source-health-meta">
                        {relativeTime(source.last_scan_ms)} · {source.source_count} 个来源
                      </span>
                      <span
                        class="source-health-status"
                        class:complete={source.status === 'complete'}
                        class:partial={source.status === 'partial'}
                        class:degraded={source.status === 'degraded'}
                      >
                        {source.status === 'complete' ? '正常' : source.status === 'partial' ? '部分' : source.status === 'degraded' ? '异常' : '未知'}
                      </span>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">暂无采集来源检查点</div>
              {/if}
            </section>
          </div>
</div>
