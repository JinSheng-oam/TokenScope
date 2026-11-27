<script lang="ts">
  import { icons } from './icons';
  import type {
    BatchAnalysisMode,
    BatchAnalysisPrefs,
    BatchProgress,
  } from './skillBatchAnalyzer';

  export let mode: BatchAnalysisMode = 'unanalyzed';
  export let prefs: BatchAnalysisPrefs;
  export let progress: BatchProgress;
  export let unanalyzedCount = 0;
  export let analyzableCount = 0;
  export let selectedCount = 0;
  export let targetCount = 0;
  export let hasAiConfig = false;
  export let onUpdatePrefs: (next: BatchAnalysisPrefs) => void = () => {};
  export let onStart: () => void = () => {};
  export let onStop: () => void = () => {};
  export let onRetryFailed: () => void = () => {};
  export let onRetrySingle: (key: string) => void = () => {};
  export let onOpenAiConfig: () => void = () => {};
  export let onClose: () => void = () => {};

  let showFailures = false;

  $: percent = progress.total > 0 ? Math.round((progress.completed / progress.total) * 100) : 0;
  $: if (progress.failed > 0 && !progress.running) {
    showFailures = true;
  }

  function updateConcurrency(value: number) {
    onUpdatePrefs({ ...prefs, concurrency: value });
  }

  function updateRetries(value: number) {
    onUpdatePrefs({ ...prefs, maxRetries: value });
  }

  function updateSkipAnalyzed(checked: boolean) {
    onUpdatePrefs({ ...prefs, skipAnalyzedOnSelected: checked });
  }
</script>

