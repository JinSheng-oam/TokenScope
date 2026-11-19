<script lang="ts">
  import SharedInstruction from './SharedInstruction.svelte';
  import { icons } from './icons';

  let {
    runtimeAvailable,
    dirty = $bindable(false),
  }: {
    runtimeAvailable: boolean;
    dirty?: boolean;
  } = $props();
</script>

<div class="shared-instruction-page">
  <div class="page-header standalone-heading">
    <h1>统一指令</h1>
    <span class="local-badge" title="支持通过 WebDAV 仅同步指令正文，应用目标仍仅写入本机文件">
      {@html icons.shield}
      <span>跨工具主稿分发</span>
    </span>
  </div>

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理统一指令。</div>
  {:else}
    <SharedInstruction standalone={true} bind:unsaved={dirty} />
  {/if}
</div>

<style>
  .shared-instruction-page {
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
</style>
