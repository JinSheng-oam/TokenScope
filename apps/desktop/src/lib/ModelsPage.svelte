<script lang="ts">
  import { formatCost, formatNumber, formatToolLabel, percentOf } from '../lib/format';
  import type { ModelSortBy } from '../lib/dashboardView';
  import type { DashboardSnapshot, UsageBreakdown } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let sortedModels: UsageBreakdown[];
  export let topModelTotal: number;
  export let modelSortBy: ModelSortBy;

  $: allModelTokens = sortedModels.reduce((sum, m) => sum + m.total_tokens, 0);
</script>

<div class="page-header">
  <div>
    <h1>模型排行与用量分析</h1>
    <p>来源未提供模型标识的 Token 会单独列出并计入总量；不猜测具体模型</p>
  </div>
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

<section class="panel-card">
  <div class="panel-head">
    <div>
      <h2>模型消耗总榜</h2>
      <p>按{modelSortBy === 'tokens' ? 'Token 用量' : '估算支出'}降序排列；支持全模型用量追踪</p>
    </div>
    <span class="panel-tag">{sortedModels.length} 个模型分组</span>
  </div>

  {#if sortedModels.length > 0}
    <div class="rank-list" style="gap: 14px;">
      {#each sortedModels as item, index}
        <div class="rank-item" style="padding-bottom: 12px; border-bottom: 1px solid var(--border-subtle);">
          <div class="rank-row">
            <span class="rank-title" style="min-width: 0;">
              <span class="rank-num tabular">{String(index + 1).padStart(2, '0')}</span>
              <strong style="overflow: hidden; text-overflow: ellipsis; white-space: nowrap;" title={item.label}>
                {item.label}
              </strong>
            </span>
            <span class="rank-meta">
              <span class="rank-value tabular">{formatNumber(item.total_tokens)}</span>
              {#if item.estimated_cost_microusd != null}
                <span class="panel-tag" style="font-size: 10.5px; font-weight: 500;">
                  {formatCost(item.estimated_cost_microusd)}
                </span>
              {/if}
              <span class="rank-share tabular">{percentOf(item.total_tokens, allModelTokens).toFixed(1)}%</span>
            </span>
          </div>
          <div class="rank-track">
            <div class="rank-fill" style={`width: ${percentOf(item.total_tokens, topModelTotal)}%`}></div>
          </div>
          <div class="metric-foot" style="margin-top: 6px; font-size: 11px;">
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
  {:else}
    <div class="empty-box">暂无模型记录</div>
  {/if}
</section>

<section class="panel-card" style="margin-top: 16px;">
  <div class="panel-head">
    <div>
      <h2>模型响应与调用量</h2>
      <p>按来源展示可验证的响应次数；不同来源的时间范围会分别标注</p>
    </div>
    <span class="precision-chip quota">响应活动账本</span>
  </div>

  {#if snapshot.model_activity.length > 0}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th style="width: 140px;">来源工具</th>
            <th>模型名称</th>
            <th class="tabular" style="width: 140px;">调用量</th>
            <th style="width: 130px;">统计范围</th>
          </tr>
        </thead>
        <tbody>
          {#each snapshot.model_activity as activity}
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
  {:else}
    <div class="empty-box">暂无可验证的模型响应次数；历史聚合 Token 记录不会被当作调用次数。</div>
  {/if}
</section>
