<script lang="ts">
  import { displayCurrency } from './displayCurrency';
  import { formatCompactTokens, resolveSessionActivityState } from './dashboardView';
  import { formatCost, formatNumber, formatToolLabel, percentOf, relativeTime } from './format';
  import { liveNow } from './liveClock';
  import { sessionRateLabel, sessionRateTitle, type SessionRates } from './sessionRates';
  import type { CardId } from './dashboardLayout';
  import type { DashboardSnapshot, SessionActivity, SessionUsage, UsageBreakdown, UsageTotals } from './types';

  export let cardId: CardId;
  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
  export let activeTools: UsageBreakdown[] = [];
  export let topToolTotal: number = 1;
  export let sortedModels: UsageBreakdown[] = [];
  export let allModelTokens: number = 0;
  export let topModelTotal: number = 1;
  export let pendingActivity: SessionActivity[] = [];
  export let visibleRecentSessions: SessionUsage[] = [];
  export let sessionActivity: SessionActivity[] = [];
  export let sessionRates: SessionRates | null = null;
  export let visibleProjects: UsageBreakdown[] = [];
  export let allProjectTokens: number = 0;
  export let topProjectTotal: number = 1;
  export let activeAntigravityQuotas: DashboardSnapshot['antigravity_quota'] = [];
  export let onNavigateTools: () => void = () => {};
  export let onNavigateModels: () => void = () => {};
  export let onNavigateSessions: () => void = () => {};
  export let onNavigateProjects: () => void = () => {};
  export let onNavigateQuota: () => void = () => {};

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
  <!-- Tool Rankings -->
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>工具用量分布</h2>
      </div>
      <button class="panel-tag" onclick={onNavigateTools}>查看明细 →</button>
    </div>

    {#if activeTools.length > 0}
      <div class="rank-list">
        {#each activeTools.slice(0, 5) as tool, idx}
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
  <!-- Model Usage Distribution -->
  <section class="panel-card model-usage-card">
    <div class="panel-head">
      <div>
        <h2>模型用量分布</h2>
      </div>
      <button class="panel-tag" onclick={onNavigateModels}>查看明细 →</button>
    </div>

    {#if sortedModels.length > 0}
      <div class="rank-list">
        {#each sortedModels.slice(0, 5) as model, idx}
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
  <!-- Recent Sessions -->
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

{:else if cardId === 'sources'}
  <!-- Source Health -->
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

{:else if cardId === 'projects'}
  <!-- Project Usage Distribution -->
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>项目用量分布</h2>
      </div>
      <button class="panel-tag" onclick={onNavigateProjects}>查看明细 →</button>
    </div>

    {#if visibleProjects.length > 0}
      <div class="rank-list">
        {#each visibleProjects.slice(0, 5) as project, idx}
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
  <!-- Quotas Monitor -->
  <section class="panel-card">
    <div class="panel-head">
      <div>
        <h2>额度与限额监控</h2>
      </div>
      <button class="panel-tag" onclick={onNavigateQuota}>查看明细 →</button>
    </div>

    {#if activeAntigravityQuotas.length > 0}
      <div class="quota-widget-list">
        {#each activeAntigravityQuotas as quota}
          {@const pct = quota.remaining_basis_points != null ? (quota.remaining_basis_points / 100).toFixed(0) : '0'}
          {@const label = quota.window_id === 'five_hour' ? '5 小时限额' : quota.window_id === 'weekly' ? '每周限额' : '额度池'}
          <div class="quota-widget-item">
            <div class="quota-widget-head">
              <span class="quota-widget-name">{label}</span>
              <span class="quota-widget-val tabular">{pct}% 剩余</span>
            </div>
            <div class="quota-track">
              <div class="quota-fill" style={`width: ${pct}%`}></div>
            </div>
            {#if quota.reset_at_ms != null && quota.reset_at_ms > $liveNow}
              <div class="quota-widget-foot">
                <small>{relativeTime(quota.reset_at_ms)} 重置</small>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <div class="empty-box">暂无在线额度观察数据</div>
    {/if}
  </section>
{/if}
