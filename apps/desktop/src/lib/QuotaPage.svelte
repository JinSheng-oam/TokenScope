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

<div class="page-header">
  <h1>额度与活动</h1>
</div>

<section class="panel-card quota-section">
  <div class="panel-head">
    <div>
      <h2>Codex 可用额度</h2>
    </div>
    <div class="quota-header-actions">
      <span class="precision-chip quota">CODEX APP-SERVER</span>
      <button
        type="button"
        class="btn-secondary"
        disabled={codexQuotaRefreshing}
        onclick={onRefreshCodexQuota}
      >
        <span class:spin={codexQuotaRefreshing}>{@html icons.refresh}</span>
        {codexQuotaRefreshing ? '正在刷新' : '刷新额度'}
      </button>
    </div>
  </div>
  {#if codexQuotaError}
    <div class="notice-box quota-refresh-error" role="alert"><p>{codexQuotaError}</p></div>
  {/if}
  {#if codexQuotas.length > 0}
    <div class="quota-grid">
      {#each codexQuotas as q}
        {@const isExhausted = q.remaining_basis_points === 0}
        {@const isWarning = q.remaining_basis_points !== null && q.remaining_basis_points > 0 && q.remaining_basis_points < 2000}
        {@const isStale = quotaIsStale(q.observed_at_ms, $liveNow)}
        <article class="quota-item-card" class:exhausted={isExhausted} class:warning={isWarning}>
          <div class="quota-item-head">
            <span class="quota-pool-name">Codex</span>
            <div class="quota-head-badges">
              {#if isExhausted}
                <span class="quota-status-pill exhausted">已耗尽</span>
              {:else if isWarning}
                <span class="quota-status-pill warning">即将用尽</span>
              {/if}
              <span class="panel-tag">{quotaWindowLabel(q.window_duration_minutes)}</span>
            </div>
          </div>
          <div
            class="quota-gauge-val"
            class:exhausted={isExhausted}
            class:warning={isWarning}
          >
            {formatPercent(q.remaining_basis_points)}
          </div>
          <div
            class="rank-track quota-track"
            class:unknown={q.remaining_basis_points === null}
            role="meter"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={q.remaining_basis_points === null ? undefined : q.remaining_basis_points / 100}
            aria-valuetext={formatPercent(q.remaining_basis_points)}
          >
            {#if q.remaining_basis_points !== null}
              <div
                class="rank-fill"
                class:exhausted={isExhausted}
                class:warning={isWarning}
                style={`width: ${q.remaining_basis_points / 100}%;`}
              ></div>
            {/if}
          </div>
          <div class="quota-card-foot">
            <div class="quota-reset-countdown">
              {@html icons.quota}
              <span>{formatCountdown(q.reset_at_ms, $liveNow)}</span>
            </div>
            <div class="quota-observed" class:stale={isStale}>
              <span class="obs-dot" class:stale={isStale}></span>
              {#if isStale}
                可能已过期 · {relativeTime(q.observed_at_ms, $liveNow)}
              {:else}
                观测于 {relativeTime(q.observed_at_ms, $liveNow)}
              {/if}
            </div>
          </div>
        </article>
      {/each}
    </div>
  {:else}
    <div class="empty-box">尚无 Codex 额度快照。请确认本机 Codex CLI 已安装并登录，然后点击“刷新额度”。</div>
  {/if}
</section>

<section class="panel-card quota-section">
  <div class="panel-head">
    <div>
      <h2>Antigravity 可用额度</h2>
    </div>
    <div class="quota-header-actions">
      {#if hiddenStaleCount > 0}
        <button
          type="button"
          class="panel-tag quota-stale-toggle"
          class:active={showStaleAntigravity}
          onclick={() => (showStaleAntigravity = !showStaleAntigravity)}
          title="切换是否显示未在最近一次 RPC 快照中出现的历史模型池"
        >
          {showStaleAntigravity ? `隐藏历史快照 (${hiddenStaleCount})` : `已过滤 ${hiddenStaleCount} 项过期快照`}
        </button>
      {/if}
      <span class="precision-chip quota">LOOPBACK RPC</span>
    </div>
  </div>
  {#if displayedAntigravityQuotas.length > 0}
    <div class="quota-grid">
      {#each displayedAntigravityQuotas as q}
        {@const isExhausted = q.remaining_basis_points === 0}
        {@const isWarning = q.remaining_basis_points !== null && q.remaining_basis_points > 0 && q.remaining_basis_points < 2000}
        {@const isStale = quotaIsStale(q.observed_at_ms, $liveNow)}
        <article class="quota-item-card" class:exhausted={isExhausted} class:warning={isWarning}>
          <div class="quota-item-head">
            <span class="quota-pool-name">{formatQuotaPoolName(q.pool_id)}</span>
            <div class="quota-head-badges">
              {#if isExhausted}
                <span class="quota-status-pill exhausted">已耗尽</span>
              {:else if isWarning}
                <span class="quota-status-pill warning">即将用尽</span>
              {/if}
              <span class="panel-tag">{antigravityWindowLabel(q.window_id)}</span>
            </div>
          </div>
          <div
            class="quota-gauge-val"
            class:exhausted={isExhausted}
            class:warning={isWarning}
          >
            {formatPercent(q.remaining_basis_points)}
          </div>
          <div
            class="rank-track quota-track"
            class:unknown={q.remaining_basis_points === null}
            role="meter"
            aria-valuemin="0"
            aria-valuemax="100"
            aria-valuenow={q.remaining_basis_points === null ? undefined : q.remaining_basis_points / 100}
            aria-valuetext={formatPercent(q.remaining_basis_points)}
          >
            {#if q.remaining_basis_points !== null}
              <div
                class="rank-fill"
                class:exhausted={isExhausted}
                class:warning={isWarning}
                style={`width: ${q.remaining_basis_points / 100}%;`}
              ></div>
            {/if}
          </div>
          <div class="quota-card-foot">
            <div class="quota-reset-countdown">
              {@html icons.quota}
              <span>{formatCountdown(q.reset_at_ms, $liveNow)}</span>
            </div>
            <div class="quota-observed" class:stale={isStale}>
              <span class="obs-dot" class:stale={isStale}></span>
              {#if isStale}
                可能已过期 · {relativeTime(q.observed_at_ms, $liveNow)}
              {:else}
                观测于 {relativeTime(q.observed_at_ms, $liveNow)}
              {/if}
            </div>
          </div>
        </article>
      {/each}
    </div>
  {:else}
    <div class="empty-box">尚无 Antigravity 额度快照。请确认 Antigravity IDE 或 CLI 正在运行，然后刷新。</div>
  {/if}
</section>

<section class="panel-card quota-section">
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
