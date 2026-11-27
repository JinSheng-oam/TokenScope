<script lang="ts">
  import { icons } from './icons';
  import { ALL_PROVIDERS } from './providerBadges';
  import {
    deriveCardBrief,
    isSkillAnalyzable,
    sourceTypeLabel,
    type SkillLibraryItem,
  } from './skillLibraryTypes';
  import type { BatchItemState } from './skillBatchAnalyzer';

  export let item: SkillLibraryItem;
  export let batchState: BatchItemState | undefined = undefined;
  export let multiSelect = false;
  export let selected = false;
  export let busy = false;
  export let onOpen: (item: SkillLibraryItem) => void = () => {};
  export let onToggleEnabled: (item: SkillLibraryItem) => void = () => {};
  export let onToggleSelect: (key: string) => void = () => {};
  export let onAnalyzeSingle: (item: SkillLibraryItem) => void = () => {};

  $: brief = deriveCardBrief(item);
  $: analyzable = isSkillAnalyzable(item);
  $: isRunning = batchState?.status === 'running' || batchState?.status === 'retrying';
  $: isQueued = batchState?.status === 'queued';
  $: isFailed = batchState?.status === 'failed';
  $: deploymentMap = new Map(item.deployments.map((d) => [d.provider, d]));
  $: groupTag =
    item.source.source_group && item.source.source_group !== '未分类' && item.source.source_group !== '内置技能'
      ? item.source.source_group
      : item.linked
        ? '软链接'
        : 'Default';

  function handleCardClick() {
    if (multiSelect) {
      onToggleSelect(item.key);
    } else {
      onOpen(item);
    }
  }
</script>

<div
  class="skill-card"
  class:is-selected={multiSelect && selected}
  class:is-disabled={!item.enabled}
  class:is-analyzing={isRunning}
  class:is-failed={isFailed}
  role="button"
  tabindex="0"
  onclick={handleCardClick}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleCardClick();
    }
  }}