<div class="batch-panel" role="region" aria-label="批量 AI 技能分析">
  <div class="batch-top">
    <div class="batch-left">
      <span class="batch-title">
        <span class="batch-icon" aria-hidden="true">{@html icons.sparkle}</span>
        <span>批量 AI 分析</span>
      </span>

      <div class="batch-mode-group" role="group" aria-label="分析范围">
        <button
          type="button"
          class="mode-btn"
          class:active={mode === 'unanalyzed'}
          disabled={progress.running}
          onclick={() => (mode = 'unanalyzed')}
        >
          仅未分析 ({unanalyzedCount})
        </button>
        {#if selectedCount > 0}
          <button
            type="button"
            class="mode-btn"
            class:active={mode === 'selected'}
            disabled={progress.running}
            onclick={() => (mode = 'selected')}
          >
            已选 ({selectedCount})
          </button>
        {/if}
        <button
          type="button"
          class="mode-btn"
          class:active={mode === 'all'}
          disabled={progress.running}
          onclick={() => (mode = 'all')}
        >
          全量 ({analyzableCount})
        </button>
      </div>

      {#if mode === 'selected'}
        <label class="skip-check">
          <input
            type="checkbox"
            checked={prefs.skipAnalyzedOnSelected}
            disabled={progress.running}
            onchange={(e) => updateSkipAnalyzed((e.currentTarget as HTMLInputElement).checked)}
          />
          <span>跳过已分析</span>
        </label>
      {/if}
    </div>

    <div class="batch-controls">
      <label class="batch-param" title="同时发起的 AI 分析请求数量">
        <span>并发</span>
        <select
          value={prefs.concurrency}
          disabled={progress.running}
          onchange={(e) => updateConcurrency(Number((e.currentTarget as HTMLSelectElement).value))}
        >
          {#each [1, 2, 3, 4, 5, 6, 8] as n}
            <option value={n}>{n}</option>
          {/each}
        </select>
      </label>

      <label class="batch-param" title="单条分析失败后的自动退避重试次数">
        <span>失败重试</span>
        <select
          value={prefs.maxRetries}
          disabled={progress.running}
          onchange={(e) => updateRetries(Number((e.currentTarget as HTMLSelectElement).value))}
        >
          {#each [0, 1, 2, 3] as n}
            <option value={n}>{n} 次</option>
          {/each}
        </select>
      </label>

      {#if !hasAiConfig}
        <button type="button" class="btn-secondary btn-xs" onclick={onOpenAiConfig}>
          先配置 AI 接口
        </button>
      {:else if progress.running}
        <button type="button" class="btn-danger btn-xs" onclick={onStop}>
          停止分析
        </button>
      {:else}
        <button
          type="button"
          class="btn-primary btn-xs"
          disabled={targetCount === 0}
          onclick={onStart}
        >
          开始分析 ({targetCount})
        </button>
      {/if}

      {#if !progress.running && progress.failed > 0}
        <button type="button" class="btn-secondary btn-xs retry-all-btn" onclick={onRetryFailed}>
          重试失败 ({progress.failed})
        </button>
      {/if}

      {#if !progress.running}
        <button
          type="button"
          class="batch-close-btn"
          onclick={onClose}
          title="收起批量面板"
          aria-label="收起批量面板"
        >
          {@html icons.close}
        </button>
      {/if}
    </div>
  </div>

  {#if progress.total > 0}
    <div class="batch-progress-wrap">
      <div class="progress-bar-track" role="progressbar" aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
        <div
          class="progress-bar-fill"
          class:is-running={progress.running}
          class:has-error={!progress.running && progress.failed > 0}
          style={`width: ${percent}%`}
        ></div>
      </div>

      <div class="progress-meta">
        <div class="progress-stats">
          <strong>{progress.completed} / {progress.total}</strong>
          <span class="pct">({percent}%)</span>
          <span class="stat-sep">·</span>
          <span class="stat-ok">成功 {progress.succeeded}</span>
          {#if progress.runningCount > 0}
            <span class="stat-sep">·</span>
            <span class="stat-run">分析中 {progress.runningCount}</span>
          {/if}
          {#if progress.retryingCount > 0}
            <span class="stat-sep">·</span>
            <span class="stat-retry">重试中 {progress.retryingCount}</span>
          {/if}
          {#if progress.failed > 0}
            <span class="stat-sep">·</span>
            <button type="button" class="stat-fail-btn" onclick={() => (showFailures = !showFailures)}>
              失败 {progress.failed} {showFailures ? '▴' : '▾'}
            </button>
          {/if}
          {#if progress.cancelled && !progress.running}
            <span class="stat-sep">·</span>
            <span class="stat-cancelled">已手动停止</span>
          {/if}
        </div>

        {#if progress.activeNames.length > 0}
          <div class="active-names" title={progress.activeNames.join('、')}>
            正在处理：{progress.activeNames.slice(0, 3).join('、')}{progress.activeNames.length > 3 ? ` 等 ${progress.activeNames.length} 项` : ''}
          </div>
        {/if}
      </div>

      {#if showFailures && progress.failedList.length > 0}
        <div class="failure-list">
          {#each progress.failedList as item (item.key)}
            <div class="failure-row">
              <div class="failure-info">
                <strong class="failure-name">{item.name}</strong>
                <span class="failure-err" title={item.error}>{item.error || '未知错误'}</span>
              </div>
              <button
                type="button"
                class="retry-single-btn"
                disabled={progress.running}
                onclick={() => onRetrySingle(item.key)}
              >
                重试
              </button>
            </div>
          {/each}
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .batch-panel {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 10px 14px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-md);
    background: color-mix(in srgb, var(--brand-subtle) 42%, var(--bg-card));
  }

  .batch-top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
  }

  .batch-left {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .batch-title {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12.5px;
    font-weight: 600;
    color: var(--brand-strong);
    white-space: nowrap;
  }

  .batch-icon {
    display: inline-flex;
    width: 14px;
    height: 14px;
  }

  .batch-icon :global(svg),
  .batch-close-btn :global(svg) {
    width: 100%;
    height: 100%;
  }

  .batch-mode-group {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .mode-btn {
    height: 24px;
    padding: 0 9px;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .mode-btn:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .mode-btn.active {
    background: var(--brand-subtle);
    border-color: var(--brand-border);
    color: var(--brand-text);
    font-weight: 600;
  }

  .skip-check {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .batch-controls {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .batch-param {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11.5px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .batch-param select {
    height: 26px;
    padding: 0 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    color: var(--text-primary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }

  .btn-xs {
    height: 26px;
    padding: 0 11px;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  .retry-all-btn {
    color: var(--color-danger);
    border-color: var(--color-danger-border);
    background: var(--color-danger-bg);
  }

  .batch-close-btn {
    width: 24px;
    height: 24px;
    padding: 5px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .batch-close-btn:hover {
    color: var(--text-primary);
    background: var(--bg-card);
  }

  .batch-progress-wrap {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .progress-bar-track {
    width: 100%;
    height: 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--bg-secondary) 85%, var(--bg-card));
    overflow: hidden;
  }

  .progress-bar-fill {
    height: 100%;
    border-radius: 999px;
    background: var(--brand-primary);
    transition: width 220ms ease;
    position: relative;
  }

  .progress-bar-fill.has-error {
    background: var(--color-estimated);
  }

  .progress-bar-fill.is-running::after {
    content: '';
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, transparent, rgba(255, 255, 255, 0.45), transparent);
    animation: batch-bar-shimmer 1.3s linear infinite;
  }

  @keyframes batch-bar-shimmer {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(100%); }
  }

  .progress-meta {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .progress-stats {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex-wrap: wrap;
    font-variant-numeric: tabular-nums;
  }

  .progress-stats strong {
    color: var(--text-primary);
  }

  .pct {
    color: var(--text-muted);
  }

  .stat-sep {
    color: var(--border-default);
  }

  .stat-ok {
    color: var(--color-exact);
    font-weight: 500;
  }

  .stat-run {
    color: var(--brand-strong);
    font-weight: 500;
  }

  .stat-retry {
    color: var(--color-estimated);
    font-weight: 500;
  }

  .stat-fail-btn {
    padding: 0;
    border: none;
    background: transparent;
    color: var(--color-danger);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }

  .stat-cancelled {
    color: var(--text-muted);
  }

  .active-names {
    font-size: 11px;
    color: var(--text-muted);
    max-width: 380px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .failure-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    max-height: 150px;
    overflow-y: auto;
    padding: 8px 10px;
    border: 1px solid var(--color-danger-border);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
  }

  .failure-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 11.5px;
  }

  .failure-info {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
    flex: 1;
  }

  .failure-name {
    color: var(--text-primary);
    flex-shrink: 0;
  }

  .failure-err {
    color: var(--color-danger);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .retry-single-btn {
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-secondary);
    color: var(--text-primary);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .retry-single-btn:hover:not(:disabled) {
    border-color: var(--brand-border);
    background: var(--brand-subtle);
    color: var(--brand-text);
  }
</style>
