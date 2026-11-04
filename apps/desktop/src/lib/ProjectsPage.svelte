<script lang="ts">
  import { formatNumber, percentOf } from '../lib/format';
  import type { DashboardSnapshot, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;

  $: totalProjectTokens = snapshot.by_project.reduce((sum, prj) => sum + prj.total_tokens, 0);
  $: topProjectTokens = Math.max(1, ...snapshot.by_project.map((p) => p.total_tokens));
</script>

<div class="page-header">
  <div>
    <h1>项目分析</h1>
    <p>基于脱敏项目标识符归集；源码路径与仓库名留在本地，绝不上传</p>
  </div>
</div>

<!-- Project Summary Strip -->
<div class="device-summary-strip" style="margin-bottom: 16px;">
  <div class="summary-pill-card">
    <span class="summary-label">已识别项目数</span>
    <strong class="tabular">{snapshot.by_project.length}</strong>
    <span class="summary-foot">本地工作区脱敏归集</span>
  </div>
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">项目关联 Token</span>
      <span class="precision-chip exact">占 {percentOf(totalProjectTokens, local?.total_tokens ?? 0).toFixed(1)}%</span>
    </div>
    <strong class="tabular">{formatNumber(totalProjectTokens)}</strong>
    <span class="summary-foot">全机有效项目消耗</span>
  </div>
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">未关联项目 / 全局</span>
      <span class="precision-chip muted">保护隔离</span>
    </div>
    <strong class="tabular">{formatNumber(snapshot.unassigned.total_tokens)}</strong>
    <span class="summary-foot">独立会话或全局命令</span>
  </div>
</div>

<section class="panel-card">
  <div class="panel-head">
    <div>
      <h2>项目用量矩阵</h2>
      <p>根据代码工作区目录、Git 仓库或会话配置自动关联归集</p>
    </div>
    <span class="panel-tag">{snapshot.by_project.length} 个有效项目</span>
  </div>

  {#if snapshot.by_project.length > 0}
    <div class="table-wrap">
      <table>
        <thead>
          <tr>
            <th style="width: 48px;">#</th>
            <th>项目标识 / 目录名</th>
            <th class="tabular">Token 消耗量</th>
            <th class="tabular">用量记录数</th>
            <th class="tabular" style="width: 180px;">全机份额</th>
          </tr>
        </thead>
        <tbody>
          {#each snapshot.by_project as prj, idx}
            <tr>
              <td><span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span></td>
              <td>
                <div style="display: flex; align-items: center; gap: 7px;">
                  <strong title={prj.label}>{prj.label}</strong>
                  {#if prj.key && prj.key !== prj.label}
                    <code style="font-size: 11px; color: var(--text-muted);">{prj.key}</code>
                  {/if}
                </div>
              </td>
              <td class="tabular"><strong>{formatNumber(prj.total_tokens)}</strong></td>
              <td class="tabular">{formatNumber(prj.record_count)}</td>
              <td class="tabular">
                <div class="table-track-cell">
                  <div class="rank-track">
                    <div class="rank-fill" style={`width: ${percentOf(prj.total_tokens, topProjectTokens)}%`}></div>
                  </div>
                  <span class="rank-share tabular">{percentOf(prj.total_tokens, local?.total_tokens ?? 0).toFixed(1)}%</span>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="session-empty-state" style="min-height: 160px; padding: 32px 20px;">
      <strong>当前本地来源尚未解析出项目标识</strong>
      <p style="max-width: 60ch; line-height: 1.6; margin-top: 6px;">
        当在 VSCode、Cursor、Claude Code、Codex 或 Antigravity 中打开带 Git 仓库或工作区目录的项目时，TokenScope 会自动读取脱敏项目标识并汇集于此。
      </p>
    </div>
  {/if}
</section>
