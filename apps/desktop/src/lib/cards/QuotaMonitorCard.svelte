<script lang="ts">
  import { formatCountdown } from '../dashboardView';
  import { formatPercent, relativeTime } from '../format';
  import { icons } from '../icons';
  import { liveMinute, liveNow } from '../liveClock';
  import { buildUnifiedQuotaItems } from '../overviewCards';
  import type { CodexQuota, DashboardSnapshot } from '../types';

  let {
    activeCodexQuotas = [],
    activeAntigravityQuotas = [],
    codexQuotaRefreshing = false,
    onRefreshCodexQuota = null,
    onNavigateQuota = () => {},
  }: {
    activeCodexQuotas?: CodexQuota[];
    activeAntigravityQuotas?: DashboardSnapshot['antigravity_quota'];
    codexQuotaRefreshing?: boolean;
    onRefreshCodexQuota?: (() => void) | null;
    onNavigateQuota?: () => void;
  } = $props();

  let unifiedQuotas = $derived(
    buildUnifiedQuotaItems(activeCodexQuotas, activeAntigravityQuotas, $liveMinute),
  );
  let exhaustedQuotaCount = $derived(unifiedQuotas.filter((q) => q.status === 'exhausted').length);
  let warningQuotaCount = $derived(unifiedQuotas.filter((q) => q.status === 'warning').length);
</script>

