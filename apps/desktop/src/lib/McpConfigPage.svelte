<script lang="ts">
  import AgentMcpConfig from './AgentMcpConfig.svelte';
  import { confirmDialog } from './dialogStore';
  import { icons } from './icons';
  import type { Provider } from './agentCustomizationEntries';

  let {
    runtimeAvailable,
    dirty = $bindable(false),
  }: {
    runtimeAvailable: boolean;
    dirty?: boolean;
  } = $props();

  const providers: { id: Provider; label: string }[] = [
    { id: 'codex', label: 'Codex' },
    { id: 'claude', label: 'Claude Code' },
    { id: 'antigravity', label: 'Antigravity' },
    { id: 'pi', label: 'Pi Agent' },
    { id: 'deepseek', label: 'DeepSeek Harness' },
  ];

  let provider = $state<Provider>('codex');
  let activeProviderLabel = $derived(providers.find((p) => p.id === provider)?.label ?? 'MCP');

  async function switchProvider(next: Provider) {
    if (provider === next) return;
    if (
      dirty &&
      !(await confirmDialog({
        title: '未保存的 MCP 修改',
        message: '当前工具的 MCP 配置有未保存修改，确定放弃并切换工具？',
        confirmText: '放弃修改',
      }))
    ) {
      return;
    }
    dirty = false;
    provider = next;
  }
</script>

<div class="mcp-page">
  <div class="page-header standalone-heading">
    <h1>MCP 配置</h1>
    <span class="local-badge" title="MCP 配置可能包含 API 密钥，仅读写本机文件，不进入数据库或云同步">
      {@html icons.shield}
      <span>仅操作本机文件</span>
    </span>
  </div>

  <div class="mcp-provider-toolbar">
    <div class="provider-tabs" role="group" aria-label="选择工具">
      {#each providers as item (item.id)}
        <button
          type="button"
          class="provider-tab-btn"
          class:selected={provider === item.id}
          aria-pressed={provider === item.id}
          onclick={() => void switchProvider(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </div>
  </div>

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理本机 MCP 配置。</div>
  {:else}
    {#key provider}
      <AgentMcpConfig
        {provider}
        title={`${activeProviderLabel} MCP 服务器`}
        bind:dirty
      />
    {/key}
  {/if}
</div>

<style>
  .mcp-page {
    display: flex;
    flex-direction: column;
    gap: var(--page-heading-gap, 14px);
    min-width: 0;
  }

  .standalone-heading {
    margin-bottom: 0;
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

  .mcp-provider-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 4px 16px;
    padding: 6px 8px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
  }

  .provider-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
  }

  .provider-tab-btn {
    min-height: 32px;
    padding: 3px 10px;
    border: 1px solid transparent;
    background: transparent;
    border-radius: var(--radius-xs);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background-color var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast);
    white-space: nowrap;
  }

  .provider-tab-btn:hover {
    color: var(--text-primary);
  }

  .provider-tab-btn:focus-visible {
    outline: 2px solid var(--brand-primary);
    outline-offset: 1px;
  }

  .provider-tab-btn.selected {
    background: var(--brand-subtle);
    color: var(--brand-text);
    border-color: var(--brand-border);
  }
</style>
