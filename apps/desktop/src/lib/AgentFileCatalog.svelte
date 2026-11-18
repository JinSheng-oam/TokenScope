<script lang="ts">
  import { icons } from "./icons";
  import { instructionInfo, instructionRootLabel, type Entry } from "./agentCustomizationEntries";

  export let instructions: Entry[];
  export let skills: Entry[];
  export let selected: Entry | null;
  export let busy: boolean;
  export let refresh: () => Promise<void>;
  export let select: (entry: Entry) => Promise<void>;
  export let beginCreateInstruction: () => void;
  export let beginImport: () => void;
  export let beginCreate: () => void;

  const categories = [
    { id: 'all', label: '全部' },
    { id: 'instruction', label: '指令' },
    { id: 'skill', label: 'Skills' },
  ] as const;

  let category: (typeof categories)[number]['id'] = 'all';
  let filterQuery = '';
  let list: HTMLDivElement;
  $: query = filterQuery.trim().toLowerCase();
  $: filteredInstructions = instructions.filter(
    (entry) => (entry.exists || !entry.instructionEntry)
      && entry.id.name.toLowerCase().includes(query)
  );
  $: filteredSkills = skills.filter((entry) => entry.id.name.toLowerCase().includes(query));
  $: resultCount = (category === 'skill' ? 0 : filteredInstructions.length)
    + (category === 'instruction' ? 0 : filteredSkills.length);
  $: userSkills = filteredSkills.filter((entry) => !entry.builtin);
  $: builtinSkills = filteredSkills.filter((entry) => entry.builtin);
  $: skillGroups = [
    { label: '用户安装的 Skills', entries: userSkills, builtin: false },
    { label: 'Agent 自带的 Skills', entries: builtinSkills, builtin: true },
  ];

  function selectCategory(next: typeof category) {
    if (category === next) return;
    category = next;
    list.scrollTop = 0;
  }
