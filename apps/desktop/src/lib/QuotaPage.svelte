<script lang="ts">
  import {
    antigravityWindowLabel,
    formatCountdown,
    formatQuotaPoolName,
    quotaIsStale,
    quotaWindowLabel,
  } from '../lib/dashboardView';
  import { formatNumber, formatPercent, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import { liveNow } from '../lib/liveClock';
  import type { DashboardSnapshot, UsageBreakdown } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let antigravityUsage: UsageBreakdown | undefined;

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
  <p>Codex 通过本机 Codex app-server 读取；Antigravity 通过仅限回环地址的 RPC 读取。额度、响应次数与 Token 用量分别统计。</p>
</div>

<section class="panel-card quota-section">
  <div class="panel-head">
    <div>
      <h2>Codex 可用额度</h2>
      <p>由 Codex 本地 app-server 返回的计划限额窗口</p>
    </div>
    <span class="precision-chip quota">CODEX APP-SERVER</span>
  </div>
  {#if snapshot.codex_quota.length > 0}
    <div class="quota-grid">
      {#each snapshot.codex_quota as q}
        <article class="quota-item-card" class:exhausted={q.remaining_basis_points === 0}>
          <div class="quota-item-head">
            <span class="quota-pool-name">Codex</span>
            <div class="quota-head-badges">
              {#if q.remaining_basis_points === 0}
                <span class="quota-status-pill exhausted">已耗尽</span>
              {:else if q.remaining_basis_points !== null && q.remaining_basis_points < 2000}
                <span class="quota-status-pill warning">即将用尽</span>
              {/if}
              <span class="panel-tag">{quotaWindowLabel(q.window_duration_minutes)}</span>
            </div>
          </div>
          <div
            class="quota-gauge-val"
            class:exhausted={q.remaining_basis_points === 0}
            class:warning={q.remaining_basis_points !== null && q.remaining_basis_points > 0 && q.remaining_basis_points < 2000}
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
                class:exhausted={q.remaining_basis_points === 0}
                class:warning={q.remaining_basis_points > 0 && q.remaining_basis_points < 2000}
                style={`width: ${q.remaining_basis_points / 100}%;`}
              ></div>
            {/if}
          </div>
          <div class="quota-reset-countdown">
            {@html icons.quota}
            <span>{formatCountdown(q.reset_at_ms, $liveNow)}</span>
          </div>
          <div class="quota-observed" class:stale={quotaIsStale(q.observed_at_ms, $liveNow)}>
            {#if quotaIsStale(q.observed_at_ms, $liveNow)}
              <span class="stale-dot"></span>可能已过期 · {relativeTime(q.observed_at_ms, $liveNow)}
            {:else}
              最近观测 · {relativeTime(q.observed_at_ms, $liveNow)}
            {/if}
          </div>
        </article>
      {/each}
    </div>
  {:else}
    <div class="empty-box">尚无 Codex 额度快照。请确认本机 Codex CLI 已安装并登录，然后刷新。</div>
  {/if}
</section>

<section class="panel-card quota-section">
  <div class="panel-head">
    <div>
      <h2>Antigravity 可用额度</h2>
      <p>来自本机 127.0.0.1 RPC；未知值保留为“未知”，不按 0 处理</p>
    </div>
    <span class="precision-chip quota">LOOPBACK RPC</span>
  </div>
  {#if snapshot.antigravity_quota.length > 0}
    <div class="quota-grid">
      {#each snapshot.antigravity_quota as q}
        <article class="quota-item-card" class:exhausted={q.remaining_basis_points === 0}>
          <div class="quota-item-head">
            <span class="quota-pool-name">{formatQuotaPoolName(q.pool_id)}</span>
            <div class="quota-head-badges">
              {#if q.remaining_basis_points === 0}
                <span class="quota-status-pill exhausted">已耗尽</span>
              {:else if q.remaining_basis_points !== null && q.remaining_basis_points < 2000}
                <span class="quota-status-pill warning">即将用尽</span>
              {/if}
              <span class="panel-tag">{antigravityWindowLabel(q.window_id)}</span>
            </div>
          </div>
          <div
            class="quota-gauge-val"
            class:exhausted={q.remaining_basis_points === 0}
            class:warning={q.remaining_basis_points !== null && q.remaining_basis_points > 0 && q.remaining_basis_points < 2000}
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
                class:exhausted={q.remaining_basis_points === 0}
                class:warning={q.remaining_basis_points > 0 && q.remaining_basis_points < 2000}
                style={`width: ${q.remaining_basis_points / 100}%;`}
              ></div>
            {/if}
          </div>
          <div class="quota-reset-countdown">
            {@html icons.quota}
            <span>{formatCountdown(q.reset_at_ms, $liveNow)}</span>
          </div>
          <div class="quota-observed" class:stale={quotaIsStale(q.observed_at_ms, $liveNow)}>
            {#if quotaIsStale(q.observed_at_ms, $liveNow)}
              <span class="stale-dot"></span>可能已过期 · {relativeTime(q.observed_at_ms, $liveNow)}
            {:else}
              最近观测 · {relativeTime(q.observed_at_ms, $liveNow)}
            {/if}
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
      <p>仅显示来源提供的生成 Token 计数；系统提示计数若存在则列为聚合，额度不会换算成 Token</p>
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
    <div class="empty-box">当前范围没有可计量的 Antigravity Token 记录。不会根据响应次数或额度推算用量。</div>
  {/if}
</section>
