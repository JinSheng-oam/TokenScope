<script lang="ts">
  import {
    antigravityWindowLabel,
    filterActiveAntigravityQuotas,
    formatCountdown,
    formatQuotaPoolName,
    quotaIsStale,
    quotaWindowLabel,
  } from '../lib/dashboardView';
  import { formatNumber, formatPercent, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import { groupUpcomingResets } from './quotaPageView';
  import { liveNow } from '../lib/liveClock';
  import type { CodexQuota, DashboardSnapshot, UsageBreakdown } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let antigravityUsage: UsageBreakdown | undefined;
  export let codexQuotaOverride: CodexQuota[] | null = null;
  export let codexQuotaRefreshing = false;
  export let codexQuotaError = '';
  export let onRefreshCodexQuota: () => void = () => {};

  let showStaleAntigravity = false;

  $: codexQuotas = [...(codexQuotaOverride ?? snapshot.codex_quota)].sort(
    (a, b) => a.window_duration_minutes - b.window_duration_minutes
  );
  $: activeAntigravityQuotas = filterActiveAntigravityQuotas(snapshot.antigravity_quota, false);
  $: allDedupedAntigravityQuotas = filterActiveAntigravityQuotas(snapshot.antigravity_quota, true);
  $: hiddenStaleCount = Math.max(0, allDedupedAntigravityQuotas.length - activeAntigravityQuotas.length);
  $: displayedAntigravityQuotas = showStaleAntigravity
    ? allDedupedAntigravityQuotas
    : activeAntigravityQuotas;

  type QuotaSnapshot = Pick<CodexQuota, 'remaining_basis_points' | 'reset_at_ms' | 'observed_at_ms'>;
  type QuotaRow = QuotaSnapshot & { key: string; name: string; period: string };

  $: codexRows = codexQuotas.map((q): QuotaRow => ({
    ...q,
    key: JSON.stringify(['codex', q.source_id, q.pool_id, q.window_duration_minutes]),
    name: 'Codex',
    period: quotaWindowLabel(q.window_duration_minutes).replace(/周期$/, ''),
  }));
  function antigravityRow(q: DashboardSnapshot['antigravity_quota'][number]): QuotaRow {
    return {
      ...q,
      key: JSON.stringify(['antigravity', q.pool_id, q.window_id]),
      name: formatQuotaPoolName(q.pool_id),
      period: antigravityWindowLabel(q.window_id).replace(/周期$/, ''),
    };
  }
  $: antigravityRows = displayedAntigravityQuotas.map(antigravityRow);
  $: resetCandidates = [...codexRows, ...activeAntigravityQuotas.map(antigravityRow)];
  $: resetGroups = groupUpcomingResets(resetCandidates, $liveNow);

  $: quotaPrecision = !antigravityUsage
    ? null
    : antigravityUsage.exact_records === antigravityUsage.record_count
      ? { className: 'exact', label: '精确事实' }
      : antigravityUsage.estimated_records === antigravityUsage.record_count
        ? { className: 'estimated', label: '显式估算' }
        : antigravityUsage.unknown_precision_records === antigravityUsage.record_count
          ? { className: 'muted', label: '精度未知' }
          : { className: 'aggregate', label: '混合精度' };
</script>

{#snippet quotaRows(rows: QuotaRow[])}
  <ul class="quota-rows">
    {#each rows as row (row.key)}
      {@const remaining = row.remaining_basis_points}
      {@const exhausted = remaining === 0}
      {@const warning = remaining !== null && remaining > 0 && remaining < 2000}
      {@const stale = quotaIsStale(row.observed_at_ms, $liveNow)}
      <li class="quota-row" class:exhausted class:warning>
        <div class="quota-row-identity" title={`观测于 ${relativeTime(row.observed_at_ms, $liveNow)}`}>
          <strong>{row.name}</strong>
          <span class="panel-tag">{row.period}</span>
          {#if exhausted || warning || stale}
            <span class="quota-row-status">
              {[exhausted ? '已耗尽' : warning ? '即将用尽' : '', stale ? '快照可能过期' : ''].filter(Boolean).join(' · ')}
            </span>
          {/if}
        </div>
        <div class="quota-row-gauge">
          <div class="rank-track quota-track" class:unknown={remaining === null} aria-hidden="true">
            {#if remaining !== null}
              <div class="rank-fill" style:width={`${remaining / 100}%`}></div>
            {/if}
          </div>
          <strong class="quota-gauge-val">{formatPercent(remaining)}</strong>
        </div>
        <span class="quota-reset-countdown">
          {@html icons.quota}
          <span>{formatCountdown(row.reset_at_ms, $liveNow)}</span>
        </span>
      </li>
    {/each}
  </ul>
{/snippet}

<div class="page-header quota-page-header">
  <h1>额度与活动</h1>
  <button
    type="button"
    class="btn-secondary"
    disabled={codexQuotaRefreshing}
    onclick={onRefreshCodexQuota}
    title="刷新本机 Codex 额度"
  >
    <span class:spin={codexQuotaRefreshing}>{@html icons.refresh}</span>
    {codexQuotaRefreshing ? '正在刷新' : '刷新额度'}
  </button>
</div>

<div class="quota-layout">
  <div class="quota-layout-grid">
    <div class="quota-provider-stack">
      <section class="panel-card quota-panel" aria-label="Codex 可用额度" aria-busy={codexQuotaRefreshing}>
        <div class="panel-head quota-panel-head">
          <h2>Codex 可用额度</h2>
          <span class="precision-chip quota">CODEX APP-SERVER</span>
        </div>
        {#if codexQuotaError}
          <div class="notice-box quota-refresh-error" role="alert"><p>{codexQuotaError}</p></div>
        {/if}
        {#if codexRows.length}
          {@render quotaRows(codexRows)}
        {:else}
          <div class="empty-box">尚无 Codex 额度快照。请确认本机 Codex CLI 已安装并登录，然后点击“刷新额度”。</div>
        {/if}
      </section>

      <section class="panel-card quota-panel" aria-label="Antigravity 可用额度">
        <div class="panel-head quota-panel-head">
          <h2>Antigravity 可用额度</h2>
          <div class="quota-header-actions">
            {#if hiddenStaleCount > 0}
              <button
                type="button"
                class="panel-tag quota-stale-toggle"
                aria-pressed={showStaleAntigravity}
                onclick={() => (showStaleAntigravity = !showStaleAntigravity)}
              >{showStaleAntigravity ? `隐藏历史快照 (${hiddenStaleCount})` : `查看历史快照 (${hiddenStaleCount})`}</button>
            {/if}
            <span class="precision-chip quota">LOOPBACK RPC</span>
          </div>
        </div>
        {#if antigravityRows.length}
          {@render quotaRows(antigravityRows)}
        {:else}
          <div class="empty-box">尚无 Antigravity 额度快照。请确认 Antigravity IDE 或 CLI 正在运行，然后刷新。</div>
        {/if}
      </section>
    </div>

    <section class="panel-card quota-timeline" aria-label="重置顺序">
      <div class="panel-head"><h2>重置顺序</h2></div>
      {#each resetGroups as group (group.label)}
        <section class="quota-timeline-group">
          <h3>{group.label}</h3>
          <ol>
            {#each group.rows as row (row.key)}
              <li>
                <div class="quota-timeline-identity">
                  <strong>{row.name}</strong>
                  <span class="panel-tag">{row.period}</span>
                </div>
                <span class="quota-timeline-countdown tabular">{formatCountdown(row.reset_at_ms, $liveNow)}</span>
              </li>
            {/each}
          </ol>
        </section>
      {:else}
        <div class="empty-box">暂无可确认的重置时间。</div>
      {/each}
    </section>
  </div>
</div>

<section class="panel-card quota-usage-summary">
  <div class="panel-head">
    <div>
      <h2>Antigravity Token 用量</h2>
    </div>
    {#if quotaPrecision}
      <span class={`precision-chip ${quotaPrecision.className}`}>{quotaPrecision.label}</span>
    {:else}
      <span class="precision-chip muted">无 Token 记录</span>
    {/if}
  </div>
  {#if antigravityUsage}
    <div class="device-summary-strip quota-summary-strip">
      <div class="summary-pill-card"><span>Token 总量</span><strong class="tabular">{formatNumber(antigravityUsage.total_tokens)}</strong></div>
      <div class="summary-pill-card"><span>来源记录</span><strong class="tabular">{formatNumber(antigravityUsage.record_count)}</strong></div>
      <div class="summary-pill-card"><span>精确 / 聚合</span><strong class="tabular">{formatNumber(antigravityUsage.exact_records)} / {formatNumber(antigravityUsage.aggregate_records)}</strong></div>
      <div class="summary-pill-card"><span>显式估算</span><strong class="tabular">{formatNumber(antigravityUsage.estimated_records)}</strong></div>
    </div>
    {#if antigravityUsage.untimed_records > 0}
      <p class="usage-note">{formatNumber(antigravityUsage.untimed_records)} 条记录没有可靠事件时间，因此不会进入所选日期范围和日趋势。</p>
    {/if}
  {:else}
    <div class="empty-box">当前范围无 Antigravity Token 记录。</div>
  {/if}
</section>
