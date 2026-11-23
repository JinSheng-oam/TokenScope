<script lang="ts">
  import { displayCurrency } from './displayCurrency';
  import { formatCompactTokens, resolveSessionActivityState } from './dashboardView';
  import { formatCost, formatNumber, formatToolLabel, percentOf, relativeTime } from './format';
  import type { HeatmapCalendar } from './heatmap';
  import { liveMinute } from './liveClock';
  import { sessionRateLabel, sessionRateTitle, type SessionRates } from './sessionRates';
  import type { CardId } from './dashboardLayout';
  import type { CodexQuota, DashboardSnapshot, RemoteDevice, SessionActivity, SessionUsage, UsageBreakdown, UsageTotals } from './types';
  import QuotaMonitorCard from './cards/QuotaMonitorCard.svelte';
  import QuotaResetsCard from './cards/QuotaResetsCard.svelte';
  import CostBreakdownCard from './cards/CostBreakdownCard.svelte';
  import EfficiencyCard from './cards/EfficiencyCard.svelte';
  import DevicesSyncCard from './cards/DevicesSyncCard.svelte';
  import ActivitySummaryCard from './cards/ActivitySummaryCard.svelte';

  let {
    cardId,
    cardRows = 5,
    snapshot,
    local,
    activeTools = [],
    topToolTotal = 1,
    sortedModels = [],
    allModelTokens = 0,
    topModelTotal = 1,
    pendingActivity = [],
    visibleRecentSessions = [],
    sessionActivity = [],
    sessionRates = null,
    visibleProjects = [],
    allProjectTokens = 0,
    topProjectTotal = 1,
    activeAntigravityQuotas = [],
    activeCodexQuotas = [],
    codexQuotaRefreshing = false,
    onRefreshCodexQuota = null,
    remoteDevices = [],
    heatmapCalendar = null,
    cacheHitRate = '0',
    inputShareRate = '0',
    outputShareRate = '0',
    cacheShareRate = '0',
    reasoningShareRate = '0',
    cacheTokens = 0,
    onNavigateTools = () => {},
    onNavigateModels = () => {},
    onNavigateSessions = () => {},
    onNavigateProjects = () => {},
    onNavigateQuota = () => {},
    onNavigateDevices = () => {},
    onNavigatePricing = () => {},
  }: {
    cardId: CardId;
    cardRows?: number;
    snapshot: DashboardSnapshot;
    local: UsageTotals | undefined;
    activeTools?: UsageBreakdown[];
    topToolTotal?: number;
    sortedModels?: UsageBreakdown[];
    allModelTokens?: number;
    topModelTotal?: number;
    pendingActivity?: SessionActivity[];
    visibleRecentSessions?: SessionUsage[];
    sessionActivity?: SessionActivity[];
    sessionRates?: SessionRates | null;
    visibleProjects?: UsageBreakdown[];
    allProjectTokens?: number;
    topProjectTotal?: number;
    activeAntigravityQuotas?: DashboardSnapshot['antigravity_quota'];
    activeCodexQuotas?: CodexQuota[];
    codexQuotaRefreshing?: boolean;
    onRefreshCodexQuota?: (() => void) | null;
    remoteDevices?: RemoteDevice[];
    heatmapCalendar?: HeatmapCalendar | null;
    cacheHitRate?: string;
    inputShareRate?: string;
    outputShareRate?: string;
    cacheShareRate?: string;
    reasoningShareRate?: string;
    cacheTokens?: number;
    onNavigateTools?: () => void;
    onNavigateModels?: () => void;
    onNavigateSessions?: () => void;
    onNavigateProjects?: () => void;
    onNavigateQuota?: () => void;
    onNavigateDevices?: () => void;
    onNavigatePricing?: () => void;
  } = $props();

  const modelMillionsFormat = new Intl.NumberFormat('en-US', { maximumFractionDigits: 1, useGrouping: false });

  function formatNarrowModelTokens(tokens: number): string {
    return tokens >= 1_000_000 ? `${modelMillionsFormat.format(tokens / 1_000_000)}M` : formatCompactTokens(tokens);
  }

  function compactModelLabel(label: string): string {
    const [provider, model, ...rest] = label.split('/');
    return provider && model && rest.length === 0 && model.toLowerCase().startsWith(`${provider.toLowerCase()}-`)
      ? model
      : label;
  }
</script>