>
  <div class="card-head">
    <div class="card-title-wrap">
      <span class="status-dot" class:enabled={item.enabled}></span>
      <strong class="card-name" title={item.name}>{item.name}</strong>
    </div>

    <div class="card-head-right">
      {#if analyzable && !multiSelect}
        <button
          type="button"
          class="card-ai-btn"
          class:always-show={isFailed || isRunning}
          class:failed={isFailed}
          disabled={isRunning || isQueued}
          title={isFailed
            ? `分析失败：${batchState?.error ?? ''}（点击重试）`
            : item.summary
              ? '重新 AI 分析'
              : '用 AI 生成简短与详细分析'}
          onclick={(e) => {
            e.stopPropagation();
            onAnalyzeSingle(item);
          }}
        >
          <span class="ai-icon" class:spin={isRunning} aria-hidden="true">{@html icons.sparkle}</span>
        </button>
      {/if}

      {#if multiSelect}
        <input
          type="checkbox"
          class="select-box"
          checked={selected}
          aria-label={`选择 ${item.name}`}
          onclick={(e) => e.stopPropagation()}
          onchange={() => onToggleSelect(item.key)}
        />
      {:else if !item.builtin}
        <button
          type="button"
          class="skill-switch"
          role="switch"
          aria-checked={item.enabled}
          aria-label={item.enabled ? `停用 ${item.name}` : `启用 ${item.name}`}
          disabled={busy}
          onclick={(e) => {
            e.stopPropagation();
            onToggleEnabled(item);
          }}
        >
          <span></span>
        </button>
      {:else}
        <span class="builtin-tag">内置</span>
      {/if}
    </div>
  </div>

  <div class="card-body">
    {#if isRunning}
      <div class="live-status-row running">
        <span class="live-dot"></span>
        <span>
          {batchState?.status === 'retrying'
            ? `AI 分析重试中 (${batchState.attempt}/${batchState.maxRetries})…`
            : 'AI 正在分析技能用途…'}
        </span>
      </div>
    {:else if isQueued}
      <div class="live-status-row queued">
        <span>已加入批量队列，等待分析…</span>
      </div>
    {:else if isFailed}
      <div class="live-status-row failed" title={batchState?.error}>
        <span class="failed-text">分析失败：{batchState?.error ?? '请求出错'}</span>
        <button
          type="button"
          class="inline-retry-btn"
          onclick={(e) => {
            e.stopPropagation();
            onAnalyzeSingle(item);
          }}
        >
          重试
        </button>
      </div>
    {/if}

    <p class="card-brief" class:is-ai={brief.isAi} class:is-empty={!brief.text} title={brief.text || undefined}>
      {#if brief.isAi}
        <span class="brief-ai-mark" title="AI 智能分析简介">{@html icons.sparkle}</span>
      {/if}
      <span>{brief.text || '暂无简介，点击右上角 AI 按钮生成中文功能摘要'}</span>
    </p>
  </div>

  <div class="card-foot">
    <div class="card-source">
      <span class="source-pill">{sourceTypeLabel(item.source.source_type)}</span>
      <span class="source-dot">·</span>
      <span class="source-group" title={groupTag}>{groupTag}</span>
      {#if !item.summary && analyzable && !isRunning && !isQueued && !isFailed}
        <span class="unanalyzed-dot" title="尚未进行 AI 分析">未分析</span>
      {/if}
    </div>

    <div class="provider-strip" aria-label="已部署工具">
      {#each ALL_PROVIDERS as p (p.id)}
        {@const dep = deploymentMap.get(p.id)}
        {#if dep}
          <span
            class="provider-chip"
            class:inactive={!dep.enabled}
            style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}
            title={`${p.label}：${dep.enabled ? '已启用' : '已停用'}`}
          >
            {@html p.icon}
          </span>
        {/if}
      {/each}
    </div>
  </div>
</div>

<style>
  .skill-card {
    position: relative;
    display: flex;
    flex-direction: column;
    justify-content: space-between;
    gap: 10px;
    padding: 13px 14px 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    cursor: pointer;
    overflow: hidden;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast), transform var(--transition-fast);
    min-height: 124px;
  }

  .skill-card:hover {
    border-color: var(--brand-border);
    box-shadow: 0 4px 14px rgba(15, 23, 42, 0.06);
  }

  .skill-card:focus-visible {
    outline: 2px solid var(--brand-primary);
    outline-offset: 1px;
  }

  .skill-card.is-selected {
    border-color: var(--brand-primary);
    background: color-mix(in srgb, var(--brand-subtle) 55%, var(--bg-card));
  }

  .skill-card.is-disabled {
    opacity: 0.78;
  }

  .skill-card.is-failed {
    border-color: var(--color-danger-border);
  }

  .skill-card.is-analyzing {
    border-color: var(--brand-border);
  }

  .skill-card.is-analyzing::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(
      110deg,
      transparent 20%,
      rgba(79, 91, 213, 0.09) 46%,
      rgba(255, 255, 255, 0.35) 52%,
      rgba(79, 91, 213, 0.09) 58%,
      transparent 82%
    );
    animation: skill-card-sweep 1.5s linear infinite;
  }

  @keyframes skill-card-sweep {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(100%); }
  }

  .card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    min-width: 0;
  }

  .card-title-wrap {
    display: flex;
    align-items: center;
    gap: 7px;
    min-width: 0;
    flex: 1;
  }

  .status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-muted);
    flex-shrink: 0;
  }

  .status-dot.enabled {
    background: var(--color-exact);
  }

  .card-name {
    font-size: 13px;
    font-weight: 650;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    line-height: 1.3;
  }

  .card-head-right {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .card-ai-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--brand-primary);
    cursor: pointer;
    opacity: 0;
    transition: all var(--transition-fast);
  }

  .skill-card:hover .card-ai-btn,
  .card-ai-btn:focus-visible,
  .card-ai-btn.always-show {
    opacity: 1;
  }

  .card-ai-btn:hover:not(:disabled) {
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .card-ai-btn.failed {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }

  .ai-icon {
    display: inline-flex;
    width: 13px;
    height: 13px;
  }

  .ai-icon :global(svg),
  .brief-ai-mark :global(svg),
  .provider-chip :global(svg) {
    width: 100%;
    height: 100%;
  }

  .ai-icon.spin {
    animation: spin 1.1s linear infinite;
  }

  .select-box {
    width: 16px;
    height: 16px;
    accent-color: var(--brand-primary);
    cursor: pointer;
  }

  .skill-switch {
    display: flex;
    align-items: center;
    width: 34px;
    height: 20px;
    padding: 2px;
    border: none;
    border-radius: 999px;
    background: var(--border-strong);
    cursor: pointer;
    transition: background-color 150ms ease;
    flex-shrink: 0;
  }

  .skill-switch span {
    display: block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.16);
    transition: transform 150ms ease;
  }

  .skill-switch[aria-checked='true'] {
    background: #22c55e;
  }

  .skill-switch[aria-checked='true'] span {
    transform: translateX(14px);
  }

  .skill-switch:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .builtin-tag {
    padding: 1px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 500;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
  }

  .card-body {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 5px;
    min-height: 36px;
  }

  .live-status-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    padding: 3px 7px;
    border-radius: var(--radius-xs);
    font-size: 10.5px;
    font-weight: 500;
  }

  .live-status-row.running {
    background: var(--brand-subtle);
    color: var(--brand-strong);
    justify-content: flex-start;
  }

  .live-status-row.queued {
    background: var(--bg-secondary);
    color: var(--text-muted);
    justify-content: flex-start;
  }

  .live-status-row.failed {
    background: var(--color-danger-bg);
    color: var(--color-danger);
  }

  .live-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--brand-primary);
    animation: pulse-dot 1s ease-in-out infinite;
    flex-shrink: 0;
  }

  @keyframes pulse-dot {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }

  .failed-text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .inline-retry-btn {
    height: 18px;
    padding: 0 6px;
    border: 1px solid var(--color-danger-border);
    border-radius: 4px;
    background: var(--bg-card);
    color: var(--color-danger);
    font: inherit;
    font-size: 10px;
    font-weight: 600;
    cursor: pointer;
    flex-shrink: 0;
  }

  .card-brief {
    margin: 0;
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .card-brief.is-ai {
    color: var(--text-primary);
  }

  .card-brief.is-empty {
    color: var(--text-muted);
    font-size: 11.5px;
  }

  .brief-ai-mark {
    display: inline-flex;
    width: 11px;
    height: 11px;
    color: var(--brand-primary);
    vertical-align: -1px;
    margin-right: 4px;
  }

  .card-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 8px;
    border-top: 1px solid color-mix(in srgb, var(--border-subtle) 75%, transparent);
    min-width: 0;
  }

  .card-source {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    font-size: 11px;
    color: var(--text-muted);
  }

  .source-pill {
    white-space: nowrap;
    flex-shrink: 0;
  }

  .source-dot {
    color: var(--border-default);
    flex-shrink: 0;
  }

  .source-group {
    color: var(--color-estimated);
    font-weight: 500;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .unanalyzed-dot {
    padding: 0 5px;
    border-radius: 999px;
    background: var(--bg-secondary);
    color: var(--text-muted);
    font-size: 9.5px;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .provider-strip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
  }

  .provider-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 19px;
    height: 19px;
    padding: 3px;
    border-radius: 5px;
    color: var(--p-color);
    background: var(--p-bg);
  }

  .provider-chip.inactive {
    opacity: 0.35;
    filter: grayscale(1);
  }
</style>
