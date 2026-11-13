<script lang="ts">
  import CustomSelect from './CustomSelect.svelte';
  import { displayCurrency } from './displayCurrency';
  import { formatCost, formatNumber, formatToolLabel, percentOf } from '../lib/format';
  import type { ModelSortBy } from '../lib/dashboardView';
  import type { DashboardSnapshot, UsageBreakdown } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let deviceScope: string;
  export let sortedModels: UsageBreakdown[];
  export let topModelTotal: number;
  export let modelSortBy: ModelSortBy;

  const RANK_PAGE_SIZES = [4, 8, 15, 30] as const;
  const ACTIVITY_PAGE_SIZES = [4, 8, 15, 30] as const;
  const rankPageSizeOptions = RANK_PAGE_SIZES.map((n) => ({ key: String(n), label: `${n} 条 / 页` }));
  const activityPageSizeOptions = ACTIVITY_PAGE_SIZES.map((n) => ({ key: String(n), label: `${n} 条 / 页` }));

  let rankPageSize = 4;
  let rankPage = 1;
  let activityPageSize = 4;
  let activityPage = 1;
  let prevSortBy = modelSortBy;

  $: if (modelSortBy !== prevSortBy) {
    prevSortBy = modelSortBy;
    rankPage = 1;
  }

  $: allModelTokens = sortedModels.reduce((sum, m) => sum + m.total_tokens, 0);
  $: rankTotalPages = Math.max(1, Math.ceil(sortedModels.length / rankPageSize));
  $: currentRankPage = Math.min(rankPage, rankTotalPages);
  $: rankOffset = (currentRankPage - 1) * rankPageSize;
  $: visibleModels = sortedModels.slice(rankOffset, rankOffset + rankPageSize);

  $: modelActivity = snapshot.model_activity;
  $: activityTotalPages = Math.max(1, Math.ceil(modelActivity.length / activityPageSize));
  $: currentActivityPage = Math.min(activityPage, activityTotalPages);
  $: activityOffset = (currentActivityPage - 1) * activityPageSize;
  $: visibleActivity = modelActivity.slice(activityOffset, activityOffset + activityPageSize);
</script>

<div class="page-header">
  <h1>模型排行与用量分析</h1>
  <div class="range-capsule" role="group" aria-label="模型列表排序模式">
    <button
      class="range-btn"
      class:selected={modelSortBy === 'tokens'}
      aria-pressed={modelSortBy === 'tokens'}
      onclick={() => (modelSortBy = 'tokens')}
    >
      按 Token 排序
    </button>
    <button
      class="range-btn"
      class:selected={modelSortBy === 'cost'}
      aria-pressed={modelSortBy === 'cost'}
      onclick={() => (modelSortBy = 'cost')}
    >
      按 估算成本 排序
    </button>
  </div>
</div>

