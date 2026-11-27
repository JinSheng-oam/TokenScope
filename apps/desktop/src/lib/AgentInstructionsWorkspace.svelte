<script lang="ts">
  import './agentCustomizationBadges.css';
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { confirmDialog, promptDialog } from './dialogStore';
  import { icons } from './icons';
  import {
    instructionInfo,
    instructionRootLabel,
    isReferenced,
    addReference,
    removeReference,
    getMainEntry,
    type Entry,
    type Provider,
  } from './agentCustomizationEntries';
  import AgentFileEditor from './AgentFileEditor.svelte';
  import AgentCreatePanel from './AgentCreatePanel.svelte';
  import AgentAuxReferenceBar from './AgentAuxReferenceBar.svelte';

  export let runtimeAvailable: boolean;
  export let dirty = false;

  const providers: { id: Provider; label: string }[] = [
    { id: 'codex', label: 'Codex' },
    { id: 'claude', label: 'Claude Code' },
    { id: 'antigravity', label: 'Antigravity' },
    { id: 'pi', label: 'Pi Agent' },
    { id: 'deepseek', label: 'DeepSeek Harness' },
  ];

  let provider: Provider = 'codex';
  let entries: Entry[] = [];
  let selected: Entry | null = null;
  let content = '';
  let original: string | null = null;
  let busy = false;
  let error = '';
  let notice = '';
  let creatingInstruction = false;
  let instructionName = '';
  let instructionRoot = '';
  let instructionPreset = '';
  let filterQuery = '';
  let request = 0;
  let copiedPath = false;
  let copyPathTimer: ReturnType<typeof setTimeout> | undefined;

  let isReferencedInMain = false;
  let checkingReference = false;
  let togglingReference = false;
  let targetMainEntryName = '';
  let lastCheckedKey = '';

  function normalizeLines(val: string | null | undefined): string {
    return (val ?? '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  }

  $: dirty = selected !== null && normalizeLines(content) !== normalizeLines(original);
  $: instructions = entries.filter((entry) => entry.kind === 'instruction');
  $: query = filterQuery.trim().toLowerCase();
  $: filteredInstructions = instructions.filter(
    (entry) => (entry.exists || !entry.instructionEntry) && entry.id.name.toLowerCase().includes(query)
  );
  $: instructionPresets = instructions.filter(
    (entry) => entry.id.root === instructionRoot && entry.instructionEntry && !entry.exists
  );
  $: instructionRootOptions = instructions
    .map((entry) => entry.id.root)
    .filter((root, index, roots) => roots.indexOf(root) === index)
    .map((root) => ({ key: root, label: instructionRootLabel(root) }));
  $: instructionPresetOptions = instructionPresets.map((entry) => ({
    key: entry.id.name,
    label: entry.id.name,
  }));

  $: availableMainEntries =
    selected && selected.id.provider !== 'pi' && selected.kind === 'instruction' && !selected.instructionEntry
      ? instructions.filter((item) => item.id.root === selected?.id.root && item.instructionEntry)
      : [];

  $: {
    if (availableMainEntries.length > 0) {
      if (!targetMainEntryName || !availableMainEntries.some((item) => item.id.name === targetMainEntryName)) {
        const preferred = getMainEntry(selected!, instructions);
        targetMainEntryName = preferred?.id.name ?? availableMainEntries[0].id.name;
      }
    } else {
      targetMainEntryName = '';
    }
  }

  $: mainEntryForSelected = availableMainEntries.find((item) => item.id.name === targetMainEntryName) ?? null;

  $: if (selected && selected.kind === 'instruction' && !selected.instructionEntry && mainEntryForSelected) {
    const key = `${selected.id.provider}:${selected.id.root}:${selected.id.name}->${mainEntryForSelected.id.name}`;
    if (key !== lastCheckedKey) {
      lastCheckedKey = key;
      void checkReferenceStatus(selected, mainEntryForSelected);
    }
  } else {
    lastCheckedKey = '';
    isReferencedInMain = false;
  }

  async function checkReferenceStatus(entry: Entry, targetMain: Entry) {
    checkingReference = true;
    try {
      const mainRaw = await invoke<string | null>('read_customization', { id: targetMain.id });
      isReferencedInMain = mainRaw !== null && isReferenced(mainRaw, entry);
    } catch {
      isReferencedInMain = false;
    } finally {
      checkingReference = false;
    }
  }

  async function toggleMainReference() {
    if (!selected || selected.kind !== 'instruction' || selected.instructionEntry || togglingReference) return;
    const targetEntry = mainEntryForSelected;
    if (!targetEntry) return;

    togglingReference = true;
    error = '';
    notice = '';
    try {
      if (!selected.exists || dirty) {
        await save();
      }
      const mainRaw = await invoke<string | null>('read_customization', { id: targetEntry.id });
      const mainContent = mainRaw ?? '';
      const currentlyReferenced = isReferenced(mainContent, selected);
      const nextReferenced = !currentlyReferenced;

      const refLine = selected.path.includes(' ') ? `@"${selected.path}"` : `@${selected.path}`;
      const nextContent = nextReferenced
        ? addReference(mainContent, refLine)
        : removeReference(mainContent, selected);
      const toSave = mainRaw && mainRaw.includes('\r\n') ? nextContent.replace(/\r?\n/g, '\r\n') : nextContent;

      await invoke('save_customization', { id: targetEntry.id, expected: mainRaw, content: toSave });
      isReferencedInMain = nextReferenced;
      notice = nextReferenced
        ? `已在 ${targetEntry.id.name} 中添加引用：${refLine}`
        : `已从 ${targetEntry.id.name} 中移除引用`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) {
      error = `更新主入口引用失败：${String(e)}`;
    } finally {
      togglingReference = false;
    }
  }

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      copiedPath = true;
      if (copyPathTimer !== undefined) clearTimeout(copyPathTimer);
      copyPathTimer = setTimeout(() => {
        copiedPath = false;
        copyPathTimer = undefined;
      }, 2000);
    } catch {
      // ignore
    }
  }

  async function canLeave(): Promise<boolean> {
    if (!dirty) return true;
    return confirmDialog({
      title: '未保存的更改',
      message: '当前指令文件有未保存的更改，确定放弃更改？',
      confirmText: '放弃更改',
    });
  }

  async function refresh() {
    if (!runtimeAvailable) return;
    const current = ++request;
    busy = true;
    error = '';
    try {
      const result = await invoke<Entry[]>('list_customizations', { provider });
      if (current === request) entries = result;
    } catch (e) {
      if (current === request) error = String(e);
    } finally {
      if (current === request) busy = false;
    }
  }

  async function switchProvider(next: Provider) {
    if (provider === next || !(await canLeave())) return;
    provider = next;
    selected = null;
    content = '';
    original = null;
    creatingInstruction = false;
    notice = '';
    await refresh();
  }

  async function select(entry: Entry) {
    if (selected?.id.root === entry.id.root && selected?.id.name === entry.id.name) return;
    if (!(await canLeave()) || !entry.editable) return;
    const current = ++request;
    busy = true;
    error = '';
    notice = '';
    try {
      const value = await invoke<string | null>('read_customization', { id: entry.id });
      if (current !== request) return;
      selected = entry;
      original = value;
      content = value ? normalizeLines(value) : '';
      creatingInstruction = false;
    } catch (e) {
      if (current === request) error = String(e);
    } finally {
      if (current === request) busy = false;
    }
  }

  async function save() {
    if (!selected || !selected.editable || busy) return;
    busy = true;
    error = '';
    notice = '';
    try {
      const toSave = original && original.includes('\r\n') ? content.replace(/\r?\n/g, '\r\n') : content;
      await invoke('save_customization', { id: selected.id, expected: original, content: toSave });
      original = toSave;
      content = normalizeLines(toSave);
      selected = { ...selected, exists: true };
      notice = '已保存到工具的本地文件。';
      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function renameCustomization() {
    if (!selected || !selected.exists || busy) return;
    const newName = await promptDialog({
      title: '重命名指令文件',
      message: '请输入新的指令文件名（必须以 .md 结尾）：',
      defaultValue: selected.id.name,
      confirmText: '重命名',
    });
    if (!newName || newName.trim() === '' || newName.trim() === selected.id.name) return;
    busy = true;
    error = '';
    notice = '';
    const oldEntry = selected;
    const wasReferenced = isReferencedInMain && mainEntryForSelected;
    const targetMain = mainEntryForSelected;
    try {
      const updated = await invoke<Entry>('rename_customization', {
        id: selected.id,
        newName: newName.trim(),
      });
      notice = `已成功重命名为 ${updated.id.name}`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
      selected = updated;

      if (wasReferenced && targetMain) {
        try {
          const mainRaw = await invoke<string | null>('read_customization', { id: targetMain.id });
          if (mainRaw) {
            let next = removeReference(mainRaw, oldEntry);
            const refLine = updated.path.includes(' ') ? `@"${updated.path}"` : `@${updated.path}`;
            next = addReference(next, refLine);
            const toSave = mainRaw.includes('\r\n') ? next.replace(/\r?\n/g, '\r\n') : next;
            await invoke('save_customization', { id: targetMain.id, expected: mainRaw, content: toSave });
            isReferencedInMain = true;
          }
        } catch {
          // best effort
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function deleteInstruction() {
    if (!selected || selected.kind !== 'instruction' || !selected.exists || busy || selected.linked) return;
    const unsavedNote = dirty ? '\n\n未保存的编辑也会丢弃。' : '';
    const confirmed = await confirmDialog({
      title: '删除指令文件',
      message: `确定删除指令文件“${selected.id.name}”？\n\n${selected.path}\n\n此操作无法撤销。${unsavedNote}`,
      tone: 'danger',
      confirmText: '确认删除',
    });
    if (!confirmed) return;

    const deletedEntry = selected;
    const wasReferenced = isReferencedInMain && mainEntryForSelected;
    const targetMain = mainEntryForSelected;
    const deletedName = selected.id.name;
    busy = true;
    error = '';
    notice = '';
    try {
      await invoke('delete_instruction', { id: selected.id });
      selected = null;
      original = null;
      content = '';
      entries = await invoke<Entry[]>('list_customizations', { provider });
      notice = `已删除指令文件：${deletedName}`;

      if (wasReferenced && targetMain) {
        try {
          const mainRaw = await invoke<string | null>('read_customization', { id: targetMain.id });
          if (mainRaw) {
            const next = removeReference(mainRaw, deletedEntry);
            const toSave = mainRaw.includes('\r\n') ? next.replace(/\r?\n/g, '\r\n') : next;
            await invoke('save_customization', { id: targetMain.id, expected: mainRaw, content: toSave });
          }
        } catch {
          // best effort
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function beginCreateInstruction() {
    if (!(await canLeave())) return;
    selected = null;
    original = null;
    content = '';
    instructionName = '';
    instructionPreset = '';
    const instRoots = instructions.map((e) => e.id.root);
    instructionRoot = instRoots[0] || `${provider}-instructions`;
    creatingInstruction = true;
    error = '';
    notice = '';
  }

  async function createInstruction() {
    let name = instructionName.trim();
    if (!name.toLowerCase().endsWith('.md')) name = `${name}.md`;
    if (name.length < 4 || name.length > 64) {
      error = '请输入有效的文件名（必须以 .md 结尾）';
      return;
    }
    busy = true;
    error = '';
    try {
      const newEntry = await invoke<Entry>('prepare_instruction', {
        provider,
        root: instructionRoot,
        name,
      });
      creatingInstruction = false;
      selected = newEntry;
      original = null;
      content = `# ${name}\n\n`;
      notice = '这是尚未落盘的草稿；保存后才会创建文件。';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void refresh();
  });

  onDestroy(() => {
    if (copyPathTimer !== undefined) clearTimeout(copyPathTimer);
  });
</script>

<div class="custom-page fit-workspace instr-workspace">
  <div class="workspace-toolbar">
    <div class="provider-tabs" role="group" aria-label="选择工具">
      {#each providers as item}
        <button
          type="button"
          class="provider-tab-btn"
          class:selected={provider === item.id}
          aria-pressed={provider === item.id}
          disabled={busy}
          onclick={() => void switchProvider(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </div>
  </div>

  {#if error}<p class="message error" role="alert">{error}</p>{/if}
  {#if notice}<p class="message success" role="status">{notice}</p>{/if}

  <div class="workspace">
    <section class="catalog" aria-label="指令文件列表">
      <div class="catalog-top">
        <div class="catalog-head">
          <div class="catalog-head-left">
            <strong>本地指令</strong>
            <span class="catalog-total-pill">{filteredInstructions.length} 项</span>
          </div>
          <div class="catalog-head-actions">
            <button
              type="button"
              class="btn-ghost-sm"
              disabled={busy}
              onclick={() => void beginCreateInstruction()}
            >
              <span class="btn-ghost-icon" aria-hidden="true">{@html icons.plus}</span>
              <span>新建指令</span>
            </button>
            <button
              type="button"
              class="btn-icon-refresh"
              disabled={busy}
              onclick={() => void refresh()}
              title="刷新本地指令文件"
              aria-label="刷新指令文件列表"
            >
              <span class:spin={busy}>{@html icons.refresh}</span>
            </button>
          </div>
        </div>

        <div class="catalog-search-box">
          <span class="search-icon" aria-hidden="true">{@html icons.search}</span>
          <input
            type="search"
            class="catalog-search-input"
            bind:value={filterQuery}
            placeholder="搜索指令文件..."
            aria-label="搜索指令文件"
          />
          {#if filterQuery}
            <button
              type="button"
              class="search-clear-btn"
              onclick={() => (filterQuery = '')}
              title="清除搜索"
              aria-label="清除搜索"
            >
              {@html icons.close}
            </button>
          {/if}
        </div>
      </div>

      <div class="catalog-items">
        {#each filteredInstructions as entry}
          {@const info = instructionInfo(entry, instructions)}
          <button
            type="button"
            class="file-row"
            class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name}
            aria-current={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name ? 'true' : undefined}
            disabled={busy || !entry.editable}
            title={info.detail}
            onclick={() => void select(entry)}
          >
            <div class="file-row-main">
              <span class="file-type-icon" aria-hidden="true">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                  <path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z" />
                  <polyline points="14 2 14 8 20 8" />
                  <line x1="8" y1="13" x2="16" y2="13" />
                  <line x1="8" y1="17" x2="13" y2="17" />
                </svg>
              </span>
              <div class="instruction-name-col">
                <strong class="file-name" title={entry.id.name}>{entry.id.name}</strong>
                <span class={`instruction-badge ${info.tone}`}>{info.label}</span>
              </div>
            </div>
            <span class="file-sub-root">{instructionRootLabel(entry.id.root)} · {entry.exists ? '已保存' : '未创建'}</span>
          </button>
        {:else}
          <p class="empty-list">{query ? '未匹配到指令文件' : '暂无指令文件'}</p>
        {/each}
      </div>
    </section>

    <section class="editor" class:is-editing={Boolean(selected && !creatingInstruction)} aria-label="指令编辑器">
      {#if creatingInstruction}
        <AgentCreatePanel
          mode="instruction"
          {provider}
          {busy}
          {instructionRootOptions}
          {instructionPresets}
          {instructionPresetOptions}
          providerRootOptions={[]}
          bind:instructionRoot
          bind:instructionPreset
          bind:instructionName
          onCreateInstruction={() => void createInstruction()}
          onCancel={() => { creatingInstruction = false; }}
        />
      {:else if selected}
        {@const info = instructionInfo(selected, instructions)}
        <div class="editor-head">
          <div class="editor-head-top">
            <div class="editor-title-group">
              <div class="editor-title-row">
                <h2 class="editor-file-title" title={selected.id.name}>{selected.id.name}</h2>
                <div class="editor-badges">
                  <span class={`instruction-badge ${info.tone}`} title={info.detail}>{info.label}</span>
                  {#if selected.linked}
                    <span class="link-badge">软链接目标</span>
                  {/if}
                </div>
              </div>
              <div class="editor-path-row">
                <span class="editor-path" title={selected.path}>{selected.path}</span>
                {#if selected.path}
                  <button
                    type="button"
                    class="copy-path-btn"
                    class:copied={copiedPath}
                    onclick={() => { if (selected) void copyPath(selected.path); }}
                  >
                    {#if copiedPath}
                      <span class="copy-success" role="status">已复制</span>
                    {:else}
                      <span>复制路径</span>
                    {/if}
                  </button>
                {/if}
              </div>
            </div>

            {#if selected.editable}
              <button
                type="button"
                class="btn-primary btn-save"
                disabled={busy || (!dirty && selected.exists)}
                onclick={() => void save()}
              >
                {busy ? '处理中…' : selected.exists ? '保存更改' : '创建文件'}
              </button>
            {/if}
          </div>

          {#if selected.instructionEntry || !mainEntryForSelected}
            <p class={`instruction-status-note ${info.tone}`}>{info.detail}</p>
          {/if}

          <div class="editor-actions">
            {#if selected.exists && selected.editable}
              <button
                type="button"
                class="btn-secondary btn-action"
                disabled={busy}
                onclick={() => void renameCustomization()}
              >
                改名
              </button>
            {/if}
            {#if selected.exists}
              <button
                type="button"
                class="btn-secondary btn-action btn-danger"
                disabled={busy || selected.linked}
                onclick={() => void deleteInstruction()}
                title={selected.linked ? '符号链接文件请在原位置管理' : '删除此指令文件'}
              >
                删除
              </button>
            {/if}
          </div>
        </div>

        {#if !selected.instructionEntry && mainEntryForSelected}
          <AgentAuxReferenceBar
            {selected}
            {mainEntryForSelected}
            {availableMainEntries}
            bind:targetMainEntryName
            {isReferencedInMain}
            {checkingReference}
            {togglingReference}
            {busy}
            onToggleReference={() => void toggleMainReference()}
          />
        {/if}

        <AgentFileEditor bind:content {busy} {dirty} exists={selected.exists} readOnly={!selected.editable} />
      {:else}
        <div class="editor-empty">
          <div class="empty-icon-box">{@html icons.edit}</div>
          <h2>从左侧选择指令文件进行编辑</h2>
        </div>
      {/if}
    </section>
  </div>
</div>

<style>
  .instr-workspace {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .workspace-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 5px 6px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    flex-shrink: 0;
  }

  .provider-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }

  .provider-tab-btn {
    min-height: 30px;
    padding: 0 11px;
    border: 1px solid transparent;
    background: transparent;
    border-radius: var(--radius-sm);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .provider-tab-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }

  .provider-tab-btn.selected {
    background: var(--brand-subtle);
    color: var(--brand-text);
    border-color: var(--brand-border);
    font-weight: 600;
  }

  .workspace {
    flex: 1;
    min-height: 320px;
    display: grid;
    grid-template-columns: clamp(232px, 30%, 304px) minmax(0, 1fr);
    grid-template-rows: minmax(0, 1fr);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    overflow: hidden;
  }

  .catalog {
    border-right: 1px solid var(--border-subtle);
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
  }

  .catalog-top {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 12px 10px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .catalog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .catalog-head-left {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-primary);
  }

  .catalog-head-actions {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .catalog-total-pill {
    padding: 1px 6px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border-radius: 999px;
    font-variant-numeric: tabular-nums;
  }

  .btn-ghost-sm {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 26px;
    padding: 0 8px;
    font: inherit;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .btn-ghost-sm:hover:not(:disabled) {
    background: var(--brand-subtle);
    color: var(--brand-text);
    border-color: var(--brand-border);
  }

  .btn-ghost-icon {
    display: inline-flex;
    width: 11px;
    height: 11px;
  }

  .btn-ghost-icon :global(svg),
  .search-icon :global(svg),
  .search-clear-btn :global(svg),
  .btn-icon-refresh :global(svg) {
    width: 100%;
    height: 100%;
  }

  .btn-icon-refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    padding: 6px;
    border: 1px solid var(--border-subtle);
    background: var(--bg-card);
    color: var(--text-muted);
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-icon-refresh:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .btn-icon-refresh :global(.spin) {
    animation: spin 0.8s linear infinite;
  }

  .catalog-search-box {
    position: relative;
    width: 100%;
  }

  .catalog-search-input {
    width: 100%;
    height: 31px;
    padding: 0 28px 0 30px;
    font: inherit;
    font-size: 11.5px;
    color: var(--text-primary);
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .catalog-search-input:focus {
    outline: none;
    border-color: var(--brand-primary);
    box-shadow: 0 0 0 2px var(--brand-subtle);
  }

  .search-icon {
    position: absolute;
    top: 50%;
    left: 9px;
    transform: translateY(-50%);
    width: 13px;
    height: 13px;
    display: inline-flex;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search-clear-btn {
    position: absolute;
    right: 5px;
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 18px;
    padding: 2px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .catalog-items {
    flex: 1;
    min-height: 0;
    padding: 6px 8px 10px;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .file-row {
    flex-shrink: 0;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    gap: 2px;
    text-align: left;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text-primary);
    padding: 6px 8px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .file-row:hover:not(:disabled) {
    background: var(--bg-secondary);
    border-color: var(--border-subtle);
  }

  .file-row.active {
    background: var(--brand-subtle);
    border-color: var(--brand-border);
    box-shadow: inset 2.5px 0 0 var(--brand-primary);
  }

  .file-row-main {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    min-width: 0;
  }

  .instruction-name-col {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }

  .file-type-icon {
    width: 14px;
    height: 14px;
    display: inline-flex;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .file-row.active .file-type-icon {
    color: var(--brand-primary);
  }

  .file-name {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
  }

  .file-sub-root {
    font-size: 10.5px;
    color: var(--text-muted);
    padding-left: 21px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .empty-list {
    font-size: 11.5px;
    color: var(--text-muted);
    padding: 16px 8px;
    text-align: center;
    margin: 0;
  }

  .editor {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    overflow: auto;
    background: var(--bg-card);
  }

  .editor.is-editing {
    overflow: hidden;
  }

  .editor-head {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    gap: 8px;
    padding: 12px 16px 10px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .editor-head-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  .editor-title-group {
    flex: 1;
    min-width: 0;
  }

  .editor-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .editor-file-title {
    font-size: 14.5px;
    font-weight: 700;
    color: var(--text-primary);
    margin: 0;
    line-height: 1.3;
  }

  .editor-badges {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .editor-path-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    min-width: 0;
  }

  .editor-path {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }

  .copy-path-btn {
    height: 21px;
    padding: 0 7px;
    font: inherit;
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    cursor: pointer;
    flex-shrink: 0;
  }

  .copy-path-btn.copied {
    border-color: var(--color-exact-border);
    background: var(--color-exact-bg);
  }

  .copy-success {
    color: var(--color-exact);
    font-weight: 600;
  }

  .instruction-status-note {
    margin: 0;
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.45;
  }

  .instruction-status-note.warning {
    color: var(--color-estimated);
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .editor-actions:empty {
    display: none;
  }

  .btn-action {
    height: 26px;
    padding: 0 9px;
    font-size: 11.5px;
  }

  .btn-save {
    height: 29px;
    padding: 0 14px;
    font-size: 11.5px;
    font-weight: 600;
    flex-shrink: 0;
  }

  .editor-empty {
    margin: auto;
    text-align: center;
    padding: 40px 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .empty-icon-box {
    width: 42px;
    height: 42px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    color: var(--text-muted);
  }

  .empty-icon-box :global(svg) {
    width: 19px;
    height: 19px;
  }

  .editor-empty h2 {
    font-size: 13.5px;
    color: var(--text-secondary);
    margin: 0;
    font-weight: 500;
  }

  .message {
    font-size: 11.5px;
    padding: 7px 12px;
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

  @media (max-width: 640px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr);
      grid-template-rows: auto minmax(260px, 1fr);
    }
    .catalog {
      border-right: none;
      border-bottom: 1px solid var(--border-subtle);
      max-height: 260px;
    }
  }
</style>
