<script lang="ts">
import { formatCost, formatNumber, percentOf } from '../lib/format';
  import type { ModelSortBy } from '../lib/dashboardView';
  import type { DashboardSnapshot, UsageBreakdown } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let sortedModels: UsageBreakdown[];
  export let topModelTotal: number;
  export let modelSortBy: ModelSortBy;
</script>

<div class="page-header">
            <div>
              <h1>模型排行与用量分析</h1>
              <p>严格遵循 Unicode NFC 规范化；真实来源未提供之模型不进行主观猜测</p>
            </div>
            <div class="range-capsule">
              <button class="range-btn" class:selected={modelSortBy === 'tokens'} aria-pressed={modelSortBy === 'tokens'} onclick={() => (modelSortBy = 'tokens')}>
                按 Token 排序
              </button>
              <button class="range-btn" class:selected={modelSortBy === 'cost'} aria-pressed={modelSortBy === 'cost'} onclick={() => (modelSortBy = 'cost')}>
                按 估算成本 排序
              </button>
            </div>
          </div>

          <section class="panel-card">
            {#if sortedModels.length > 0}
              <div class="rank-list">
                {#each sortedModels as item, index}
                  <div class="rank-item" style="padding: 10px 0; border-bottom: 1px solid var(--border-subtle);">
                    <div class="rank-row">
                      <span class="rank-title">
                        <span class="rank-num tabular">{String(index + 1).padStart(2, '0')}</span>
                        <strong>{item.label}</strong>
                      </span>
                      <span class="rank-value tabular">
                        {formatNumber(item.total_tokens)} Token
                        <span style="font-size: 11px; color: var(--text-muted); margin-left: 8px;">
                          ({formatCost(item.estimated_cost_microusd)})
                        </span>
                      </span>
                    </div>
                    <div class="rank-track">
                      <div class="rank-fill" style={`width: ${percentOf(item.total_tokens, topModelTotal)}%`}></div>
                    </div>
                    <div class="metric-foot">
                      <span>用量记录 {formatNumber(item.record_count)} 笔 · 精确 {formatNumber(item.exact_records)} · 聚合 {formatNumber(item.aggregate_records)} · 估算 {formatNumber(item.estimated_records)}</span>
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
                <h2>模型调用量</h2>
                <p>按来源展示可验证的响应次数；不同来源的时间范围会分别标注</p>
              </div>
              <span class="precision-chip quota">响应活动</span>
            </div>
            {#if snapshot.model_activity.length > 0}
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr><th>来源</th><th>模型</th><th>调用量</th><th>统计范围</th></tr>
                  </thead>
                  <tbody>
                    {#each snapshot.model_activity as activity}
                      <tr>
                        <td>{activity.tool_id === 'codex' ? 'Codex' : activity.tool_id === 'antigravity' ? 'Antigravity' : activity.tool_id}</td>
                        <td><strong>{activity.model_id === 'unknown-model' ? '未识别模型' : activity.model_id}</strong></td>
                        <td class="tabular">{formatNumber(activity.response_count)}</td>
                        <td>{activity.time_scope === 'selected_range' ? '当前时间范围' : '来源累计'}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="empty-box">暂无可验证的模型响应次数；历史聚合 Token 记录不会被当作调用次数。</div>
            {/if}
          </section>