<div class="models-stack">
  <section class="panel-card models-panel">
    <div class="panel-head">
      <div>
        <h2>模型消耗总榜</h2>
      </div>
      <span class="panel-tag">{sortedModels.length} 个模型分组</span>
    </div>

    {#if sortedModels.length > 0}
      <div class="rank-list models-rank-list">
        {#each visibleModels as item, index (item.key)}
          <div class="rank-item models-rank-item">
            <div class="rank-row">
              <span class="rank-title" style="min-width: 0;">
                <span class="rank-num tabular">{String(rankOffset + index + 1).padStart(2, '0')}</span>
                <strong style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap;" title={item.label}>
                  {item.label}
                </strong>
              </span>
              <span class="rank-meta">
                <span class="rank-value tabular">{formatNumber(item.total_tokens)}</span>
                {#if item.estimated_cost_microusd != null}
                  <span class="panel-tag" style="font-size: 10.5px; font-weight: 500;">
                    {formatCost(item.estimated_cost_microusd, $displayCurrency)}
                  </span>
                {/if}
                <span class="rank-share tabular">{percentOf(item.total_tokens, allModelTokens).toFixed(1)}%</span>
              </span>
            </div>
            <div class="rank-track">
              <div class="rank-fill" style={`width: ${percentOf(item.total_tokens, topModelTotal)}%`}></div>
            </div>
            <div class="metric-foot models-rank-foot">
              <div style="display: flex; align-items: center; gap: 10px; flex-wrap: wrap;">
                <span style="color: var(--text-muted);">记录 {formatNumber(item.record_count)} 笔</span>
                <span class="integrity-item"><span class="integrity-dot exact"></span>精确 {formatNumber(item.exact_records)}</span>
                <span class="integrity-item"><span class="integrity-dot aggregate"></span>聚合 {formatNumber(item.aggregate_records)}</span>
                <span class="integrity-item"><span class="integrity-dot estimated"></span>估算 {formatNumber(item.estimated_records)}</span>
                {#if item.unknown_precision_records > 0}
                  <span class="integrity-item"><span class="integrity-dot muted"></span>未知 {formatNumber(item.unknown_precision_records)}</span>
                {/if}
              </div>
              {#if item.untimed_records > 0}
                <span style="color: var(--text-warning);">{formatNumber(item.untimed_records)} 条无时间</span>
              {/if}
            </div>
          </div>
        {/each}
      </div>

      <div class="data-pagination">
        <span class="data-pagination-info tabular" role="status">
          显示 <strong>{formatNumber(rankOffset + 1)}–{formatNumber(rankOffset + visibleModels.length)}</strong> / 共 <strong>{formatNumber(sortedModels.length)}</strong> 个模型
        </span>
        <div class="data-pagination-controls">
          <div class="data-page-size">
            <span>每页</span>
            <CustomSelect
              value={String(rankPageSize)}
              options={rankPageSizeOptions}
              align="right"
              ariaLabel="设置模型消耗总榜每页数量"
              onChange={(val) => { rankPageSize = Number(val); rankPage = 1; }}
            />
          </div>
          <div class="data-page-nav" role="group" aria-label="模型消耗总榜分页">
            <button type="button" class="btn-secondary btn-page" disabled={currentRankPage <= 1} onclick={() => (rankPage = 1)}>首页</button>
            <button type="button" class="btn-secondary btn-page" disabled={currentRankPage <= 1} onclick={() => (rankPage = currentRankPage - 1)}>上一页</button>
            <span class="data-page-indicator tabular">第 <strong>{currentRankPage}</strong> / {rankTotalPages} 页</span>
            <button type="button" class="btn-secondary btn-page" disabled={currentRankPage >= rankTotalPages} onclick={() => (rankPage = currentRankPage + 1)}>下一页</button>
            <button type="button" class="btn-secondary btn-page" disabled={currentRankPage >= rankTotalPages} onclick={() => (rankPage = rankTotalPages)}>末页</button>
          </div>
        </div>
      </div>
    {:else}
      <div class="empty-box">暂无模型记录</div>
    {/if}
  </section>

  <section class="panel-card models-panel">
    <div class="panel-head">
      <div>
        <h2>模型响应与调用量</h2>
      </div>
      {#if deviceScope === 'all'}
        <span class="precision-chip muted">仅本机</span>
      {/if}
    </div>

    {#if deviceScope !== 'all' && deviceScope !== 'local'}
      <div class="empty-box">暂无该设备的调用数据。</div>
    {:else if modelActivity.length > 0}
      <div class="table-wrap">
        <table class="models-activity-table">
          <thead>
            <tr>
              <th style="width: 140px;">来源工具</th>
              <th>模型名称</th>
              <th class="tabular" style="width: 140px;">调用量</th>
              <th style="width: 130px;">统计范围</th>
            </tr>
          </thead>
          <tbody>
            {#each visibleActivity as activity}
              <tr>
                <td>
                  <span class="session-facet-chip" style="font-weight: 500;">
                    {formatToolLabel(activity.tool_id)}
                  </span>
                </td>
                <td>
                  <strong
                    title={activity.model_id === 'unknown-model'
                      ? '来源会话没有提供模型标识；响应已计入统计'
                      : activity.model_id}
                  >
                    {activity.model_id === 'unknown-model' ? '来源未提供模型' : activity.model_id}
                  </strong>
                </td>
                <td class="tabular"><strong>{formatNumber(activity.response_count)}</strong></td>
                <td>
                  <span class="precision-chip" class:quota={activity.time_scope === 'selected_range'} class:muted={activity.time_scope !== 'selected_range'}>
                    {activity.time_scope === 'selected_range' ? '当前时间范围' : '来源累计'}
                  </span>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      <div class="data-pagination">
        <span class="data-pagination-info tabular" role="status">
          显示 <strong>{formatNumber(activityOffset + 1)}–{formatNumber(activityOffset + visibleActivity.length)}</strong> / 共 <strong>{formatNumber(modelActivity.length)}</strong> 条调用
        </span>
        <div class="data-pagination-controls">
          <div class="data-page-size">
            <span>每页</span>
            <CustomSelect
              value={String(activityPageSize)}
              options={activityPageSizeOptions}
              align="right"
              ariaLabel="设置模型调用量每页数量"
              onChange={(val) => { activityPageSize = Number(val); activityPage = 1; }}
            />
          </div>
          <div class="data-page-nav" role="group" aria-label="模型响应调用量分页">
            <button type="button" class="btn-secondary btn-page" disabled={currentActivityPage <= 1} onclick={() => (activityPage = 1)}>首页</button>
            <button type="button" class="btn-secondary btn-page" disabled={currentActivityPage <= 1} onclick={() => (activityPage = currentActivityPage - 1)}>上一页</button>
            <span class="data-page-indicator tabular">第 <strong>{currentActivityPage}</strong> / {activityTotalPages} 页</span>
            <button type="button" class="btn-secondary btn-page" disabled={currentActivityPage >= activityTotalPages} onclick={() => (activityPage = currentActivityPage + 1)}>下一页</button>
            <button type="button" class="btn-secondary btn-page" disabled={currentActivityPage >= activityTotalPages} onclick={() => (activityPage = activityTotalPages)}>末页</button>
          </div>
        </div>
      </div>
    {:else}
      <div class="empty-box">暂无响应次数记录。</div>
    {/if}
  </section>
</div>

<style>
  .models-stack {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .models-panel {
    padding: 12px 16px;
  }

  .models-panel .panel-head {
    margin-bottom: 8px;
  }

  .models-rank-list {
    gap: 8px;
  }

  .models-rank-item {
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .models-rank-item:last-child {
    padding-bottom: 0;
    border-bottom: none;
  }

  .models-rank-foot {
    margin-top: 4px;
    font-size: 11px;
  }

  .models-activity-table th {
    padding: 7px 14px;
  }

  .models-activity-table td {
    padding: 6px 14px;
  }
</style>
