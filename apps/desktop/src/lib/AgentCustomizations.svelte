<script lang="ts">
  import './agentCustomizationBadges.css';
  import { icons } from './icons';
  import AgentAiConfig from './AgentAiConfig.svelte';
  import SkillLibraryView from './SkillLibraryView.svelte';

  export let runtimeAvailable: boolean;
  export let dirty = false;

  let showAiConfig = false;
  let hasAiConfig = false;
  let error = '';
  let notice = '';
  let totalCount = 0;
</script>

<div class="custom-page">
  <div class="page-header custom-heading">
    <div class="heading-left">
      <h1>技能库</h1>
      {#if totalCount > 0}
        <span class="heading-count-pill">{totalCount}</span>
      {/if}
    </div>

    <div class="heading-right">
      <button
        type="button"
        class="ai-toggle-btn"
        class:active={showAiConfig}
        aria-expanded={showAiConfig}
        onclick={() => (showAiConfig = !showAiConfig)}
        title="配置用于 Skill 智能分析的 AI 模型 API"
      >
        <span class="ai-status-dot" class:configured={hasAiConfig}></span>
        <span class="ai-btn-icon">{@html icons.sparkle}</span>
        <span>AI 助手配置</span>
      </button>

      <span class="local-badge" title="Skill 仅操作本机文件，AI 摘要仅缓存在本地 Skill 目录">
        {@html icons.shield}
        <span>仅操作本机文件</span>
      </span>
    </div>
  </div>

  <AgentAiConfig {runtimeAvailable} bind:showAiConfig bind:hasAiConfig bind:error bind:notice />

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理本地技能库。</div>
  {:else}
    {#if error}<p class="message error" role="alert">{error}</p>{/if}
    {#if notice}<p class="message success" role="status">{notice}</p>{/if}
    <SkillLibraryView
      {runtimeAvailable}
      {hasAiConfig}
      bind:dirty
      bind:totalCount
      onOpenAiConfig={() => { showAiConfig = true; }}
    />
  {/if}
</div>

<style>
  .custom-page {
    max-width: 1440px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--page-heading-gap, 14px);
  }

  .custom-heading {
    margin-bottom: 0;
  }

  .heading-left,
  .heading-right {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .heading-count-pill {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 24px;
    height: 22px;
    padding: 0 7px;
    border-radius: 999px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-card);
    color: var(--text-secondary);
    font-size: 11.5px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .ai-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    padding: 0 10px;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .ai-toggle-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .ai-toggle-btn.active {
    color: var(--brand-text);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .ai-status-dot {
    width: 6px;
    height: 6px;
    flex: 0 0 6px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .ai-status-dot.configured {
    background: var(--color-exact);
  }

  .ai-btn-icon {
    display: inline-flex;
    width: 13px;
    height: 13px;
  }

  .ai-btn-icon :global(svg) {
    width: 100%;
    height: 100%;
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

  .message {
    font-size: 11.5px;
    padding: 8px 14px;
    border-radius: var(--radius-sm);
    margin: 0;
    border: 1px solid var(--border-subtle);
  }

  .message.error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }

  .message.success {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
  }
</style>
