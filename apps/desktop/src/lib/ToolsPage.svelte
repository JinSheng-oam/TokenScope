<script lang="ts">
import { formatCost, formatNumber, percentOf, relativeTime } from '../lib/format';
  import type { DashboardSnapshot, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
</script>

<div class="page-header">
            <h1>工具分析</h1>
            <p>已接入 AI 工具的原生解析器监控、扫描健康状态与用量汇总</p>
          </div>

          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="panel-head">
              <h2>工具用量详细矩阵</h2>
              <span class="panel-tag">{snapshot.by_tool.length} 个本地来源</span>
            </div>
            {#if snapshot.by_tool.length > 0}
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>工具名称</th>
                      <th>Token 总量</th>
                      <th>用量记录数</th>
                      <th>估算成本</th>
                      <th>全机占比</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each snapshot.by_tool as tool}
                      <tr>
                        <td><strong>{tool.label}</strong></td>
                        <td class="tabular">{formatNumber(tool.total_tokens)}</td>
                        <td class="tabular">{formatNumber(tool.record_count)}</td>
                        <td class="tabular">{formatCost(tool.estimated_cost_microusd)}</td>
                        <td>
                          <div class="rank-track" style="width: 100px;">
                            <div class="rank-fill" style={`width: ${percentOf(tool.total_tokens, local?.total_tokens ?? 0)}%`}></div>
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
                <p>各工具最近扫描与完整事务写入时间戳</p>
              </div>
            </div>
            {#if snapshot.source_health.length > 0}
              <div class="form-grid">
                {#each snapshot.source_health as source}
                  <div class="metric-card">
                    <div class="metric-head">
                      <strong>{source.tool_id}</strong>
                      <span class="precision-chip" class:exact={source.status === 'complete'} class:danger={source.status === 'degraded'}>
                        {source.status === 'complete' ? '正常' : source.status === 'partial' ? '部分' : '异常'}
                      </span>
                    </div>
                    <div style="margin-top: 8px; font-size: 11.5px; color: var(--text-secondary);">
                      <div>最近扫描：{relativeTime(source.last_scan_ms)}</div>
                      <div>完整扫描：{relativeTime(source.last_complete_scan_ms)}</div>
                      <div>来源文件数：{source.source_count}</div>
                    </div>
                  </div>
                {/each}
              </div>
            {:else}
              <div class="empty-box">暂无来源检查点记录</div>
            {/if}
          </section>
