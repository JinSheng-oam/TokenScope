<script lang="ts">
  import { formatCountdown } from '../dashboardView';
  import { formatPercent } from '../format';
  import { liveMinute, liveNow } from '../liveClock';
  import { buildUnifiedQuotaItems } from '../overviewCards';
  import { groupUpcomingResets } from '../quotaPageView';
  import type { CodexQuota, DashboardSnapshot } from '../types';

  let {
    activeCodexQuotas = [],
    activeAntigravityQuotas = [],
    onNavigateQuota = () => {},
  }: {
    activeCodexQuotas?: CodexQuota[];
    activeAntigravityQuotas?: DashboardSnapshot['antigravity_quota'];
    onNavigateQuota?: () => void;
  } = $props();

  let unifiedQuotas = $derived(
    buildUnifiedQuotaItems(activeCodexQuotas, activeAntigravityQuotas, $liveMinute),
  );
  let upcomingResetGroups = $derived(
    groupUpcomingResets(
      unifiedQuotas.map((q) => ({ ...q, reset_at_ms: q.resetAtMs, observed_at_ms: q.observedAtMs })),
      $liveMinute,
    ),
  );
</script>

<section class="panel-card resets-card">
  <div class="panel-head">
    <div>
      <h2>额度重置倒计时</h2>
    </div>
    <button type="button" class="panel-tag" onclick={onNavigateQuota}>查看明细 →</button>
  </div>

  {#if upcomingResetGroups.length > 0}
    <div class="reset-widget-groups">
      {#each upcomingResetGroups as group (group.label)}
        <div class="reset-widget-group">
          <div class="reset-widget-group-label">{group.label}</div>
          <div class="reset-widget-list">
            {#each group.rows as row (row.key)}
              <div class="reset-widget-row">
                <div class="reset-widget-identity">
                  <span class={`quota-provider-tag provider-${row.provider}`}>{row.providerLabel}</span>
                  <strong class="reset-widget-name">{row.name}</strong>
                  <span class="quota-period-tag">{row.period}</span>
                </div>
                <div class="reset-widget-right tabular">
                  <span class={`reset-widget-pct status-${row.status}`}>{formatPercent(row.remainingBasisPoints)}</span>
                  <span class="reset-widget-time">{formatCountdown(row.reset_at_ms, $liveNow)}</span>
                </div>
              </div>
            {/each}
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty-box">暂无待重置的活跃额度池</div>
  {/if}
</section>

<style>
  .reset-widget-groups {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .reset-widget-group {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .reset-widget-group-label {
    font-size: 10.5px;
    font-weight: 650;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }

  .reset-widget-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .reset-widget-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    min-width: 0;
  }

  .reset-widget-identity {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    flex: 1;
  }

  .reset-widget-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .reset-widget-right {
    display: flex;
    align-items: center;
    gap: 8px;
    white-space: nowrap;
    flex-shrink: 0;
    font-size: 11.5px;
  }

  .reset-widget-pct {
    font-weight: 650;
  }

  .reset-widget-pct.status-healthy {
    color: var(--color-quota);
  }

  .reset-widget-pct.status-warning {
    color: var(--color-estimated);
  }

  .reset-widget-pct.status-exhausted {
    color: var(--color-danger);
  }

  .reset-widget-pct.status-unknown {
    color: var(--text-muted);
  }

  .reset-widget-time {
    color: var(--text-secondary);
  }
</style>