<section class="panel-card quota-monitor-card">
  <div class="panel-head">
    <div class="quota-monitor-title-row">
      <h2>额度与限额监控</h2>
      {#if exhaustedQuotaCount > 0}
        <span class="quota-summary-pill exhausted">{exhaustedQuotaCount} 已耗尽</span>
      {:else if warningQuotaCount > 0}
        <span class="quota-summary-pill warning">{warningQuotaCount} 即将用尽</span>
      {:else if unifiedQuotas.length > 0}
        <span class="quota-summary-pill healthy">{unifiedQuotas.length} 项正常</span>
      {/if}
    </div>
    <div class="panel-head-actions">
      {#if onRefreshCodexQuota}
        <button
          type="button"
          class="quota-refresh-icon-btn"
          disabled={codexQuotaRefreshing}
          onclick={onRefreshCodexQuota}
          title="刷新本机 Codex 额度"
          aria-label="刷新本机 Codex 额度"
        >
          <span class:spin={codexQuotaRefreshing}>{@html icons.refresh}</span>
        </button>
      {/if}
      <button type="button" class="panel-tag" onclick={onNavigateQuota}>查看明细 →</button>
    </div>
  </div>

  {#if unifiedQuotas.length > 0}
    <div class="quota-widget-grid">
      {#each unifiedQuotas as item (item.key)}
        {@const fillWidth = item.remainingBasisPoints !== null ? Math.max(0, Math.min(100, item.remainingBasisPoints / 100)) : 0}
        <div
          class={`quota-widget-card status-${item.status}`}
          class:is-stale={item.stale}
          title={`来源：${item.providerLabel} · 观测于 ${relativeTime(item.observedAtMs, $liveMinute)}`}
        >
          <div class="quota-widget-head">
            <div class="quota-widget-identity">
              <span class={`quota-provider-tag provider-${item.provider}`}>{item.providerLabel}</span>
              <span class="quota-widget-name" title={item.name}>{item.name}</span>
              <span class="quota-period-tag">{item.period}</span>
            </div>
            <span class={`quota-widget-val tabular status-${item.status}`}>
              {formatPercent(item.remainingBasisPoints)}
            </span>
          </div>

          <div class="quota-track" class:unknown={item.remainingBasisPoints === null} aria-hidden="true">
            {#if item.remainingBasisPoints !== null}
              <div class={`quota-fill status-${item.status}`} style={`width: ${fillWidth}%`}></div>
            {/if}
          </div>

          <div class="quota-widget-foot">
            <span class="quota-widget-status">
              {#if item.status === 'exhausted'}
                <span class="status-text exhausted">已耗尽</span>
              {:else if item.status === 'warning'}
                <span class="status-text warning">即将用尽</span>
              {:else if item.stale}
                <span class="status-text warning">快照可能过期</span>
              {:else}
                <span>观测于 {relativeTime(item.observedAtMs, $liveMinute)}</span>
              {/if}
            </span>
            <span class="quota-widget-countdown tabular">
              {@html icons.quota}
              <span>{formatCountdown(item.resetAtMs, $liveNow)}</span>
            </span>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty-box">暂无 Codex 或 Antigravity 在线额度数据</div>
  {/if}
</section>

<style>
  .quota-monitor-card {
    container-type: inline-size;
  }

  .quota-monitor-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    flex-wrap: wrap;
  }

  .quota-summary-pill {
    display: inline-flex;
    align-items: center;
    padding: 1px 7px;
    border-radius: var(--radius-pill);
    font-size: 10.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  .quota-summary-pill.healthy {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border: 1px solid var(--color-exact-border);
  }

  .quota-summary-pill.warning {
    background: var(--color-estimated-bg);
    color: var(--color-estimated);
    border: 1px solid var(--color-estimated-border);
  }

  .quota-summary-pill.exhausted {
    background: var(--color-danger-bg);
    color: var(--color-danger);
    border: 1px solid var(--color-danger-border);
  }

  .quota-refresh-icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .quota-refresh-icon-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-strong);
    background: var(--bg-card-hover);
  }

  .quota-refresh-icon-btn:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .quota-refresh-icon-btn :global(svg) {
    width: 13px;
    height: 13px;
  }

  .quota-widget-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
  }

  @container (min-width: 520px) {
    .quota-widget-grid {
      grid-template-columns: repeat(2, minmax(0, 1fr));
      gap: 10px;
    }
  }

  @container (min-width: 860px) {
    .quota-widget-grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }

  .quota-widget-card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 9px 11px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
    min-width: 0;
  }

  .quota-widget-card.status-warning {
    border-color: var(--color-estimated-border);
  }

  .quota-widget-card.status-exhausted {
    border-color: var(--color-danger-border);
    background: var(--color-danger-bg);
  }

  .quota-widget-card.is-stale {
    opacity: 0.85;
  }

  .quota-widget-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .quota-widget-identity {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    flex: 1;
  }

  .quota-widget-name {
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .quota-widget-val {
    font-size: 13px;
    font-weight: 700;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .quota-widget-val.status-healthy {
    color: var(--color-quota);
  }

  .quota-widget-val.status-warning {
    color: var(--color-estimated);
  }

  .quota-widget-val.status-exhausted {
    color: var(--color-danger);
  }

  .quota-widget-val.status-unknown {
    color: var(--text-muted);
    font-size: 11.5px;
  }

  .quota-track {
    width: 100%;
    height: 6px;
    border-radius: var(--radius-pill);
    background: var(--bg-hover);
    overflow: hidden;
  }

  .quota-track.unknown {
    background: repeating-linear-gradient(
      -45deg,
      var(--border-subtle),
      var(--border-subtle) 4px,
      transparent 4px,
      transparent 8px
    );
    opacity: 0.65;
  }

  .quota-fill {
    height: 100%;
    border-radius: var(--radius-pill);
    transition: width 0.3s ease;
  }

  .quota-fill.status-healthy {
    background: linear-gradient(90deg, var(--color-quota) 0%, var(--accent-project-end) 100%);
  }

  .quota-fill.status-warning {
    background: linear-gradient(90deg, var(--accent-cost-start) 0%, var(--color-estimated) 100%);
  }

  .quota-fill.status-exhausted {
    background: var(--color-danger);
  }

  .quota-widget-foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 8px;
    color: var(--text-muted);
    font-size: 10.5px;
  }

  .quota-widget-status {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .quota-widget-status .status-text.warning {
    color: var(--color-estimated);
    font-weight: 600;
  }

  .quota-widget-status .status-text.exhausted {
    color: var(--color-danger);
    font-weight: 600;
  }

  .quota-widget-countdown {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    color: var(--text-secondary);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .quota-widget-countdown :global(svg) {
    width: 11px;
    height: 11px;
    opacity: 0.8;
  }
</style>
