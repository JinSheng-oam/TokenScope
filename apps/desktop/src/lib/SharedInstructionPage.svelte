<script lang="ts">
  import SharedInstruction from './SharedInstruction.svelte';
  import AgentInstructionsWorkspace from './AgentInstructionsWorkspace.svelte';
  import { confirmDialog } from './dialogStore';
  import { icons } from './icons';

  let {
    runtimeAvailable,
    dirty = $bindable(false),
  }: {
    runtimeAvailable: boolean;
    dirty?: boolean;
  } = $props();

  type InstructionSubTab = 'shared' | 'local';
  let subTab = $state<InstructionSubTab>('shared');
  let sharedDirty = $state(false);
  let localDirty = $state(false);

  $effect(() => {
    dirty = subTab === 'shared' ? sharedDirty : localDirty;
  });

  async function switchSubTab(next: InstructionSubTab) {
    if (subTab === next) return;
    if (dirty) {
      const ok = await confirmDialog({
        title: '未保存的指令修改',
        message: '当前标签页有未保存的指令内容，切换后将放弃这些更改，确定继续？',
        confirmText: '放弃并切换',
      });
      if (!ok) return;
      sharedDirty = false;
      localDirty = false;
    }
    subTab = next;
  }
</script>

<div class="instruction-files-page">
  <div class="page-header standalone-heading">
    <div class="heading-left">
      <h1>指令文件</h1>
      <div class="subtab-capsule" role="tablist" aria-label="指令模式">
        <button
          type="button"
          role="tab"
          aria-selected={subTab === 'shared'}
          class="subtab-btn"
          class:active={subTab === 'shared'}
          onclick={() => void switchSubTab('shared')}
        >
          统一主稿分发
        </button>
        <button
          type="button"
          role="tab"
          aria-selected={subTab === 'local'}
          class="subtab-btn"
          class:active={subTab === 'local'}
          onclick={() => void switchSubTab('local')}
        >
          各工具本地指令
        </button>
      </div>
    </div>

    <span
      class="local-badge"
      title={subTab === 'shared'
        ? '统一主稿支持通过 WebDAV 同步，应用目标仅写入本机文件'
        : '各工具本地指令仅读写本机文件'}
    >
      {@html icons.shield}
      <span>{subTab === 'shared' ? '跨工具主稿分发' : '仅操作本机文件'}</span>
    </span>
  </div>

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理指令文件。</div>
  {:else if subTab === 'shared'}
    <SharedInstruction standalone={true} bind:unsaved={sharedDirty} />
  {:else}
    <AgentInstructionsWorkspace {runtimeAvailable} bind:dirty={localDirty} />
  {/if}
</div>

<style>
  .instruction-files-page {
    display: flex;
    flex-direction: column;
    gap: var(--page-heading-gap, 14px);
    min-width: 0;
    height: 100%;
    min-height: 0;
  }

  .standalone-heading {
    margin-bottom: 0;
  }

  .heading-left {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .subtab-capsule {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .subtab-btn {
    height: 26px;
    padding: 0 11px;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .subtab-btn:hover {
    color: var(--text-primary);
  }

  .subtab-btn.active {
    background: var(--bg-card);
    border-color: var(--border-subtle);
    color: var(--brand-text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgba(15, 23, 42, 0.05);
  }

  .local-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .local-badge :global(svg) {
    width: 12px;
    height: 12px;
    color: var(--color-exact);
  }
</style>
