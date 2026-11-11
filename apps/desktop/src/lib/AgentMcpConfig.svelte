<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import type { Provider } from './agentCustomizationEntries';

  type View = { path: string; format: 'TOML' | 'JSON'; draft: string; revision: string };
  export let provider: Provider;
  export let dirty = false;
  export let onClose: () => void;

  let profile: 'desktop' | 'web' = 'desktop';
  let view: View | null = null;
  let draft = '';
  let busy = false;
  let error = '';
  let notice = '';
  let reveal = false;

  $: dirty = view !== null && draft !== view.draft;

  async function run(save = false) {
    if (busy || (save && !view)) return;
    if (!save && dirty && !window.confirm('放弃未保存的 MCP 修改？')) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const next = save
        ? await invoke<View>('save_mcp_config', { provider, profile, expected: view!.revision, draft })
        : await invoke<View>('read_mcp_config', { provider, profile });
      view = next;
      draft = next.draft;
      if (save) notice = '已保存到本机。请在原 Agent 中重新加载配置。';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function switchProfile(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const next = select.value === 'web' ? 'web' : 'desktop';
    if (dirty && !window.confirm('放弃未保存的 MCP 修改？')) {
      select.value = profile;
      return;
    }
    profile = next;
    view = null;
    draft = '';
    reveal = false;
    void run();
  }

  onMount(() => { void run(); });
</script>

<section class="mcp-panel" aria-label="MCP 配置">
  <div class="mcp-heading">
    <div>
      <strong>MCP 配置</strong>
      {#if view}<p>{view.path}</p>{/if}
    </div>
    <button type="button" class="btn-secondary" disabled={busy} onclick={onClose}>收起</button>
  </div>

  {#if provider === 'deepseek'}
    <div class="mcp-actions">
      <label for="dsh-mcp-profile">配置档案</label>
      <select id="dsh-mcp-profile" value={profile} disabled={busy} onchange={switchProfile}>
        <option value="desktop">Desktop</option>
        <option value="web">Web</option>
      </select>
    </div>
    <p>这里只管理通过 TokenScope 添加的服务器。手写、继承和插件注册的配置请在 DeepSeek 中查看。</p>
  {/if}
    {#if error}<p class="mcp-error" role="alert">{error}</p>{/if}
    {#if notice}<p role="status">{notice}</p>{/if}
    {#if view}
      <p>仅编辑 MCP 段。配置可能包含密钥，只保存在本机。</p>
      {#if reveal}
        {#if provider === 'deepseek'}
          <p>本地进程用 transport: stdio 和 command；远端用 transport: streamable-http 和 url。enabled: false 可停用。</p>
        {/if}
        <label for="mcp-config-draft">服务器配置（{view.format}）</label>
        <textarea id="mcp-config-draft" bind:value={draft} disabled={busy} spellcheck="false" autocomplete="off" autocapitalize="off"></textarea>
      {/if}
    {/if}
    <div class="mcp-actions">
      <button type="button" class="btn-secondary" disabled={busy} onclick={() => void run()}>{busy ? '处理中…' : '重新读取'}</button>
      {#if view}
        <button type="button" class="btn-secondary" disabled={busy} onclick={() => reveal = !reveal}>{reveal ? '隐藏配置' : '显示并编辑'}</button>
        <button type="button" class="btn-primary" disabled={busy || !dirty || !reveal} onclick={() => void run(true)}>保存 MCP</button>
      {/if}
    </div>
</section>

<style>
  .mcp-panel { display: grid; gap: 10px; padding: 14px; border: 1px solid var(--border-subtle); border-radius: 10px; background: var(--bg-card); }
  .mcp-heading, .mcp-actions { display: flex; align-items: center; gap: 8px; flex-wrap: wrap; }
  .mcp-heading { justify-content: space-between; }
  .mcp-heading > div { min-width: 0; }
  p { margin: 0; overflow-wrap: anywhere; color: var(--text-secondary); font-size: 12px; }
  .mcp-error { color: var(--color-danger, #b42318); }
  label { font-size: 12px; }
  textarea { width: 100%; min-height: 220px; resize: vertical; padding: 10px; border: 1px solid var(--border-subtle); border-radius: 6px; background: var(--bg-secondary); color: var(--text-primary); font: 12px/1.5 var(--font-mono, monospace); }
</style>