</script>

  <section class="catalog" aria-label="文件列表">
    <div class="catalog-head">
      <div class="catalog-head-left">
        <strong>文件列表</strong>
        <span class="catalog-total-pill">{resultCount} 项</span>
      </div>
      <button
        class="btn-icon-refresh"
        disabled={busy}
        onclick={() => void refresh()}
        title="刷新本地文件与 Skill 状态"
        aria-label="刷新文件列表"
      >
        <span class:spin={busy}>{@html icons.refresh}</span>
      </button>
    </div>

    <div class="catalog-search-box">
      <svg class="search-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="11" cy="11" r="6" /><path d="m16 16 4 4" />
      </svg>
      <input
        type="search"
        class="catalog-search-input"
        bind:value={filterQuery}
        placeholder="搜索指令或 Skill..."
        aria-label="搜索指令或 Skill"
      />
      {#if filterQuery}
        <button
          type="button"
          class="search-clear-btn"
          onclick={() => filterQuery = ''}
          title="清除搜索"
          aria-label="清除搜索"
        >{@html icons.close}</button>
      {/if}
    </div>

    <div class="catalog-filters" role="group" aria-label="筛选文件类型">
      {#each categories as item (item.id)}
        <button type="button" class="catalog-filter" aria-pressed={category === item.id}
          onclick={() => selectCategory(item.id)}>{item.label}</button>
      {/each}
    </div>

    <div class="catalog-actions" role="group" aria-label="添加文件">
      {#if category !== 'skill'}
        <button class="btn-ghost-sm" disabled={busy} onclick={beginCreateInstruction}>新建指令</button>
      {/if}
      {#if category !== 'instruction'}
        <button class="btn-ghost-sm" disabled={busy} onclick={beginCreate}>新建 Skill</button>
        <button class="btn-ghost-sm" disabled={busy} onclick={beginImport}>导入 Skill</button>
      {/if}
    </div>

    <div class="catalog-items" bind:this={list}>
      {#if category !== 'skill'}
      <div class="group-title">
        <div class="group-title-left">
          <span>指令文件</span>
          <span class="count-badge">{filteredInstructions.length}</span>
        </div>
      </div>

      {#each filteredInstructions as entry}
        {@const info = instructionInfo(entry, instructions)}
        <button
          type="button"
          class="file-row"
          class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name}
          aria-current={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name ? 'true' : undefined}
          disabled={busy || !entry.editable}
          title={info.detail}
          aria-label={`${entry.id.name}，${info.label}。${info.detail}`}
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
        <p class="empty-list">{query ? '未匹配到指令' : '暂无指令文件'}</p>
      {/each}
      {/if}

      {#if category !== 'instruction'}
      {#each skillGroups as group (group.label)}
      <div class="group-title">
        <div class="group-title-left">
          <span>{group.label}</span>
          <span class="count-badge">{group.entries.length}</span>
        </div>
      </div>

      {#each group.entries as entry}
        <button
          type="button"
          class="file-row"
          class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name}
          aria-current={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name ? 'true' : undefined}
          class:row-disabled={!entry.enabled}
          disabled={busy || (!entry.editable && !entry.builtin)}
          title={(entry.editable || entry.builtin) ? (entry.path || entry.builtin_description) : '符号链接请在原位置管理'}
          onclick={() => void select(entry)}
        >
          <div class="file-row-main">
            <span class="file-type-icon skill-icon" aria-hidden="true">
              <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round">
                <polygon points="12 2 2 7 12 12 22 7 12 2" />
                <polyline points="2 17 12 22 22 17" />
                <polyline points="2 12 12 17 22 12" />
              </svg>
            </span>
            <div class="skill-name-col">
              <strong class="file-name" title={entry.id.name}>{entry.id.name}</strong>
              <div class="skill-tags-row">
                {#if !entry.enabled}
                  <span class="status-badge disabled">已停用</span>
                {/if}
                {#if entry.linked}
                  <span class="link-badge">软链接</span>
                {/if}
              </div>
            </div>
          </div>
          <span class="file-sub-root">{(entry.editable || entry.builtin) ? instructionRootLabel(entry.id.root) : '无法访问目标文件'}</span>
        </button>
      {:else}
        <p class="empty-list">{query ? '未匹配到相关 Skill' : group.builtin ? '未发现可识别的自带 Skill。' : '尚未发现用户安装的 Skill。'}</p>
      {/each}
      {/each}
      {/if}
    </div>
  </section>

<style>
  .catalog {
    border-right: 1px solid var(--border-subtle);
    min-width: 0;
    min-height: 0;
    padding: 12px;
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--bg-elevated);
  }

  .catalog-items {
    min-height: 0;
    padding: 4px 3px 2px 2px;
    border-top: 1px solid var(--border-subtle);
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .catalog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
  }

  .catalog-filters {
    display: flex;
    gap: 2px;
    padding: 2px;
    background: var(--bg-secondary);
    border-radius: var(--radius-sm);
  }

  .catalog-filter {
    flex: 1;
    min-height: 28px;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .catalog-filter:hover {
    color: var(--text-primary);
  }

  .catalog-filter[aria-pressed='true'] {
    background: var(--bg-card);
    border-color: var(--border-subtle);
    color: var(--brand-text);
    font-weight: 600;
  }

  .catalog-head-left {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-primary);
  }

  .catalog-total-pill {
    padding: 1px 6px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border-radius: 999px;
  }

  .btn-icon-refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-muted);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-icon-refresh:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .btn-icon-refresh :global(svg) {
    width: 13px;
    height: 13px;
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
    height: 34px;
    padding: 0 32px;
    font: inherit;
    font-size: 11.5px;
    color: var(--text-primary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    transition: all var(--transition-fast);
  }

  .search-icon {
    position: absolute;
    top: 9px;
    left: 10px;
    width: 16px;
    height: 16px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .catalog-search-input::-webkit-search-cancel-button {
    appearance: none;
  }

  .catalog-search-input:focus {
    outline: none;
    border-color: var(--brand-primary);
    background: var(--bg-card);
  }

  .catalog-search-input::placeholder {
    color: var(--text-secondary);
    opacity: 1;
  }

  .search-clear-btn {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
  }

  .search-clear-btn :global(svg) {
    width: 14px;
    height: 14px;
  }

  .group-title {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 6px;
    padding: 4px 8px;
    font-size: 11.5px;
    color: var(--text-secondary);
    font-weight: 600;
  }

  .group-title-left {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 15px;
    padding: 0 4px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 999px;
  }

  .btn-ghost-sm {
    min-height: 28px;
    padding: 0 9px;
    font: inherit;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .btn-ghost-sm:hover:not(:disabled) {
    background: var(--brand-subtle);
    color: var(--brand-text);
    border-color: var(--brand-border);
  }

  .catalog-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
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
    border-radius: var(--radius-xs);
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
  }

  .file-row:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .file-row.row-disabled {
    opacity: 0.65;
  }

  .file-row-main {
    display: flex;
    align-items: center;
    gap: 7px;
    width: 100%;
    min-width: 0;
  }

  .instruction-name-col,
  .skill-name-col {
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
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .file-type-icon svg {
    width: 100%;
    height: 100%;
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
    line-height: 1.35;
  }

  .file-sub-root {
    font-size: 10.5px;
    color: var(--text-secondary);
    padding-left: 21px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .skill-tags-row {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .empty-list {
    font-size: 11px;
    color: var(--text-secondary);
    padding: 10px 4px;
    text-align: center;
    margin: 0;
  }

  :global(.fit-workspace) .catalog { max-height: none; }

  @media (max-width: 640px) {
    .catalog,
    :global(.fit-workspace) .catalog {
      border-right: none;
      border-bottom: 1px solid var(--border-subtle);
      max-height: 320px;
    }
  }
</style>
