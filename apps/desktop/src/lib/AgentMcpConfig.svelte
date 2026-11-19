<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { confirmDialog } from './dialogStore';
  import type { Provider } from './agentCustomizationEntries';
  import { icons } from './icons';
  import {
    createEmptyFormDraft,
    entryToFormDraft,
    formDraftToEntry,
    formatServerSummary,
    parseMcpJsonSnippet,
    transportBadgeLabel,
    validateFormDraft,
    type McpFormDraft,
    type McpServerEntry,
    type McpView,
  } from './mcpForm';

  export let provider: Provider;
  export let dirty = false;
  export let onClose: () => void;

  let profile: 'desktop' | 'web' = 'desktop';
  let view: McpView | null = null;
  let draft = '';
  let mode: 'visual' | 'raw' = 'visual';
  let busy = false;
  let error = '';
  let notice = '';
  let reveal = false;

  // Graphical add/edit form state
  let editing = false;
  let editingOriginalName: string | null = null;
  let form: McpFormDraft = createEmptyFormDraft('stdio');
  let initialFormSnapshot = '';
  let showJsonImport = false;
  let jsonSnippet = '';

  $: servers = view?.servers ?? [];
  $: rawDirty = mode === 'raw' && view !== null && draft !== view.draft;
  $: formDirty = editing && JSON.stringify(form) !== initialFormSnapshot;
  $: dirty = rawDirty || formDirty;

  async function confirmDiscard(): Promise<boolean> {
    if (!dirty) return true;
    return confirmDialog({
      title: '未保存的 MCP 修改',
      message: '当前有未保存的 MCP 配置修改，确定放弃？',
      confirmText: '放弃修改',
    });
  }

  async function run(save = false) {
    if (busy || (save && !view)) return;
    if (!save && !(await confirmDiscard())) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const next = save
        ? await invoke<McpView>('save_mcp_config', {
            provider,
            profile,
            expected: view!.revision,
            draft,
          })
        : await invoke<McpView>('read_mcp_config', { provider, profile });
      view = next;
      draft = next.draft;
      if (!save) {
        editing = false;
        editingOriginalName = null;
      } else {
        notice = '已保存到本机，请在原 Agent 中重新加载配置。';
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function switchProfile(event: Event) {
    const select = event.currentTarget as HTMLSelectElement;
    const next = select.value === 'web' ? 'web' : 'desktop';
    if (!(await confirmDiscard())) {
      select.value = profile;
      return;
    }
    profile = next;
    view = null;
    draft = '';
    reveal = false;
    editing = false;
    editingOriginalName = null;
    void run();
  }

  async function switchMode(next: 'visual' | 'raw') {
    if (mode === next) return;
    if (!(await confirmDiscard())) return;
    error = '';
    notice = '';
    if (next === 'raw') {
      editing = false;
      editingOriginalName = null;
      draft = view?.draft ?? '';
    }
    mode = next;
  }

  async function openAddForm() {
    if (mode === 'raw' && rawDirty && !(await confirmDiscard())) return;
    mode = 'visual';
    error = '';
    notice = '';
    editingOriginalName = null;
    form = createEmptyFormDraft('stdio');
    initialFormSnapshot = JSON.stringify(form);
    showJsonImport = false;
    jsonSnippet = '';
    editing = true;
  }

  async function openEditForm(entry: McpServerEntry) {
    if (editing && formDirty && !(await confirmDiscard())) return;
    mode = 'visual';
    error = '';
    notice = '';
    editingOriginalName = entry.name;
    form = entryToFormDraft(entry);
    initialFormSnapshot = JSON.stringify(form);
    showJsonImport = false;
    jsonSnippet = '';
    editing = true;
  }

  async function cancelForm() {
    if (formDirty && !(await confirmDiscard())) return;
    editing = false;
    editingOriginalName = null;
    showJsonImport = false;
    jsonSnippet = '';
    error = '';
  }

  function applyCommandPreset(cmd: string, defaultArgs: string[]) {
    form = {
      ...form,
      command: cmd,
      args: form.args.length > 0 ? form.args : [...defaultArgs],
    };
  }

  function addArg() {
    form = { ...form, args: [...form.args, ''] };
  }

  function updateArg(index: number, value: string) {
    const next = [...form.args];
    next[index] = value;
    form = { ...form, args: next };
  }

  function removeArg(index: number) {
    form = { ...form, args: form.args.filter((_, i) => i !== index) };
  }

  function addKv(field: 'env' | 'headers') {
    form = { ...form, [field]: [...form[field], { key: '', value: '', reveal: false }] };
  }

  function updateKv(field: 'env' | 'headers', index: number, prop: 'key' | 'value', value: string) {
    const next = form[field].map((row, i) => (i === index ? { ...row, [prop]: value } : row));
    form = { ...form, [field]: next };
  }

  function toggleKvReveal(field: 'env' | 'headers', index: number) {
    const next = form[field].map((row, i) => (i === index ? { ...row, reveal: !row.reveal } : row));
    form = { ...form, [field]: next };
  }

  function removeKv(field: 'env' | 'headers', index: number) {
    form = { ...form, [field]: form[field].filter((_, i) => i !== index) };
  }

  function handleImportJson() {
    error = '';
    notice = '';
    const parsed = parseMcpJsonSnippet(jsonSnippet);
    if ('error' in parsed) {
      error = parsed.error;
      return;
    }
    const imported = parsed.draft;
    form = {
      ...imported,
      name: imported.name || form.name,
      transport: provider === 'deepseek' && imported.transport === 'sse' ? 'http' : imported.transport,
    };
    showJsonImport = false;
    jsonSnippet = '';
    notice =
      parsed.extraCount > 0
        ? `已解析首个服务器配置到表单（片段中其余 ${parsed.extraCount} 个请逐个导入或切换到源码编辑）。`
        : '已解析到表单，请核对后点击保存。';
  }

  async function saveForm() {
    if (busy || !view) return;
    error = '';
    notice = '';
    const existingNames = servers.map((s) => s.name);
    const validationError = validateFormDraft(form, provider, existingNames, editingOriginalName);
    if (validationError) {
      error = validationError;
      return;
    }
    busy = true;
    try {
      const entry = formDraftToEntry(form, provider);
      const next = await invoke<McpView>('upsert_mcp_server', {
        provider,
        profile,
        expected: view.revision,
        originalName: editingOriginalName,
        entry,
      });
      view = next;
      draft = next.draft;
      editing = false;
      editingOriginalName = null;
      notice = `已保存「${entry.name}」到本机，请在原 Agent 中重新加载配置。`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleServerEnabled(entry: McpServerEntry) {
    if (busy || !view) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const updated: McpServerEntry = { ...entry, enabled: !entry.enabled };
      const next = await invoke<McpView>('upsert_mcp_server', {
        provider,
        profile,
        expected: view.revision,
        originalName: entry.name,
        entry: updated,
      });
      view = next;
      draft = next.draft;
      notice = `已${updated.enabled ? '启用' : '停用'}「${entry.name}」。`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function removeServer(entry: McpServerEntry) {
    if (busy || !view) return;
    if (
      !(await confirmDialog({
        title: '删除 MCP 服务器',
        message: `确定删除 MCP 服务器「${entry.name}」？此操作会从本机配置移除该条目。`,
        tone: 'danger',
        confirmText: '删除服务器',
      }))
    ) {
      return;
    }
    busy = true;
    error = '';
    notice = '';
    try {
      const next = await invoke<McpView>('delete_mcp_server', {
        provider,
        profile,
        expected: view.revision,
        name: entry.name,
      });
      view = next;
      draft = next.draft;
      if (editingOriginalName === entry.name) {
        editing = false;
        editingOriginalName = null;
      }
      notice = `已删除「${entry.name}」。`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void run();
  });
</script>

<section class="mcp-panel" aria-label="MCP 配置">
  <div class="mcp-heading">
    <div class="mcp-title-block">
      <div class="mcp-title-row">
        <strong>MCP 配置</strong>
        {#if view}
          <span class="mcp-count-pill">{servers.length} 个服务</span>
        {/if}
      </div>
      {#if view}<p class="mcp-path mono">{view.path}</p>{/if}
    </div>

    <div class="mcp-toolbar">
      {#if provider === 'deepseek'}
        <select
          id="dsh-mcp-profile"
          class="mcp-select"
          aria-label="DeepSeek 配置档案"
          value={profile}
          disabled={busy}
          onchange={switchProfile}
        >
          <option value="desktop">Desktop</option>
          <option value="web">Web</option>
        </select>
      {/if}

      {#if view}
        <div class="mcp-mode-switch" role="tablist" aria-label="MCP 编辑模式">
          <button
            type="button"
            role="tab"
            class="mcp-mode-btn"
            class:active={mode === 'visual'}
            aria-selected={mode === 'visual'}
            disabled={busy}
            onclick={() => switchMode('visual')}
          >
            图形化
          </button>
          <button
            type="button"
            role="tab"
            class="mcp-mode-btn"
            class:active={mode === 'raw'}
            aria-selected={mode === 'raw'}
            disabled={busy}
            onclick={() => switchMode('raw')}
          >
            源码 ({view.format})
          </button>
        </div>

        <button type="button" class="btn-primary mcp-add-btn" disabled={busy} onclick={openAddForm}>
          <span class="btn-icon">{@html icons.plus}</span>
          <span>添加 MCP</span>
        </button>
      {/if}

      <button
        type="button"
        class="btn-icon-only"
        disabled={busy}
        onclick={() => void run()}
        title="重新读取本机配置"
        aria-label="重新读取"
      >
        {@html icons.refresh}
      </button>

      <button type="button" class="btn-close-panel" disabled={busy} onclick={onClose} title="收起 MCP 配置面板">
        ✕ 收起
      </button>
    </div>
  </div>

  {#if error}<p class="mcp-error" role="alert">{error}</p>{/if}
  {#if notice}<p class="mcp-notice" role="status">{notice}</p>{/if}

  {#if view && mode === 'visual'}
    {#if editing}
      <form
        class="mcp-form"
        onsubmit={(event) => {
          event.preventDefault();
          void saveForm();
        }}
      >
        <div class="mcp-form-header">
          <strong>{editingOriginalName ? `编辑服务器：${editingOriginalName}` : '添加 MCP 服务器'}</strong>
          <button
            type="button"
            class="btn-link"
            disabled={busy}
            onclick={() => {
              showJsonImport = !showJsonImport;
              error = '';
            }}
          >
            {showJsonImport ? '收起 JSON 导入' : '从 JSON 片段填充'}
          </button>
        </div>

        {#if showJsonImport}
          <div class="mcp-json-import">
            <textarea
              class="mcp-snippet-input"
              bind:value={jsonSnippet}
              disabled={busy}
              spellcheck="false"
              placeholder={'粘贴 mcpServers 或单个服务 JSON，例如：{"mcpServers":{"filesystem":{"command":"npx","args":["-y","@modelcontextprotocol/server-filesystem","/path"]}}}'}
            ></textarea>
            <div class="mcp-import-actions">
              <button type="button" class="btn-secondary" disabled={busy || !jsonSnippet.trim()} onclick={handleImportJson}>
                解析并填充表单
              </button>
            </div>
          </div>
        {/if}

        <div class="mcp-form-grid">
          <div class="mcp-field">
            <label for="mcp-name">服务器名称</label>
            <input
              id="mcp-name"
              type="text"
              bind:value={form.name}
              disabled={busy}
              placeholder={provider === 'deepseek' ? '仅限字母、数字、_ 或 -' : '例如 filesystem、github'}
              autocomplete="off"
              spellcheck="false"
            />
          </div>

          <div class="mcp-field">
            <span class="mcp-field-label">传输方式</span>
            <div class="mcp-transport-group" role="group" aria-label="传输方式">
              <button
                type="button"
                class="mcp-transport-btn"
                class:active={form.transport === 'stdio'}
                disabled={busy}
                onclick={() => (form = { ...form, transport: 'stdio' })}
              >
                本地进程 (STDIO)
              </button>
              <button
                type="button"
                class="mcp-transport-btn"
                class:active={form.transport === 'http'}
                disabled={busy}
                onclick={() => (form = { ...form, transport: 'http' })}
              >
                {provider === 'deepseek' ? '远端 HTTP' : '远端 HTTP'}
              </button>
              {#if provider !== 'deepseek'}
                <button
                  type="button"
                  class="mcp-transport-btn"
                  class:active={form.transport === 'sse'}
                  disabled={busy}
                  onclick={() => (form = { ...form, transport: 'sse' })}
                >
                  SSE
                </button>
              {/if}
            </div>
          </div>
        </div>

        {#if form.transport === 'stdio'}
          <div class="mcp-form-grid">
            <div class="mcp-field">
              <div class="mcp-field-head">
                <label for="mcp-command">启动命令</label>
                {#if !editingOriginalName && !form.command.trim()}
                  <div class="mcp-presets">
                    <button type="button" class="mcp-preset-chip" onclick={() => applyCommandPreset('npx', ['-y'])}>npx</button>
                    <button type="button" class="mcp-preset-chip" onclick={() => applyCommandPreset('uvx', [])}>uvx</button>
                    <button type="button" class="mcp-preset-chip" onclick={() => applyCommandPreset('node', [])}>node</button>
                    <button type="button" class="mcp-preset-chip" onclick={() => applyCommandPreset('python', ['-m'])}>python</button>
                  </div>
                {/if}
              </div>
              <input
                id="mcp-command"
                class="mono"
                type="text"
                bind:value={form.command}
                disabled={busy}
                placeholder="例如 npx、uvx、node 或可执行文件路径"
                autocomplete="off"
                spellcheck="false"
              />
            </div>

            <div class="mcp-field">
              <label for="mcp-cwd">工作目录（可选）</label>
              <input
                id="mcp-cwd"
                class="mono"
                type="text"
                bind:value={form.cwd}
                disabled={busy}
                placeholder="留空则使用默认目录"
                autocomplete="off"
                spellcheck="false"
              />
            </div>
          </div>

          <div class="mcp-field">
            <div class="mcp-field-head">
              <span class="mcp-field-label">命令参数（Args）</span>
              <button type="button" class="btn-link" disabled={busy} onclick={addArg}>+ 添加参数</button>
            </div>
            {#if form.args.length > 0}
              <div class="mcp-list-rows">
                {#each form.args as arg, index}
                  <div class="mcp-arg-row">
                    <input
                      class="mono"
                      type="text"
                      value={arg}
                      disabled={busy}
                      placeholder={`参数 ${index + 1}`}
                      autocomplete="off"
                      spellcheck="false"
                      oninput={(event) => updateArg(index, (event.currentTarget as HTMLInputElement).value)}
                    />
                    <button
                      type="button"
                      class="btn-row-remove"
                      disabled={busy}
                      onclick={() => removeArg(index)}
                      title="移除参数"
                      aria-label="移除参数"
                    >
                      {@html icons.close}
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>

          <div class="mcp-field">
            <div class="mcp-field-head">
              <span class="mcp-field-label">环境变量（Env，仅保存在本机）</span>
              <button type="button" class="btn-link" disabled={busy} onclick={() => addKv('env')}>+ 添加变量</button>
            </div>
            {#if form.env.length > 0}
              <div class="mcp-list-rows">
                {#each form.env as row, index}
                  <div class="mcp-kv-row">
                    <input
                      class="mono"
                      type="text"
                      value={row.key}
                      disabled={busy}
                      placeholder="KEY"
                      autocomplete="off"
                      spellcheck="false"
                      oninput={(event) => updateKv('env', index, 'key', (event.currentTarget as HTMLInputElement).value)}
                    />
                    <div class="mcp-secret-wrap">
                      <input
                        class="mono"
                        type={row.reveal ? 'text' : 'password'}
                        value={row.value}
                        disabled={busy}
                        placeholder="VALUE"
                        autocomplete="off"
                        spellcheck="false"
                        oninput={(event) => updateKv('env', index, 'value', (event.currentTarget as HTMLInputElement).value)}
                      />
                      <button
                        type="button"
                        class="btn-eye"
                        disabled={busy}
                        onclick={() => toggleKvReveal('env', index)}
                        title={row.reveal ? '隐藏值' : '显示值'}
                        aria-label={row.reveal ? '隐藏值' : '显示值'}
                      >
                        {@html row.reveal ? icons.eyeOff : icons.eye}
                      </button>
                    </div>
                    <button
                      type="button"
                      class="btn-row-remove"
                      disabled={busy}
                      onclick={() => removeKv('env', index)}
                      title="移除变量"
                      aria-label="移除变量"
                    >
                      {@html icons.close}
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="mcp-field">
            <label for="mcp-url">服务地址（URL）</label>
            <input
              id="mcp-url"
              class="mono"
              type="text"
              bind:value={form.url}
              disabled={busy}
              placeholder="https://example.com/mcp"
              autocomplete="off"
              spellcheck="false"
            />
          </div>

          <div class="mcp-field">
            <div class="mcp-field-head">
              <span class="mcp-field-label">请求头（Headers，仅保存在本机）</span>
              <button type="button" class="btn-link" disabled={busy} onclick={() => addKv('headers')}>+ 添加请求头</button>
            </div>
            {#if form.headers.length > 0}
              <div class="mcp-list-rows">
                {#each form.headers as row, index}
                  <div class="mcp-kv-row">
                    <input
                      class="mono"
                      type="text"
                      value={row.key}
                      disabled={busy}
                      placeholder="Authorization"
                      autocomplete="off"
                      spellcheck="false"
                      oninput={(event) => updateKv('headers', index, 'key', (event.currentTarget as HTMLInputElement).value)}
                    />
                    <div class="mcp-secret-wrap">
                      <input
                        class="mono"
                        type={row.reveal ? 'text' : 'password'}
                        value={row.value}
                        disabled={busy}
                        placeholder="Bearer ..."
                        autocomplete="off"
                        spellcheck="false"
                        oninput={(event) => updateKv('headers', index, 'value', (event.currentTarget as HTMLInputElement).value)}
                      />
                      <button
                        type="button"
                        class="btn-eye"
                        disabled={busy}
                        onclick={() => toggleKvReveal('headers', index)}
                        title={row.reveal ? '隐藏值' : '显示值'}
                        aria-label={row.reveal ? '隐藏值' : '显示值'}
                      >
                        {@html row.reveal ? icons.eyeOff : icons.eye}
                      </button>
                    </div>
                    <button
                      type="button"
                      class="btn-row-remove"
                      disabled={busy}
                      onclick={() => removeKv('headers', index)}
                      title="移除请求头"
                      aria-label="移除请求头"
                    >
                      {@html icons.close}
                    </button>
                  </div>
                {/each}
              </div>
            {/if}
          </div>
        {/if}

        <div class="mcp-form-footer">
          <label class="mcp-check-row">
            <input type="checkbox" bind:checked={form.enabled} disabled={busy} />
            <span>启用此服务</span>
          </label>
          <div class="mcp-actions">
            <button type="button" class="btn-secondary" disabled={busy} onclick={cancelForm}>取消</button>
            <button type="submit" class="btn-primary" disabled={busy}>
              {busy ? '保存中…' : '保存服务器'}
            </button>
          </div>
        </div>
      </form>
    {/if}

    {#if servers.length === 0 && !editing}
      <div class="mcp-empty">
        <span class="mcp-empty-icon">{@html icons.tools}</span>
        <p>暂未配置 MCP 服务器</p>
        <button type="button" class="btn-primary mcp-add-btn" disabled={busy} onclick={openAddForm}>
          <span class="btn-icon">{@html icons.plus}</span>
          <span>添加第一个 MCP</span>
        </button>
      </div>
    {:else if servers.length > 0}
      <div class="mcp-server-list">
        {#each servers as server (server.name)}
          {@const envCount = Object.keys(server.env ?? {}).length}
          {@const headerCount = Object.keys(server.headers ?? {}).length}
          <div class="mcp-server-card" class:disabled={!server.enabled}>
            <div class="mcp-server-main">
              <div class="mcp-server-top">
                <span class="mcp-status-dot" class:enabled={server.enabled}></span>
                <strong class="mcp-server-name">{server.name}</strong>
                <span class="mcp-badge">{transportBadgeLabel(server.transport)}</span>
                {#if !server.enabled}
                  <span class="mcp-badge muted">已停用</span>
                {/if}
                {#if envCount > 0}
                  <span class="mcp-badge subtle">{envCount} 环境变量</span>
                {/if}
                {#if headerCount > 0}
                  <span class="mcp-badge subtle">{headerCount} 请求头</span>
                {/if}
                {#if server.hasExtraFields}
                  <span class="mcp-badge subtle" title="包含其他扩展字段，图形化保存时会自动保留">扩展字段</span>
                {/if}
              </div>
              <p class="mcp-server-summary mono" title={formatServerSummary(server)}>
                {formatServerSummary(server)}
              </p>
            </div>

            <div class="mcp-server-actions">
              <button
                type="button"
                class="mcp-toggle-pill"
                class:on={server.enabled}
                role="switch"
                aria-checked={server.enabled}
                aria-label={server.enabled ? `停用 ${server.name}` : `启用 ${server.name}`}
                disabled={busy}
                onclick={() => void toggleServerEnabled(server)}
                title={server.enabled ? '点击停用' : '点击启用'}
              >
                <span class="mcp-toggle-thumb"></span>
              </button>
              <button
                type="button"
                class="btn-icon-only"
                disabled={busy}
                onclick={() => openEditForm(server)}
                title="编辑服务器"
                aria-label={`编辑 ${server.name}`}
              >
                {@html icons.edit}
              </button>
              <button
                type="button"
                class="btn-icon-only danger"
                disabled={busy}
                onclick={() => void removeServer(server)}
                title="删除服务器"
                aria-label={`删除 ${server.name}`}
              >
                {@html icons.trash}
              </button>
            </div>
          </div>
        {/each}
      </div>
    {/if}
  {:else if view && mode === 'raw'}
    <div class="mcp-raw-box">
      <div class="mcp-raw-bar">
        <p>仅编辑 MCP 段，配置仅保存在本机。</p>
        <div class="mcp-actions">
          <button type="button" class="btn-secondary" disabled={busy} onclick={() => (reveal = !reveal)}>
            {reveal ? '隐藏配置' : '显示并编辑'}
          </button>
          <button type="button" class="btn-primary" disabled={busy || !rawDirty || !reveal} onclick={() => void run(true)}>
            保存源码
          </button>
        </div>
      </div>
      {#if reveal}
        <textarea
          id="mcp-config-draft"
          bind:value={draft}
          disabled={busy}
          spellcheck="false"
          autocomplete="off"
          autocapitalize="off"
        ></textarea>
      {/if}
    </div>
  {/if}
</section>

<style>
  .mcp-panel {
    display: grid;
    gap: 12px;
    padding: 16px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    box-shadow: var(--shadow-card);
  }
  .mcp-heading,
  .mcp-toolbar,
  .mcp-actions,
  .mcp-title-row,
  .mcp-form-header,
  .mcp-form-footer,
  .mcp-field-head,
  .mcp-raw-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }
  .mcp-heading,
  .mcp-form-header,
  .mcp-form-footer,
  .mcp-field-head,
  .mcp-raw-bar {
    justify-content: space-between;
  }
  .mcp-title-block {
    min-width: 0;
  }
  .mcp-title-block strong {
    font-size: 13.5px;
    color: var(--text-primary);
  }
  .mcp-count-pill {
    padding: 1px 8px;
    border-radius: var(--radius-pill);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
  }
  .mcp-path {
    margin: 2px 0 0;
    font-size: 11px;
    color: var(--text-muted);
    overflow-wrap: anywhere;
  }
  .mcp-mode-switch {
    display: inline-flex;
    padding: 2px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
  }
  .mcp-mode-btn {
    height: 24px;
    padding: 0 9px;
    border-radius: calc(var(--radius-sm) - 2px);
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text-secondary);
    transition: all var(--transition-fast);
  }
  .mcp-mode-btn.active {
    background: var(--bg-card);
    color: var(--text-primary);
    box-shadow: var(--shadow-sm);
  }
  .mcp-add-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 28px;
    padding: 0 10px;
    font-size: 12px;
  }
  .btn-icon {
    width: 13px;
    height: 13px;
    display: inline-flex;
  }
  .btn-icon :global(svg),
  .btn-icon-only :global(svg),
  .btn-row-remove :global(svg),
  .btn-eye :global(svg),
  .mcp-empty-icon :global(svg) {
    width: 100%;
    height: 100%;
  }
  .btn-icon-only {
    width: 28px;
    height: 28px;
    padding: 6px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--border-subtle);
    background: transparent;
    color: var(--text-secondary);
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: all var(--transition-fast);
  }
  .btn-icon-only:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-secondary);
    border-color: var(--border-default);
  }
  .btn-icon-only.danger:hover:not(:disabled) {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }
  .btn-link {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--brand-text, var(--brand-primary));
    padding: 2px 4px;
    border-radius: var(--radius-xs);
  }
  .btn-link:hover:not(:disabled) {
    background: var(--brand-subtle);
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
    color: var(--text-secondary);
    font-size: 12px;
  }
  .mcp-error,
  .mcp-notice {
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
  }
  .mcp-error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border: 1px solid var(--color-danger-border);
  }
  .mcp-notice {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border: 1px solid var(--color-exact-border);
  }
  .mcp-select {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-primary);
    font: inherit;
    font-size: 12px;
  }
  .mcp-form {
    display: grid;
    gap: 12px;
    padding: 14px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-md);
    background: var(--bg-canvas);
  }
  .mcp-form-header strong {
    font-size: 13px;
    color: var(--text-primary);
  }
  .mcp-form-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
  }
  .mcp-field {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  label,
  .mcp-field-label {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }
  .mcp-field input[type='text'],
  .mcp-field input[type='password'] {
    width: 100%;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-primary);
    font-size: 12px;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }
  .mcp-field input:focus,
  textarea:focus {
    border-color: var(--brand-primary);
    box-shadow: 0 0 0 2px var(--brand-subtle);
    outline: none;
  }
  .mcp-transport-group {
    display: flex;
    gap: 6px;
    flex-wrap: wrap;
  }
  .mcp-transport-btn {
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-secondary);
    font-size: 12px;
    font-weight: 500;
    transition: all var(--transition-fast);
  }
  .mcp-transport-btn.active {
    border-color: var(--brand-border);
    background: var(--brand-subtle);
    color: var(--brand-text, var(--brand-primary));
  }
  .mcp-presets {
    display: inline-flex;
    gap: 4px;
  }
  .mcp-preset-chip {
    height: 20px;
    padding: 0 7px;
    border-radius: var(--radius-pill);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font: 11px var(--font-mono);
  }
  .mcp-preset-chip:hover {
    border-color: var(--brand-border);
    color: var(--brand-text, var(--brand-primary));
  }
  .mcp-list-rows {
    display: grid;
    gap: 6px;
  }
  .mcp-arg-row {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 28px;
    gap: 6px;
    align-items: center;
  }
  .mcp-kv-row {
    display: grid;
    grid-template-columns: minmax(120px, 0.9fr) minmax(0, 1.4fr) 28px;
    gap: 6px;
    align-items: center;
  }
  .mcp-secret-wrap {
    position: relative;
    display: flex;
    align-items: center;
  }
  .mcp-secret-wrap input {
    padding-right: 30px;
  }
  .btn-eye {
    position: absolute;
    right: 4px;
    width: 24px;
    height: 24px;
    padding: 4px;
    color: var(--text-muted);
    border-radius: var(--radius-xs);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .btn-eye:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }
  .btn-row-remove {
    width: 28px;
    height: 28px;
    padding: 6px;
    color: var(--text-muted);
    border-radius: var(--radius-sm);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }
  .btn-row-remove:hover:not(:disabled) {
    color: var(--color-danger);
    background: var(--color-danger-bg);
  }
  .mcp-json-import {
    display: grid;
    gap: 8px;
  }
  .mcp-snippet-input {
    min-height: 92px;
  }
  .mcp-import-actions {
    display: flex;
    justify-content: flex-end;
  }
  .mcp-check-row {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    color: var(--text-primary);
  }
  .mcp-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    padding: 24px 16px;
    border: 1px dashed var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-canvas);
  }
  .mcp-empty-icon {
    width: 24px;
    height: 24px;
    color: var(--text-muted);
  }
  .mcp-server-list {
    display: grid;
    gap: 8px;
  }
  .mcp-server-card {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-canvas);
    transition: border-color var(--transition-fast), opacity var(--transition-fast);
  }
  .mcp-server-card:hover {
    border-color: var(--border-default);
  }
  .mcp-server-card.disabled {
    opacity: 0.68;
  }
  .mcp-server-main {
    min-width: 0;
    flex: 1;
    display: grid;
    gap: 4px;
  }
  .mcp-server-top {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .mcp-status-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--text-muted);
    flex: 0 0 auto;
  }
  .mcp-status-dot.enabled {
    background: var(--color-exact);
  }
  .mcp-server-name {
    font-size: 13px;
    color: var(--text-primary);
  }
  .mcp-badge {
    padding: 1px 6px;
    border-radius: var(--radius-xs);
    font-size: 10.5px;
    font-weight: 600;
    color: var(--brand-text, var(--brand-primary));
    background: var(--brand-subtle);
    border: 1px solid var(--brand-border);
  }
  .mcp-badge.subtle {
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-color: var(--border-subtle);
  }
  .mcp-badge.muted {
    font-weight: 500;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border-color: var(--border-subtle);
  }
  .mcp-server-summary {
    font-size: 11.5px;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .mcp-server-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 0 0 auto;
  }
  .mcp-toggle-pill {
    width: 34px;
    height: 18px;
    padding: 2px;
    border-radius: var(--radius-pill);
    background: var(--bg-secondary);
    border: 1px solid var(--border-default);
    display: inline-flex;
    align-items: center;
    transition: background-color var(--transition-fast), border-color var(--transition-fast);
  }
  .mcp-toggle-pill.on {
    background: var(--brand-strong);
    border-color: var(--brand-strong);
  }
  .mcp-toggle-thumb {
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #ffffff;
    transform: translateX(0);
    transition: transform var(--transition-fast);
  }
  .mcp-toggle-pill.on .mcp-toggle-thumb {
    transform: translateX(16px);
  }
  .mcp-raw-box {
    display: grid;
    gap: 10px;
  }
  textarea {
    width: 100%;
    min-height: 220px;
    resize: vertical;
    padding: 10px 12px;
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-primary);
    font: 12px/1.55 var(--font-mono);
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast), background-color var(--transition-fast);
  }
  @media (max-width: 780px) {
    .mcp-form-grid,
    .mcp-kv-row {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
