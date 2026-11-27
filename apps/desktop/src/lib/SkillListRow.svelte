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
      : 'Default';

  function handleRowClick() {
    if (multiSelect) {
      onToggleSelect(item.key);
    } else {
      onOpen(item);
    }
  }
</script>

<div
  class="skill-row"
  class:is-selected={multiSelect && selected}
  class:is-disabled={!item.enabled}
  class:is-analyzing={isRunning}
  class:is-failed={isFailed}
  role="button"
  tabindex="0"
  onclick={handleRowClick}
  onkeydown={(e) => {
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault();
      handleRowClick();
    }
  }}
>
  <div class="row-name-col">
    {#if multiSelect}
      <input
        type="checkbox"
        class="select-box"
        checked={selected}
        aria-label={`选择 ${item.name}`}
        onclick={(e) => e.stopPropagation()}
        onchange={() => onToggleSelect(item.key)}
      />
    {:else}
      <span class="status-dot" class:enabled={item.enabled}></span>
    {/if}
    <strong class="row-name" title={item.name}>{item.name}</strong>
  </div>

  <div class="row-brief-col" title={brief.text || undefined}>
    {#if isRunning}
      <span class="row-state running">
        {batchState?.status === 'retrying'
          ? `重试 ${batchState.attempt}/${batchState.maxRetries}…`
          : 'AI 分析中…'}
      </span>
    {:else if isQueued}
      <span class="row-state queued">排队中</span>
    {:else if isFailed}
      <span class="row-state failed" title={batchState?.error}>分析失败</span>
    {:else if brief.isAi}
      <span class="brief-ai-mark" title="AI 智能分析简介">{@html icons.sparkle}</span>
    {/if}
    <span class="row-brief-text" class:is-ai={brief.isAi}>
      {brief.text || '暂无简介'}
    </span>
  </div>

  <div class="row-meta-col">
    <div class="provider-strip">
      {#each ALL_PROVIDERS as p (p.id)}
        {@const dep = deploymentMap.get(p.id)}
        {@const isOn = Boolean(dep?.enabled)}
        <span
          class="provider-chip"
          class:inactive={!isOn}
          style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}
          title={`${p.label}：${isOn ? '已启用' : '已禁用'}`}
        >
          {@html p.icon}
        </span>
      {/each}
    </div>

    <span class="row-source">{sourceTypeLabel(item.source.source_type)}</span>
    <span class="row-group" title={groupTag}>{groupTag}</span>

    {#if analyzable && !multiSelect}
      <button
        type="button"
        class="row-ai-btn"
        class:failed={isFailed}
        disabled={isRunning || isQueued}
        title={isFailed ? `重试分析：${batchState?.error ?? ''}` : item.summary ? '重新 AI 分析' : 'AI 分析'}
        onclick={(e) => {
          e.stopPropagation();
          onAnalyzeSingle(item);
        }}
      >
        <span class="ai-icon" class:spin={isRunning}>{@html icons.sparkle}</span>
      </button>
    {/if}

    {#if !multiSelect && !item.builtin}
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
    {:else if !multiSelect}
      <span class="builtin-tag">内置</span>
    {/if}
  </div>
</div>

<style>
  .skill-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 9px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    cursor: pointer;
    overflow: hidden;
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }

  .skill-row:hover {
    border-color: var(--brand-border);
    background: var(--bg-card-hover);
  }

  .skill-row.is-selected {
    border-color: var(--brand-primary);
    background: color-mix(in srgb, var(--brand-subtle) 55%, var(--bg-card));
  }

  .skill-row.is-disabled {
    opacity: 0.78;
  }

  .skill-row.is-failed {
    border-color: var(--color-danger-border);
  }

  .skill-row.is-analyzing::after {
    content: '';
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(90deg, transparent 20%, rgba(79, 91, 213, 0.1) 50%, transparent 80%);
    animation: row-sweep 1.4s linear infinite;
  }

  @keyframes row-sweep {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(100%); }
  }

  .row-name-col {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 196px;
    flex-shrink: 0;
    min-width: 0;
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

  .select-box {
    width: 15px;
    height: 15px;
    accent-color: var(--brand-primary);
    cursor: pointer;
    flex-shrink: 0;
  }

  .row-name {
    font-size: 12.5px;
    font-weight: 650;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-brief-col {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .row-state {
    padding: 1px 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .row-state.running {
    background: var(--brand-subtle);
    color: var(--brand-strong);
  }

  .row-state.queued {
    background: var(--bg-secondary);
    color: var(--text-muted);
  }

  .row-state.failed {
    background: var(--color-danger-bg);
    color: var(--color-danger);
  }

  .brief-ai-mark {
    display: inline-flex;
    width: 11px;
    height: 11px;
    color: var(--brand-primary);
    flex-shrink: 0;
  }

  .brief-ai-mark :global(svg),
  .provider-chip :global(svg),
  .ai-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .row-brief-text {
    font-size: 12px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-brief-text.is-ai {
    color: var(--text-primary);
  }

  .row-meta-col {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  .provider-strip {
    display: inline-flex;
    align-items: center;
    gap: 3px;
  }

  .provider-chip {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    padding: 3px;
    border-radius: 5px;
    color: var(--p-color);
    background: var(--p-bg);
  }

  .provider-chip.inactive {
    opacity: 0.35;
    filter: grayscale(1);
  }

  .row-source {
    font-size: 11px;
    color: var(--text-muted);
    min-width: 48px;
    text-align: right;
  }

  .row-group {
    font-size: 11px;
    color: var(--color-estimated);
    font-weight: 500;
    max-width: 110px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .row-ai-btn {
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
  }

  .row-ai-btn:hover:not(:disabled) {
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .row-ai-btn.failed {
    color: var(--color-danger);
    background: var(--color-danger-bg);
  }

  .ai-icon {
    display: inline-flex;
    width: 13px;
    height: 13px;
  }

  .ai-icon.spin {
    animation: spin 1.1s linear infinite;
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

  .builtin-tag {
    padding: 1px 6px;
    border-radius: 999px;
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
  }

  @media (max-width: 900px) {
    .row-group,
    .row-source {
      display: none;
    }
    .row-name-col {
      width: 150px;
    }
  }
</style>
