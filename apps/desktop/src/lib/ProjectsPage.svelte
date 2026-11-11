<script lang="ts">
  import { formatCompactTokens } from '../lib/dashboardView';
  import { formatNumber, percentOf } from '../lib/format';
  import type { DashboardSnapshot, UsageBreakdown, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
  export let projects: UsageBreakdown[] | undefined = undefined;

  let groupMode: 'merged' | 'raw' = 'merged';
  let copiedKey = '';

  $: rawProjects = projects ?? snapshot.by_project;
  $: workspaceProjects = rawProjects.filter((project) => !isUnassignedProject(project));
  $: unassignedProjectTokens = rawProjects
    .filter(isUnassignedProject)
    .reduce((sum, project) => sum + project.total_tokens, 0);
  $: unassignedTokens = unassignedProjectTokens + snapshot.unassigned.total_tokens;

  $: localProjectKeys = new Set(snapshot.by_project.map((p) => p.key));
  $: remoteProjectKeys = new Set(snapshot.remote_by_project.map((p) => p.key));

  interface DisplayProject extends UsageBreakdown {
    isAnonymous: boolean;
    workspaceKeys: string[];
    hasLocal: boolean;
    hasRemote: boolean;
  }

  function isUnassignedProject(project: Pick<UsageBreakdown, 'key' | 'label'>): boolean {
    return project.key === 'no-project' || project.label === '未关联项目';
  }

  function isUnnamedLabel(label: string, key: string): boolean {
    if (!label || label === '项目名称未提供' || label === 'no-project') {
      return true;
    }
    return label === key && key.startsWith('prj_');
  }

  function shortHash(key: string): string {
    if (!key) return '';
    if (key.startsWith('prj_') && key.length > 14) {
      return `#${key.slice(4, 10)}`;
    }
    if (key.length > 14) {
      return `${key.slice(0, 6)}…${key.slice(-4)}`;
    }
    return key;
  }

  async function copyProjectKey(key: string) {
    try {
      if (typeof navigator !== 'undefined' && navigator.clipboard) {
        await navigator.clipboard.writeText(key);
        copiedKey = key;
        setTimeout(() => {
          if (copiedKey === key) copiedKey = '';
        }, 1600);
      }
    } catch {
      // ignore clipboard errors
    }
  }

  $: displayProjects = ((): DisplayProject[] => {
    if (groupMode === 'raw') {
      return workspaceProjects.map((p) => {
        const anon = isUnnamedLabel(p.label, p.key);
        return {
          ...p,
          label: anon ? `未命名工作区 ${shortHash(p.key)}` : p.label,
          isAnonymous: anon,
          workspaceKeys: [p.key],
          hasLocal: localProjectKeys.has(p.key),
          hasRemote: remoteProjectKeys.has(p.key),
        };
      });
    }

    const byGroup = new Map<string, DisplayProject>();
    for (const p of workspaceProjects) {
      const anon = isUnnamedLabel(p.label, p.key);
      const groupId = anon ? '__anonymous_group__' : `name:${p.label.trim().toLowerCase()}`;
      const existing = byGroup.get(groupId);
      const hasLocal = localProjectKeys.has(p.key);
      const hasRemote = remoteProjectKeys.has(p.key);

      if (existing) {
        existing.total_tokens += p.total_tokens;
        existing.record_count += p.record_count;
        existing.exact_records += p.exact_records;
        existing.aggregate_records += p.aggregate_records;
        existing.estimated_records += p.estimated_records;
        existing.unknown_precision_records += p.unknown_precision_records;
        existing.untimed_records += p.untimed_records;
        if (p.estimated_cost_microusd !== null) {
          existing.estimated_cost_microusd =
            (existing.estimated_cost_microusd ?? 0) + p.estimated_cost_microusd;
        }
        if (!existing.workspaceKeys.includes(p.key)) {
          existing.workspaceKeys.push(p.key);
        }
        existing.hasLocal = existing.hasLocal || hasLocal;
        existing.hasRemote = existing.hasRemote || hasRemote;
      } else {
        byGroup.set(groupId, {
          ...p,
          label: anon ? '未命名 / 尚无项目名称' : p.label,
          isAnonymous: anon,
          workspaceKeys: [p.key],
          hasLocal,
          hasRemote,
        });
      }
    }

    return Array.from(byGroup.values()).sort((a, b) => {
      // Keep named projects ranked by tokens, and if tied, named before anonymous
      if (b.total_tokens !== a.total_tokens) return b.total_tokens - a.total_tokens;
      return Number(a.isAnonymous) - Number(b.isAnonymous);
    });
  })();

  $: namedProjectCount = displayProjects.filter((p) => !p.isAnonymous).length;
  $: totalProjectTokens = workspaceProjects.reduce((sum, prj) => sum + prj.total_tokens, 0);
  $: shareDenominator = Math.max(local?.total_tokens ?? 0, totalProjectTokens, 1);
  $: topProjectTokens = Math.max(1, ...displayProjects.map((p) => p.total_tokens));
</script>

<div class="page-header">
  <h1>项目分析</h1>
  {#if workspaceProjects.length > 0}
    <div class="range-capsule" role="group" aria-label="项目展示聚合方式">
      <button
        type="button"
        class="range-btn"
        class:selected={groupMode === 'merged'}
        aria-pressed={groupMode === 'merged'}
        onclick={() => (groupMode = 'merged')}
      >
        智能归并 ({displayProjects.length})
      </button>
      <button
        type="button"
        class="range-btn"
        class:selected={groupMode === 'raw'}
        aria-pressed={groupMode === 'raw'}
        onclick={() => (groupMode = 'raw')}
      >
        按工作区展开 ({workspaceProjects.length})
      </button>
    </div>
  {/if}
</div>

<!-- Project Summary Strip -->
<div class="device-summary-strip" style="margin-bottom: 16px;">
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">已识别项目数</span>
      {#if namedProjectCount > 0}
        <span class="precision-chip quota">{namedProjectCount} 个具名项目</span>
      {/if}
    </div>
    <strong class="tabular">{displayProjects.length}</strong>
    <span class="summary-foot">统计 {workspaceProjects.length} 个项目工作区</span>
  </div>
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">项目关联 Token</span>
      <span class="precision-chip exact">占 {percentOf(totalProjectTokens, shareDenominator).toFixed(1)}%</span>
    </div>
    <strong class="tabular">{formatNumber(totalProjectTokens)}</strong>
    <span class="summary-foot">已归属至具体工作区消耗</span>
  </div>
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">未关联项目 / 全局</span>
      <span class="precision-chip muted">保护隔离</span>
    </div>
    <strong class="tabular">{formatNumber(unassignedTokens)}</strong>
    <span class="summary-foot">未关联工作区的用量，以及独立会话或全局命令</span>
  </div>
</div>

<section class="panel-card">
  <div class="panel-head">
    <div>
      <h2>项目用量矩阵</h2>
    </div>
    <div class="panel-head-actions">
      <span class="panel-tag">{displayProjects.length} 个项目条目</span>
      {#if workspaceProjects.length > 0}
        <div class="trend-mode-switch" role="group" aria-label="项目展示聚合方式">
          <button
            type="button"
            class="trend-mode-btn"
            class:selected={groupMode === 'merged'}
            aria-pressed={groupMode === 'merged'}
            onclick={() => (groupMode = 'merged')}
          >
            智能归并
          </button>
          <button
            type="button"
            class="trend-mode-btn"
            class:selected={groupMode === 'raw'}
            aria-pressed={groupMode === 'raw'}
            onclick={() => (groupMode = 'raw')}
          >
            按工作区 ({workspaceProjects.length})
          </button>
        </div>
      {/if}
    </div>
  </div>

  {#if displayProjects.length > 0}
    <div class="table-wrap">
      <table class="project-table">
        <thead>
          <tr>
            <th class="col-prj-rank">#</th>
            <th class="col-prj-name">项目名称 / 工作区标识</th>
            <th class="col-prj-status">来源状态</th>
            <th class="col-prj-tokens tabular">Token 消耗量</th>
            <th class="col-prj-records tabular">记录数</th>
            <th class="col-prj-avg tabular">单笔均值</th>
            <th class="col-prj-share tabular">份额占比</th>
          </tr>
        </thead>
        <tbody>
          {#each displayProjects as prj, idx}
            {@const avgTokens = prj.record_count > 0 ? Math.round(prj.total_tokens / prj.record_count) : 0}
            <tr class:project-row-anon={prj.isAnonymous}>
              <td><span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span></td>
              <td>
                <div class="project-identity-cell">
                  <strong class="project-title" class:anon={prj.isAnonymous} title={prj.label}>
                    {prj.label}
                  </strong>

                  {#if prj.workspaceKeys.length === 1}
                    {@const singleKey = prj.workspaceKeys[0]}
                    {#if singleKey && singleKey !== prj.label}
                      <button
                        type="button"
                        class="project-hash-pill"
                        title={`点击复制完整标识：${singleKey}`}
                        onclick={() => copyProjectKey(singleKey)}
                      >
                        {copiedKey === singleKey ? '已复制' : shortHash(singleKey)}
                      </button>
                    {/if}
                  {:else if prj.workspaceKeys.length > 1}
                    <span
                      class="project-workspace-badge"
                      title={`合并自 ${prj.workspaceKeys.length} 个工作区：\n${prj.workspaceKeys.join('\n')}`}
                    >
                      {prj.workspaceKeys.length} 个工作区
                    </span>
                  {/if}
                </div>
              </td>
              <td>
                <div class="project-status-cell">
                  {#if prj.hasLocal && prj.hasRemote}
                    <span class="precision-chip quota">本机 + 远端</span>
                  {:else if prj.hasRemote && !prj.hasLocal}
                    <span class="precision-chip muted" title="项目用量来自另一台已配对设备">远端设备</span>
                  {:else if prj.isAnonymous}
                    <span class="precision-chip muted" title="采集来源未提供可显示的项目名称">缺少项目名称</span>
                  {:else}
                    <span class="precision-chip exact">本机工作区</span>
                  {/if}
                </div>
              </td>
              <td class="tabular"><strong class="project-token-val">{formatNumber(prj.total_tokens)}</strong></td>
              <td class="tabular">{formatNumber(prj.record_count)}</td>
              <td class="tabular" style="color: var(--text-muted);">{formatCompactTokens(avgTokens)}/笔</td>
              <td class="tabular">
                <div class="table-track-cell">
                  <div class="rank-track">
                    <div class="rank-fill" style={`width: ${percentOf(prj.total_tokens, topProjectTokens)}%`}></div>
                  </div>
                  <span class="rank-share tabular">{percentOf(prj.total_tokens, shareDenominator).toFixed(1)}%</span>
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  {:else}
    <div class="session-empty-state" style="min-height: 160px; padding: 32px 20px;">
      <strong>当前范围尚未解析出项目标识</strong>
    </div>
  {/if}
</section>
