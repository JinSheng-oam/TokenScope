<script lang="ts">
  import type { Entry } from './agentCustomizationEntries';

  export let selected: Entry;
  export let mainEntryForSelected: Entry;
  export let availableMainEntries: Entry[] = [];
  export let targetMainEntryName = '';
  export let isReferencedInMain = false;
  export let checkingReference = false;
  export let togglingReference = false;
  export let busy = false;
  export let onToggleReference: () => void = () => {};
</script>

<div class="aux-reference-bar" class:is-active={isReferencedInMain}>
  <div class="aux-ref-main">
    <span class="aux-ref-icon" aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <path d="M10 13a5 5 0 0 0 7.54.54l3-3a5 5 0 0 0-7.07-7.07l-1.72 1.71" />
        <path d="M14 11a5 5 0 0 0-7.54-.54l-3 3a5 5 0 0 0 7.07 7.07l1.71-1.71" />
      </svg>
    </span>
    <span class="aux-ref-title">引用进主入口</span>
    {#if availableMainEntries.length > 1}
      <div class="aux-ref-select-wrap">
        <select
          class="aux-ref-select"
          bind:value={targetMainEntryName}
          disabled={busy || togglingReference}
          aria-label="选择目标主 md 文件"
        >
          {#each availableMainEntries as item}
            <option value={item.id.name}>{item.id.name}</option>
          {/each}
        </select>
      </div>
    {:else}
      <span class="aux-ref-target-pill">{mainEntryForSelected.id.name}</span>
    {/if}
    <span class="aux-ref-sep">·</span>
    <span class="aux-ref-hint" title={`@${selected.path}`}>
      {isReferencedInMain ? '已挂载' : '开启后在末尾追加'}
      <code>@{selected.path}</code>
    </span>
  </div>
  <div class="aux-ref-switch-wrap">
    <span class="aux-ref-status-text" class:active={isReferencedInMain}>
      {checkingReference ? '检查中…' : isReferencedInMain ? '已引用' : '未引用'}
    </span>
    <button
      type="button"
      class="ref-switch"
      role="switch"
      aria-label={`在 ${mainEntryForSelected.id.name} 中以 @ 语法引用此辅助文件`}
      aria-checked={isReferencedInMain}
      disabled={busy || checkingReference || togglingReference}
      onclick={onToggleReference}
    >
      <span></span>
    </button>
  </div>
</div>

<style>
  .aux-reference-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 18px;
    background: color-mix(in srgb, var(--bg-secondary) 75%, var(--bg-card));
    border-bottom: 1px solid var(--border-subtle);
  }

  .aux-reference-bar.is-active {
    background: color-mix(in srgb, var(--brand-subtle) 55%, var(--bg-card));
    border-bottom-color: var(--brand-border);
  }

  .aux-ref-main {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
  }

  .aux-ref-icon {
    width: 13px;
    height: 13px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--brand-primary);
    flex-shrink: 0;
  }

  .aux-ref-icon svg {
    width: 100%;
    height: 100%;
  }

  .aux-ref-title {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-primary);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .aux-ref-target-pill {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-card);
    border: 1px solid var(--brand-border);
    font-size: 10.5px;
    font-family: var(--font-mono, monospace);
    font-weight: 600;
    color: var(--brand-primary);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .aux-ref-select-wrap select {
    height: 22px;
    padding: 0 6px;
    border-radius: 4px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    font-size: 11px;
    font-family: var(--font-mono, monospace);
    color: var(--text-primary);
    cursor: pointer;
  }

  .aux-ref-sep {
    color: var(--text-muted);
    font-size: 11px;
    flex-shrink: 0;
  }

  .aux-ref-hint {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .aux-ref-hint code {
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    font-family: var(--font-mono, monospace);
    font-size: 10.5px;
    color: var(--text-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 320px;
  }

  .aux-ref-switch-wrap {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-shrink: 0;
  }

  .aux-ref-status-text {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
  }

  .aux-ref-status-text.active {
    color: var(--color-exact);
    font-weight: 600;
  }

  .ref-switch {
    display: flex;
    align-items: center;
    width: 38px;
    height: 22px;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    background: var(--bg-secondary);
    cursor: pointer;
    box-sizing: border-box;
    transition: background-color 150ms ease, border-color 150ms ease;
  }

  .ref-switch span {
    display: block;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: var(--text-muted);
    transition: transform 150ms ease, background-color 150ms ease;
  }

  .ref-switch[aria-checked='true'] {
    border-color: var(--brand-strong);
    background: var(--brand-strong);
  }

  .ref-switch[aria-checked='true'] span {
    background: #fff;
    transform: translateX(16px);
  }

  .ref-switch:focus-visible {
    outline: 2px solid var(--brand-primary);
    outline-offset: 2px;
  }

  .ref-switch:disabled {
    cursor: not-allowed;
    opacity: 0.5;
  }
</style>
