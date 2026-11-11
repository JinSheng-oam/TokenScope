<script lang="ts">
  import { displayCurrency } from './displayCurrency';
  import { formatCost, formatNumber, formatToolLabel, percentOf, relativeTime } from '../lib/format';
  import type { DashboardSnapshot, UsageBreakdown, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
  export let tools: UsageBreakdown[] | undefined = undefined;
  $: activeTools = tools ?? snapshot.by_tool;

  $: completeCount = snapshot.source_health.filter((s) => s.status === 'complete').length;
</script>

<div class="page-header">
  <h1>工具分析</h1>
</div>

<section class="panel-card" style="margin-bottom: 16px;">
  <div class="panel-head">
    <div>
      <h2>工具用量详细矩阵</h2>
    </div>
    <span class="panel-tag">{activeTools.length} 个本地来源</span>
  </div>

  {#if activeTools.length > 0}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th style="width: 48px;">#</th>
            <th>工具名称</th>
            <th class="tabular">Token 总量</th>
            <th class="tabular">用量记录数</th>
            <th class="tabular">估算成本</th>
            <th class="tabular" style="width: 180px;">全机占比</th>
          </tr>
        </thead>
        <tbody>
          {#each activeTools as tool, idx}
            <tr>
              <td><span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span></td>
              <td>
                <div style="display: flex; align-items: baseline; gap: 6px;">
                  <strong>{tool.label}</strong>
                  {#if tool.key && tool.key.toLowerCase() !== tool.label.toLowerCase()}
                    <code style="font-size: 11px; color: var(--text-muted);">{tool.key}</code>
                  {/if}
                </div>
              </td>
              <td class="tabular"><strong>{formatNumber(tool.total_tokens)}</strong></td>
              <td class="tabular">{formatNumber(tool.record_count)}</td>
              <td class="tabular">{formatCost(tool.estimated_cost_microusd, $displayCurrency)}</td>
              <td class="tabular">
                <div class="table-track-cell">
                  <div class="rank-track">
                    <div class="rank-fill" style={`width: ${percentOf(tool.total_tokens, local?.total_tokens ?? 0)}%`}></div>
                  </div>
                  <span class="rank-share tabular">{percentOf(tool.total_tokens, local?.total_tokens ?? 0).toFixed(1)}%</span>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="empty-box">暂无工具统计记录</div>
  {/if}
</section>

<!-- Collector Health -->
<section class="panel-card">
  <div class="panel-head">
    <div>
      <h2>采集器健康度与检查点</h2>
    </div>
    <span class="panel-tag">{completeCount} / {snapshot.source_health.length} 状态正常</span>
  </div>

  {#if snapshot.source_health.length > 0}
    <div class="form-grid" style="grid-template-columns: repeat(auto-fill, minmax(260px, 1fr));">
      {#each snapshot.source_health as source}
        <div class="metric-card" style="padding: 12px 14px; gap: 8px;">
          <div class="metric-head" style="align-items: center;">
            <div style="display: flex; align-items: center; gap: 7px; min-width: 0;">
              <span
                class="source-status-dot"
                class:complete={source.status === 'complete'}
                class:partial={source.status === 'partial'}
                class:degraded={source.status === 'degraded'}
              ></span>
              <strong style="font-size: 13px; color: var(--text-primary);">{formatToolLabel(source.tool_id)}</strong>
            </div>
            <span
              class="precision-chip"
              class:exact={source.status === 'complete'}
              class:estimated={source.status === 'partial'}
              class:danger={source.status === 'degraded'}
            >
              {source.status === 'complete' ? '正常' : source.status === 'partial' ? '部分' : '异常'}
            </span>
          </div>

          <div style="display: flex; flex-direction: column; gap: 4px; font-size: 11.5px; color: var(--text-secondary); margin-top: 4px;">
            <div style="display: flex; justify-content: space-between;">
              <span style="color: var(--text-muted);">最近扫描</span>
              <span>{relativeTime(source.last_scan_ms)}</span>
            </div>
            <div style="display: flex; justify-content: space-between;">
              <span style="color: var(--text-muted);">完整扫描</span>
              <span>{relativeTime(source.last_complete_scan_ms)}</span>
            </div>
            <div style="display: flex; justify-content: space-between;">
              <span style="color: var(--text-muted);">覆盖来源</span>
              <span class="tabular">{formatNumber(source.source_count)} 个</span>
            </div>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty-box">暂无来源检查点记录</div>
  {/if}
</section>