{#if cardId === 'tools'}
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>工具用量分布</h2>
      </div>
      <button type="button" class="panel-tag" onclick={onNavigateTools}>查看明细 →</button>
    </div>

    {#if activeTools.length > 0}
      <div class="rank-list">
        {#each activeTools.slice(0, cardRows) as tool, idx}
          <div class="rank-item">
            <div class="rank-row">
              <span class="rank-title">
                <span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span>
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

{:else if cardId === 'models'}
  <section class="panel-card model-usage-card">
    <div class="panel-head">
      <div>
        <h2>模型用量分布</h2>
      </div>
      <button type="button" class="panel-tag" onclick={onNavigateModels}>查看明细 →</button>
    </div>

    {#if sortedModels.length > 0}
      <div class="rank-list">
        {#each sortedModels.slice(0, cardRows) as model, idx}
          {@const shortLabel = compactModelLabel(model.label)}
          <div class="rank-item">
            <div class="rank-row">
              <span class="rank-title">
                <span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span>
                <strong title={model.label}>
                  {#if shortLabel === model.label}
                    {model.label}
                  {:else}
                    <span class="model-label-full">{model.label}</span>
                    <span class="model-label-compact" aria-hidden="true">{shortLabel}</span>
                  {/if}
                </strong>
              </span>
              <span class="rank-meta">
                <span class="rank-value tabular" title={`${formatNumber(model.total_tokens)} Token`}>
                  <span class="model-tokens-full">{formatNumber(model.total_tokens)}</span>
                  <span class="model-tokens-compact" aria-hidden="true">{formatNarrowModelTokens(model.total_tokens)}</span>
                </span>
                {#if model.estimated_cost_microusd != null}
                  {@const cost = formatCost(model.estimated_cost_microusd, $displayCurrency)}
                  <span class="panel-tag small-cost-tag tabular" title={`预估费用：${cost}`}>
                    {cost}
                  </span>
                {/if}
                <span class="rank-share tabular">{percentOf(model.total_tokens, allModelTokens).toFixed(1)}%</span>
              </span>
            </div>
            <div class="rank-track">
              <div class="rank-fill model-fill" style={`width: ${percentOf(model.total_tokens, topModelTotal)}%`}></div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-box">暂无模型消耗记录</div>
    {/if}
  </section>

{:else if cardId === 'sessions'}
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>最近会话</h2>
      </div>
      <button type="button" class="panel-tag" onclick={onNavigateSessions}>查看全部 →</button>
    </div>

    {#if pendingActivity.length > 0 || visibleRecentSessions.length > 0}
      <div class="recent-session-list">
        {#each pendingActivity as activity, index (`${activity.tool_id}:${activity.session_id}`)}
          {@const rate = sessionRateLabel(sessionRates, activity, $liveMinute)}
          <div class="recent-session-row">
            <span class="recent-session-rank tabular">{String(index + 1).padStart(2, '0')}</span>
            <span class="recent-session-main">
              <strong title={activity.display_name ?? '未命名会话'}>{activity.display_name || '未命名会话'}</strong>
              <small>{[formatToolLabel(activity.tool_id), activity.model_id, relativeTime(activity.observed_at_ms, $liveMinute)].filter(Boolean).join(' · ')}</small>
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
          {@const actState = resolveSessionActivityState(session, sessionActivity, $liveMinute)}
          {@const rate = sessionRateLabel(sessionRates, session, $liveMinute)}
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
                  relativeTime(session.last_event_at_ms ?? session.activity_observed_at_ms, $liveMinute),
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

{:else if cardId === 'sources'}
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>采集源状态</h2>
      </div>
      <button type="button" class="panel-tag" onclick={onNavigateTools}>运行明细 →</button>
    </div>

    {#if snapshot.source_health.length > 0}
      <div class="source-health-list">
        {#each snapshot.source_health.slice(0, cardRows) as source}
          <div class="source-health-row">
            <span
              class="source-status-dot"
              class:complete={source.status === 'complete'}
              class:partial={source.status === 'partial'}
              class:degraded={source.status === 'degraded'}
            ></span>
            <span class="source-health-name">{formatToolLabel(source.tool_id)}</span>
            <span class="source-health-meta">
              {relativeTime(source.last_scan_ms, $liveMinute)} · {source.source_count} 个来源
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

{:else if cardId === 'projects'}
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>项目用量分布</h2>
      </div>
      <button type="button" class="panel-tag" onclick={onNavigateProjects}>查看明细 →</button>
    </div>

    {#if visibleProjects.length > 0}
      <div class="rank-list">
        {#each visibleProjects.slice(0, cardRows) as project, idx}
          <div class="rank-item">
            <div class="rank-row">
              <span class="rank-title">
                <span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span>
                <strong title={project.label}>{project.label}</strong>
              </span>
              <span class="rank-meta">
                <span class="rank-value tabular">{formatNumber(project.total_tokens)}</span>
                <span class="rank-share tabular">{percentOf(project.total_tokens, allProjectTokens).toFixed(1)}%</span>
              </span>
            </div>
            <div class="rank-track">
              <div class="rank-fill project-fill" style={`width: ${percentOf(project.total_tokens, topProjectTotal)}%`}></div>
            </div>
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-box">暂无工作区项目记录</div>
    {/if}
  </section>

{:else if cardId === 'quotas'}
  <QuotaMonitorCard
    {cardRows}
    {activeCodexQuotas}
    {activeAntigravityQuotas}
    {codexQuotaRefreshing}
    {onRefreshCodexQuota}
    {onNavigateQuota}
  />

{:else if cardId === 'cost'}
  <CostBreakdownCard {cardRows} {local} {sortedModels} {onNavigatePricing} />

{:else if cardId === 'efficiency'}
  <EfficiencyCard
    {local}
    {cacheHitRate}
    {inputShareRate}
    {outputShareRate}
    {cacheShareRate}
    {reasoningShareRate}
    {cacheTokens}
  />

{:else if cardId === 'devices'}
  <DevicesSyncCard {cardRows} {snapshot} {remoteDevices} {onNavigateDevices} />

{:else if cardId === 'activity'}
  <ActivitySummaryCard {snapshot} {heatmapCalendar} totalTokens={local?.total_tokens ?? 0} />

{:else if cardId === 'resets'}
  <QuotaResetsCard {activeCodexQuotas} {activeAntigravityQuotas} {onNavigateQuota} />
{/if}
