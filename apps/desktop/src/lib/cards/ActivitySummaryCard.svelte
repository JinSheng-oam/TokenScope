<script lang="ts">
  import { formatCompactTokens } from '../dashboardView';
  import { formatNumber } from '../format';
  import type { HeatmapCalendar } from '../heatmap';
  import { buildActivitySummary } from '../overviewCards';
  import type { DashboardSnapshot } from '../types';

  let {
    snapshot,
    heatmapCalendar = null,
    totalTokens = 0,
  }: {
    snapshot: DashboardSnapshot;
    heatmapCalendar?: HeatmapCalendar | null;
    totalTokens?: number;
  } = $props();

  let activitySummary = $derived(
    buildActivitySummary(snapshot.daily, heatmapCalendar, snapshot.session_usage.total, totalTokens),
  );
</script>

<section class="panel-card activity-card">
  <div class="panel-head">
    <div>
      <h2>日均与峰值统计</h2>
    </div>
    {#if activitySummary.currentStreakDays > 0}
      <span class="precision-chip exact tabular">连续 {activitySummary.currentStreakDays} 天</span>
    {:else}
      <span class="panel-tag tabular">活跃 {activitySummary.activeDays} 天</span>
    {/if}
  </div>

  <div class="mini-kpi-grid">
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">活跃日均</span>
      <strong class="mini-kpi-val tabular">{formatCompactTokens(activitySummary.dailyAverageTokens)}</strong>
    </div>
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">单日峰值{activitySummary.peakDate ? ` (${activitySummary.peakDate.slice(5)})` : ''}</span>
      <strong class="mini-kpi-val tabular">{formatCompactTokens(activitySummary.peakTokens)}</strong>
    </div>
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">会话总数</span>
      <strong class="mini-kpi-val tabular">{formatNumber(activitySummary.sessionCount)}</strong>
    </div>
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">单会话均耗</span>
      <strong class="mini-kpi-val tabular">{formatCompactTokens(activitySummary.avgTokensPerSession)}</strong>
    </div>
  </div>

  {#if activitySummary.recentBars.length > 0}
    <div class="sparkbar-section">
      <div class="sparkbar-head">
        <span>近 {activitySummary.recentBars.length} 日趋势</span>
        <span class="tabular">{activitySummary.recentBars[0].date.slice(5)} – {activitySummary.recentBars[activitySummary.recentBars.length - 1].date.slice(5)}</span>
      </div>
      <div class="sparkbar-grid" role="img" aria-label="近期每日 Token 柱状图">
        {#each activitySummary.recentBars as bar (bar.date)}
          <div
            class="sparkbar-col"
            title={`${bar.date}: ${formatNumber(bar.totalTokens)} Token`}
          >
            <div
              class="sparkbar-fill"
              class:is-peak={bar.date === activitySummary.peakDate && bar.totalTokens > 0}
              class:is-empty={bar.totalTokens === 0}
              style={`height: ${bar.heightPct}%`}
            ></div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</section>

<style>
  .sparkbar-section {
    margin-top: 12px;
    padding-top: 10px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .sparkbar-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    font-size: 10.5px;
    color: var(--text-muted);
  }

  .sparkbar-grid {
    display: flex;
    align-items: flex-end;
    gap: 4px;
    height: 44px;
    padding-top: 4px;
  }

  .sparkbar-col {
    flex: 1;
    height: 100%;
    display: flex;
    align-items: flex-end;
  }

  .sparkbar-fill {
    width: 100%;
    border-radius: 3px 3px 1px 1px;
    background: var(--brand-primary);
    opacity: 0.72;
    transition: opacity var(--transition-fast), height 0.25s ease;
  }

  .sparkbar-col:hover .sparkbar-fill {
    opacity: 1;
  }

  .sparkbar-fill.is-peak {
    background: linear-gradient(180deg, var(--accent-peak-start) 0%, var(--accent-peak-end) 100%);
    opacity: 1;
  }

  .sparkbar-fill.is-empty {
    background: var(--border-subtle);
    opacity: 0.5;
  }
</style>
