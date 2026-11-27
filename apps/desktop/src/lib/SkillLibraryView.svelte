<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { icons } from './icons';
  import { ALL_PROVIDERS } from './providerBadges';
  import type { Id, Provider } from './agentCustomizationEntries';
  import type { SkillSummary } from './agentSkillSourceTypes';
  import {
    filterSkillItems,
    groupSkillItemsBySource,
    isSkillAnalyzable,
    type SkillAiFilter,
    type SkillDeployment,
    type SkillFilterState,
    type SkillLibraryItem,
    type SkillSourceFilter,
    type SkillStatusFilter,
    type SkillViewMode,
  } from './skillLibraryTypes';
  import {
    createSkillBatchController,
    emptyBatchProgress,
    loadBatchAnalysisPrefs,
    saveBatchAnalysisPrefs,
    selectBatchTargets,
    type BatchAnalysisMode,
    type BatchAnalysisPrefs,
    type BatchProgress,
  } from './skillBatchAnalyzer';
  import SkillBatchPanel from './SkillBatchPanel.svelte';
  import SkillCard from './SkillCard.svelte';
  import SkillListRow from './SkillListRow.svelte';
  import SkillDetailView from './SkillDetailView.svelte';
  import AgentCreatePanel from './AgentCreatePanel.svelte';
  import AgentSkillImport from './AgentSkillImport.svelte';

  export let runtimeAvailable: boolean;
  export let hasAiConfig = false;
  export let dirty = false;
  export let totalCount = 0;
  export let onOpenAiConfig: () => void = () => {};

  const VIEW_STORAGE_KEY = 'tokenscope:skill-library-view';

  const providerRoots: Record<Provider, { id: string; label: string }[]> = {
    codex: [
      { id: 'shared-skills', label: '共享 skills' },
      { id: 'codex-skills', label: 'Codex skills' },
    ],
    claude: [{ id: 'claude-skills', label: 'Claude skills' }],
    antigravity: [{ id: 'antigravity-skills', label: 'Antigravity skills' }],
    pi: [
      { id: 'pi-skills', label: 'Pi skills' },
      { id: 'pi-shared-skills', label: '共享 skills' },
    ],
    deepseek: [
      { id: 'dsh-skills', label: 'DeepSeek skills' },
      { id: 'dsh-shared-skills', label: '共享 skills' },
    ],
  };

  let items: SkillLibraryItem[] = [];
  let loading = false;
  let busy = false;
  let error = '';
  let notice = '';

  let viewMode: SkillViewMode = 'grid';
  let grouped = false;
  let collapsedGroups: Record<string, boolean> = {};

  let filter: SkillFilterState = {
    query: '',
    status: 'all',
    source: 'all',
    ai: 'all',
    provider: 'all',
  };

  let multiSelect = false;
  let selectedKeys = new Set<string>();
  let activeItemKey: string | null = null;
  let detailDirty = false;

  // Create / Import overlays
  let panelMode: 'none' | 'create' | 'import' = 'none';
  let targetProvider: Provider = 'codex';
  let skillRoot = providerRoots.codex[0].id;
  let skillName = '';

  // Batch AI Analysis state
  let showBatchPanel = false;
  let batchMode: BatchAnalysisMode = 'unanalyzed';
  let batchPrefs: BatchAnalysisPrefs = loadBatchAnalysisPrefs();
  let batchProgress: BatchProgress = emptyBatchProgress();

  const batchController = createSkillBatchController({
    analyze: (item) => invoke<SkillSummary>('generate_skill_summary', { id: item.primary_id }),
    onItemSuccess: (item, summary) => {
      items = items.map((entry) => (entry.key === item.key ? { ...entry, summary } : entry));
    },
    onChange: (next) => {
      batchProgress = next;
    },
  });

  function loadViewPrefs() {
    try {
      const raw = localStorage.getItem(VIEW_STORAGE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as { viewMode?: SkillViewMode; grouped?: boolean };
      if (parsed.viewMode === 'grid' || parsed.viewMode === 'list') viewMode = parsed.viewMode;
      if (typeof parsed.grouped === 'boolean') grouped = parsed.grouped;
    } catch {
      // ignore
    }
  }

  function persistViewPrefs() {
    try {
      localStorage.setItem(VIEW_STORAGE_KEY, JSON.stringify({ viewMode, grouped }));
    } catch {
      // ignore
    }
  }

  $: totalCount = items.length;
  $: dirty = detailDirty;
  $: failedKeySet = new Set(batchProgress.failedList.map((f) => f.key));
  $: filteredItems = filterSkillItems(items, filter, failedKeySet);
  $: groupedItems = grouped ? groupSkillItemsBySource(filteredItems) : [];
  $: activeItem = activeItemKey ? items.find((i) => i.key === activeItemKey) ?? null : null;
  $: analyzableItems = items.filter(isSkillAnalyzable);
  $: unanalyzedCount = analyzableItems.filter((i) => i.summary === null).length;
  $: batchTargetCount = selectBatchTargets(
    items,
    batchMode,
    selectedKeys,
    batchPrefs.skipAnalyzedOnSelected
  ).length;
  $: providerRootOptions = providerRoots[targetProvider].map((r) => ({ key: r.id, label: r.label }));

  async function refresh() {
    if (!runtimeAvailable) return;
    loading = true;
    error = '';
    try {
      items = await invoke<SkillLibraryItem[]>('list_skill_library');
      if (activeItemKey && !items.some((i) => i.key === activeItemKey)) {
        activeItemKey = null;
      }
      // Prune stale selections
      const existing = new Set(items.map((i) => i.key));
      const nextSelected = new Set<string>();
      for (const k of selectedKeys) {
        if (existing.has(k)) nextSelected.add(k);
      }
      selectedKeys = nextSelected;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  async function toggleEnabled(item: SkillLibraryItem, specificDeployment?: SkillDeployment) {
    if (busy || item.builtin) return;
    const targetId = specificDeployment?.id ?? item.primary_id;
    const currentEnabled = specificDeployment ? specificDeployment.enabled : item.enabled;
    const nextEnabled = !currentEnabled;

    busy = true;
    error = '';
    notice = '';
    try {
      await invoke('toggle_skill_enabled', { id: targetId, enabled: nextEnabled });
      await refresh();
      notice = nextEnabled ? `已启用 ${item.name}` : `已停用 ${item.name}`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function bulkToggleEnabled(enable: boolean) {
    if (busy || selectedKeys.size === 0) return;
    const targets = items.filter((i) => selectedKeys.has(i.key) && !i.builtin && i.enabled !== enable);
    if (targets.length === 0) {
      notice = enable ? '所选技能均已启用。' : '所选技能均已停用。';
      return;
    }
    busy = true;
    error = '';
    notice = '';
    let changed = 0;
    try {
      for (const item of targets) {
        try {
          await invoke('toggle_skill_enabled', { id: item.primary_id, enabled: enable });
          changed += 1;
        } catch {
          // continue others
        }
      }
      await refresh();
      notice = enable ? `已批量启用 ${changed} 个技能。` : `已批量停用 ${changed} 个技能。`;
    } finally {
      busy = false;
    }
  }

  function toggleSelectKey(key: string) {
    const next = new Set(selectedKeys);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    selectedKeys = next;
  }

  function selectAllFiltered() {
    selectedKeys = new Set(filteredItems.map((i) => i.key));
  }

  function selectUnanalyzedFiltered() {
    selectedKeys = new Set(
      filteredItems.filter((i) => isSkillAnalyzable(i) && i.summary === null).map((i) => i.key)
    );
  }

  function exitMultiSelect() {
    multiSelect = false;
    selectedKeys = new Set();
    if (batchMode === 'selected') batchMode = 'unanalyzed';
  }

  function handleUpdateBatchPrefs(next: BatchAnalysisPrefs) {
    batchPrefs = saveBatchAnalysisPrefs(next);
  }

  function openBatchAnalysis(preferredMode?: BatchAnalysisMode) {
    if (preferredMode) batchMode = preferredMode;
    showBatchPanel = true;
  }

  async function startBatchAnalysis() {
    if (!hasAiConfig) {
      onOpenAiConfig();
      error = '请先完成 AI 助手接口和模型配置后再运行批量分析。';
      return;
    }
    error = '';
    notice = '';
    await batchController.start(items, {
      mode: batchMode,
      selectedKeys,
      skipAnalyzedOnSelected: batchPrefs.skipAnalyzedOnSelected,
      concurrency: batchPrefs.concurrency,
      maxRetries: batchPrefs.maxRetries,
    });
  }

  async function retryFailedBatch() {
    if (!hasAiConfig) {
      onOpenAiConfig();
      return;
    }
    error = '';
    await batchController.retryFailed(items, {
      concurrency: batchPrefs.concurrency,
      maxRetries: batchPrefs.maxRetries,
    });
  }

  async function analyzeSingleItem(item: SkillLibraryItem) {
    if (!hasAiConfig) {
      onOpenAiConfig();
      error = '请先完成 AI 助手接口和模型配置。';
      return;
    }
    error = '';
    await batchController.retrySingle(item, batchPrefs.maxRetries);
  }

  function retrySingleByKey(key: string) {
    const target = items.find((i) => i.key === key);
    if (target) void analyzeSingleItem(target);
  }

  function setStatusFilter(next: SkillStatusFilter) {
    filter = { ...filter, status: next };
  }

  function toggleSourceFilter(next: Exclude<SkillSourceFilter, 'all'>) {
    filter = { ...filter, source: filter.source === next ? 'all' : next };
  }

  function toggleAiFilter(next: Exclude<SkillAiFilter, 'all'>) {
    filter = { ...filter, ai: filter.ai === next ? 'all' : next };
  }

  function setProviderFilter(next: 'all' | Provider) {
    filter = { ...filter, provider: filter.provider === next ? 'all' : next };
  }

  function toggleGroupCollapsed(group: string) {
    collapsedGroups = { ...collapsedGroups, [group]: !collapsedGroups[group] };
  }

  function openCreateSkill() {
    panelMode = 'create';
    skillName = '';
    skillRoot = providerRoots[targetProvider][0].id;
    error = '';
    notice = '';
  }

  function openImportSkill() {
    panelMode = 'import';
    skillRoot = providerRoots[targetProvider][0].id;
    error = '';
    notice = '';
  }

  function handleTargetProviderChange(next: Provider) {
    targetProvider = next;
    skillRoot = providerRoots[next][0].id;
  }

  async function handleCreateSkill() {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillName) || skillName.length > 64) {
      error = '名称使用小写字母、数字和连字符，最长 64 个字符。';
      return;
    }
    const id: Id = { provider: targetProvider, root: skillRoot, name: skillName };
    busy = true;
    error = '';
    notice = '';
    try {
      const existing = await invoke<string | null>('read_customization', { id });
      if (existing !== null) {
        error = '这个 Skill 已存在，请在技能库中直接打开。';
        return;
      }
      const starter = `---\nname: ${skillName}\ndescription: \n---\n\n# ${skillName}\n\n`;
      await invoke('save_customization', { id, expected: null, content: starter });
      panelMode = 'none';
      await refresh();
      const created = items.find((i) => i.name === skillName);
      if (created) activeItemKey = created.key;
      notice = `已创建 Skill：${skillName}`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    loadViewPrefs();
    void refresh();
  });

  onDestroy(() => {
    batchController.stop();
  });
</script>

{#if activeItem}
  <SkillDetailView
    item={activeItem}
    batchState={batchProgress.items[activeItem.key]}
    {hasAiConfig}
    bind:dirty={detailDirty}
    onClose={() => { activeItemKey = null; detailDirty = false; }}
    onRefreshLibrary={refresh}
    onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
    onToggleDeployment={toggleEnabled}
    {onOpenAiConfig}
  />
{:else if panelMode !== 'none'}
  <div class="overlay-card">
    <div class="overlay-provider-bar">
      <span class="overlay-label">目标工具：</span>
      <div class="overlay-provider-pills">
        {#each ALL_PROVIDERS as p (p.id)}
          <button
            type="button"
            class="overlay-provider-btn"
            class:selected={targetProvider === p.id}
            onclick={() => handleTargetProviderChange(p.id)}
          >
            {p.label}
          </button>
        {/each}
      </div>
    </div>

    {#if error}<p class="msg error" role="alert">{error}</p>{/if}

    {#if panelMode === 'create'}
      <AgentCreatePanel
        mode="skill"
        provider={targetProvider}
        {busy}
        {providerRootOptions}
        bind:skillRoot
        bind:skillName
        onCreateSkill={() => void handleCreateSkill()}
        onCancel={() => { panelMode = 'none'; error = ''; }}
      />
    {:else}
      <AgentSkillImport
        provider={targetProvider}
        {providerRootOptions}
        bind:skillRoot
        bind:busy
        bind:error
        onInstall={() => {
          panelMode = 'none';
          notice = '导入成功，已在技能库中刷新。';
          void refresh();
        }}
        onCancel={() => { panelMode = 'none'; error = ''; }}
      />
    {/if}
  </div>
{:else}
  <div class="library-shell">
    <!-- Toolbar Row 1 -->
    <div class="toolbar-row">
      <div class="toolbar-left">
        <div class="search-box">
          <span class="search-icon" aria-hidden="true">{@html icons.search}</span>
          <input
            type="search"
            class="search-input"
            bind:value={filter.query}
            placeholder="搜索技能名称、AI 简介或来源..."
            aria-label="搜索技能"
          />
          {#if filter.query}
            <button
              type="button"
              class="search-clear"
              onclick={() => (filter = { ...filter, query: '' })}
              aria-label="清除搜索"
            >
              {@html icons.close}
            </button>
          {/if}
        </div>

        <div class="segmented" role="group" aria-label="按启用状态筛选">
          <button
            type="button"
            class="seg-btn"
            class:active={filter.status === 'all'}
            onclick={() => setStatusFilter('all')}
          >
            全部
          </button>
          <button
            type="button"
            class="seg-btn"
            class:active={filter.status === 'enabled'}
            onclick={() => setStatusFilter('enabled')}
          >
            已启用
          </button>
          <button
            type="button"
            class="seg-btn"
            class:active={filter.status === 'disabled'}
            onclick={() => setStatusFilter('disabled')}
          >
            未启用
          </button>
        </div>
      </div>

      <div class="toolbar-right">
        <button
          type="button"
          class="toolbar-btn ai-batch-btn"
          class:active={showBatchPanel || batchProgress.running}
          onclick={() => {
            if (batchProgress.running) {
              showBatchPanel = true;
            } else {
              showBatchPanel = !showBatchPanel;
            }
          }}
        >
          <span class="btn-svg" class:spin={batchProgress.running} aria-hidden="true">{@html icons.sparkle}</span>
          {#if batchProgress.running}
            <span>分析中 {batchProgress.completed}/{batchProgress.total}</span>
          {:else}
            <span>批量 AI 分析</span>
            {#if unanalyzedCount > 0}
              <span class="count-pill">{unanalyzedCount}</span>
            {/if}
          {/if}
        </button>

        <button type="button" class="toolbar-btn" disabled={busy} onclick={openCreateSkill}>
          <span class="btn-svg" aria-hidden="true">{@html icons.plus}</span>
          <span>新建</span>
        </button>

        <button type="button" class="toolbar-btn" disabled={busy} onclick={openImportSkill}>
          <span class="btn-svg" aria-hidden="true">{@html icons.arrowDown}</span>
          <span>导入</span>
        </button>

        <button
          type="button"
          class="toolbar-icon-btn"
          disabled={loading}
          onclick={() => void refresh()}
          title="刷新技能库"
          aria-label="刷新技能库"
        >
          <span class="btn-svg" class:spin={loading}>{@html icons.refresh}</span>
        </button>

        <div class="view-switcher" role="group" aria-label="视图布局">
          <button
            type="button"
            class="view-btn"
            class:active={viewMode === 'grid'}
            title="卡片网格视图"
            aria-label="卡片网格视图"
            onclick={() => { viewMode = 'grid'; persistViewPrefs(); }}
          >
            {@html icons.grid}
          </button>
          <button
            type="button"
            class="view-btn"
            class:active={viewMode === 'list'}
            title="紧凑列表视图"
            aria-label="紧凑列表视图"
            onclick={() => { viewMode = 'list'; persistViewPrefs(); }}
          >
            {@html icons.list}
          </button>
          <button
            type="button"
            class="view-btn"
            class:active={grouped}
            title={grouped ? '取消按来源分组' : '按来源分组'}
            aria-label="按来源分组"
            aria-pressed={grouped}
            onclick={() => { grouped = !grouped; persistViewPrefs(); }}
          >
            {@html icons.groupTree}
          </button>
        </div>

        <button
          type="button"
          class="toolbar-btn"
          class:active={multiSelect}
          onclick={() => {
            if (multiSelect) exitMultiSelect();
            else multiSelect = true;
          }}
        >
          <span class="btn-svg" aria-hidden="true">{@html icons.checkSquare}</span>
          <span>多选</span>
        </button>
      </div>
    </div>

    <!-- Toolbar Row 2: Source, AI status & Provider Filter Chips -->
    <div class="filter-chips-row">
      <div class="chips-group">
        {#each [
          { id: 'local', label: '本地' },
          { id: 'import', label: '导入' },
          { id: 'git', label: 'Git' },
          { id: 'skills.sh', label: 'skills.sh' },
          { id: 'builtin', label: '内置' },
        ] as chip (chip.id)}
          <button
            type="button"
            class="filter-chip"
            class:active={filter.source === chip.id}
            onclick={() => toggleSourceFilter(chip.id as Exclude<SkillSourceFilter, 'all'>)}
          >
            {chip.label}
          </button>
        {/each}

        <span class="chips-divider"></span>

        <button
          type="button"
          class="filter-chip ai-chip"
          class:active={filter.ai === 'analyzed'}
          onclick={() => toggleAiFilter('analyzed')}
        >
          已分析
        </button>
        <button
          type="button"
          class="filter-chip ai-chip"
          class:active={filter.ai === 'unanalyzed'}
          onclick={() => toggleAiFilter('unanalyzed')}
        >
          未分析 ({unanalyzedCount})
        </button>
        {#if batchProgress.failed > 0}
          <button
            type="button"
            class="filter-chip danger-chip"
            class:active={filter.ai === 'failed'}
            onclick={() => toggleAiFilter('failed')}
          >
            分析失败 ({batchProgress.failed})
          </button>
        {/if}
      </div>

      <div class="chips-group provider-filters">
        {#each ALL_PROVIDERS as p (p.id)}
          <button
            type="button"
            class="provider-filter-chip"
            class:active={filter.provider === p.id}
            style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}
            title={`仅显示 ${p.label} 中的技能`}
            onclick={() => setProviderFilter(p.id)}
          >
            <span class="pf-icon">{@html p.icon}</span>
            <span>{p.shortLabel}</span>
          </button>
        {/each}
      </div>
    </div>

    {#if multiSelect}
      <div class="bulk-bar">
        <div class="bulk-left">
          <strong>已选 {selectedKeys.size} 项</strong>
          <button type="button" class="bulk-link" onclick={selectAllFiltered}>全选当前 ({filteredItems.length})</button>
          <button type="button" class="bulk-link" onclick={selectUnanalyzedFiltered}>仅选未分析</button>
          {#if selectedKeys.size > 0}
            <button type="button" class="bulk-link" onclick={() => (selectedKeys = new Set())}>清空</button>
          {/if}
        </div>
        <div class="bulk-actions">
          <button
            type="button"
            class="btn-primary btn-xs"
            disabled={selectedKeys.size === 0 || batchProgress.running}
            onclick={() => openBatchAnalysis('selected')}
          >
            批量 AI 分析已选
          </button>
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={selectedKeys.size === 0 || busy}
            onclick={() => void bulkToggleEnabled(true)}
          >
            批量启用
          </button>
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={selectedKeys.size === 0 || busy}
            onclick={() => void bulkToggleEnabled(false)}
          >
            批量停用
          </button>
          <button type="button" class="btn-secondary btn-xs" onclick={exitMultiSelect}>退出多选</button>
        </div>
      </div>
    {/if}

    {#if showBatchPanel || batchProgress.running || batchProgress.failed > 0}
      <SkillBatchPanel
        bind:mode={batchMode}
        prefs={batchPrefs}
        progress={batchProgress}
        {unanalyzedCount}
        analyzableCount={analyzableItems.length}
        selectedCount={selectedKeys.size}
        targetCount={batchTargetCount}
        {hasAiConfig}
        onUpdatePrefs={handleUpdateBatchPrefs}
        onStart={() => void startBatchAnalysis()}
        onStop={() => batchController.stop()}
        onRetryFailed={() => void retryFailedBatch()}
        onRetrySingle={retrySingleByKey}
        {onOpenAiConfig}
        onClose={() => {
          showBatchPanel = false;
          if (!batchProgress.running) batchController.clear();
        }}
      />
    {/if}

    {#if error}<p class="msg error" role="alert">{error}</p>{/if}
    {#if notice}<p class="msg success" role="status">{notice}</p>{/if}

    {#if loading && items.length === 0}
      <div class="skills-grid">
        {#each Array(6) as _}
          <div class="skeleton-skill-card">
            <div class="sk-line w-45"></div>
            <div class="sk-line w-90"></div>
            <div class="sk-line w-65"></div>
          </div>
        {/each}
      </div>
    {:else if filteredItems.length === 0}
      <div class="empty-library">
        <div class="empty-icon">{@html icons.layers}</div>
        <p>{items.length === 0 ? '尚未发现本机 Skill，可点击「新建」或「导入」添加。' : '没有符合当前筛选条件的 Skill。'}</p>
      </div>
    {:else if grouped}
      <div class="groups-container">
        {#each groupedItems as grp (grp.group)}
          {@const collapsed = Boolean(collapsedGroups[grp.group])}
          <div class="source-group-section">
            <button
              type="button"
              class="group-header-btn"
              aria-expanded={!collapsed}
              onclick={() => toggleGroupCollapsed(grp.group)}
            >
              <span class="group-chevron" class:open={!collapsed} aria-hidden="true">{@html icons.chevronRight}</span>
              <span class="group-name">{grp.group}</span>
              <span class="group-count">{grp.items.length}</span>
            </button>

            {#if !collapsed}
              {#if viewMode === 'grid'}
                <div class="skills-grid">
                  {#each grp.items as item (item.key)}
                    <SkillCard
                      {item}
                      batchState={batchProgress.items[item.key]}
                      {multiSelect}
                      selected={selectedKeys.has(item.key)}
                      {busy}
                      onOpen={(target) => { activeItemKey = target.key; }}
                      onToggleEnabled={(target) => void toggleEnabled(target)}
                      onToggleSelect={toggleSelectKey}
                      onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
                    />
                  {/each}
                </div>
              {:else}
                <div class="skills-list">
                  {#each grp.items as item (item.key)}
                    <SkillListRow
                      {item}
                      batchState={batchProgress.items[item.key]}
                      {multiSelect}
                      selected={selectedKeys.has(item.key)}
                      {busy}
                      onOpen={(target) => { activeItemKey = target.key; }}
                      onToggleEnabled={(target) => void toggleEnabled(target)}
                      onToggleSelect={toggleSelectKey}
                      onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
                    />
                  {/each}
                </div>
              {/if}
            {/if}
          </div>
        {/each}
      </div>
    {:else if viewMode === 'grid'}
      <div class="skills-grid">
        {#each filteredItems as item (item.key)}
          <SkillCard
            {item}
            batchState={batchProgress.items[item.key]}
            {multiSelect}
            selected={selectedKeys.has(item.key)}
            {busy}
            onOpen={(target) => { activeItemKey = target.key; }}
            onToggleEnabled={(target) => void toggleEnabled(target)}
            onToggleSelect={toggleSelectKey}
            onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
          />
        {/each}
      </div>
    {:else}
      <div class="skills-list">
        {#each filteredItems as item (item.key)}
          <SkillListRow
            {item}
            batchState={batchProgress.items[item.key]}
            {multiSelect}
            selected={selectedKeys.has(item.key)}
            {busy}
            onOpen={(target) => { activeItemKey = target.key; }}
            onToggleEnabled={(target) => void toggleEnabled(target)}
            onToggleSelect={toggleSelectKey}
            onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
          />
        {/each}
      </div>
    {/if}
  </div>
{/if}

<style>
  .library-shell {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }

  .toolbar-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
  }

  .toolbar-left,
  .toolbar-right {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .search-box {
    position: relative;
    width: clamp(190px, 24vw, 270px);
  }

  .search-input {
    width: 100%;
    height: 32px;
    padding: 0 28px 0 30px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    color: var(--text-primary);
    font: inherit;
    font-size: 12px;
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .search-input:focus {
    outline: none;
    border-color: var(--brand-primary);
    box-shadow: 0 0 0 2px var(--brand-subtle);
  }

  .search-input::-webkit-search-cancel-button {
    appearance: none;
  }

  .search-icon {
    position: absolute;
    left: 9px;
    top: 50%;
    transform: translateY(-50%);
    width: 14px;
    height: 14px;
    display: inline-flex;
    color: var(--text-muted);
    pointer-events: none;
  }

  .search-clear {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 18px;
    padding: 3px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .search-icon :global(svg),
  .search-clear :global(svg),
  .btn-svg :global(svg),
  .view-btn :global(svg),
  .pf-icon :global(svg),
  .group-chevron :global(svg),
  .empty-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .segmented {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
  }

  .seg-btn {
    height: 26px;
    padding: 0 10px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .seg-btn:hover {
    color: var(--text-primary);
  }

  .seg-btn.active {
    background: var(--bg-secondary);
    color: var(--text-primary);
    font-weight: 600;
  }

  .toolbar-btn {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 32px;
    padding: 0 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .toolbar-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .toolbar-btn.active {
    color: var(--brand-text);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .ai-batch-btn {
    color: var(--brand-text);
    border-color: var(--brand-border);
    background: color-mix(in srgb, var(--brand-subtle) 70%, var(--bg-card));
  }

  .ai-batch-btn:hover:not(:disabled) {
    background: var(--brand-subtle);
    color: var(--brand-strong);
  }

  .count-pill {
    padding: 0 5px;
    min-width: 18px;
    height: 16px;
    border-radius: 999px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: var(--brand-primary);
    color: #fff;
    font-size: 10px;
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }

  .toolbar-icon-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .toolbar-icon-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .btn-svg {
    display: inline-flex;
    width: 13px;
    height: 13px;
  }

  .btn-svg.spin {
    animation: spin 0.9s linear infinite;
  }

  .view-switcher {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
  }

  .view-btn {
    width: 26px;
    height: 26px;
    padding: 5px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    transition: all var(--transition-fast);
  }

  .view-btn:hover {
    color: var(--text-primary);
  }

  .view-btn.active {
    background: var(--bg-secondary);
    color: var(--brand-text);
  }

  .filter-chips-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px 14px;
    flex-wrap: wrap;
  }

  .chips-group {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-wrap: wrap;
  }

  .chips-divider {
    width: 1px;
    height: 14px;
    background: var(--border-subtle);
    margin: 0 3px;
  }

  .filter-chip {
    height: 24px;
    padding: 0 9px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .filter-chip:hover {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .filter-chip.active {
    background: var(--brand-subtle);
    border-color: var(--brand-border);
    color: var(--brand-text);
    font-weight: 600;
  }

  .filter-chip.danger-chip {
    color: var(--color-danger);
    border-color: var(--color-danger-border);
    background: var(--color-danger-bg);
  }

  .provider-filter-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px 0 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .provider-filter-chip:hover {
    border-color: var(--border-default);
    color: var(--text-primary);
  }

  .provider-filter-chip.active {
    border-color: var(--p-color);
    background: var(--p-bg);
    color: var(--text-primary);
    font-weight: 600;
  }

  .pf-icon {
    display: inline-flex;
    width: 12px;
    height: 12px;
    color: var(--p-color);
  }

  .bulk-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    padding: 8px 12px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-sm);
    background: var(--brand-subtle);
    font-size: 12px;
  }

  .bulk-left,
  .bulk-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .bulk-link {
    border: none;
    background: transparent;
    color: var(--brand-strong);
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    padding: 0;
  }

  .bulk-link:hover {
    text-decoration: underline;
  }

  .btn-xs {
    height: 25px;
    padding: 0 10px;
    font-size: 11.5px;
  }

  .msg {
    margin: 0;
    padding: 7px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    border: 1px solid var(--border-subtle);
  }

  .msg.error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }

  .msg.success {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
  }

  .skills-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(290px, 1fr));
    gap: 10px;
  }

  .skills-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .groups-container {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .source-group-section {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .group-header-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    align-self: flex-start;
    padding: 3px 6px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-primary);
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }

  .group-header-btn:hover {
    background: var(--bg-secondary);
  }

  .group-chevron {
    display: inline-flex;
    width: 13px;
    height: 13px;
    color: var(--text-muted);
    transition: transform 140ms ease;
  }

  .group-chevron.open {
    transform: rotate(90deg);
  }

  .group-count {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  .skeleton-skill-card {
    height: 124px;
    padding: 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .sk-line {
    height: 12px;
    border-radius: 6px;
    background: var(--bg-secondary);
  }

  .sk-line.w-45 { width: 45%; height: 14px; }
  .sk-line.w-90 { width: 90%; }
  .sk-line.w-65 { width: 65%; margin-top: auto; }

  .empty-library {
    padding: 48px 20px;
    border: 1px dashed var(--border-default);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 10px;
    color: var(--text-muted);
    font-size: 12.5px;
  }

  .empty-library p {
    margin: 0;
  }

  .empty-icon {
    width: 32px;
    height: 32px;
    opacity: 0.6;
  }

  .overlay-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    overflow: hidden;
  }

  .overlay-provider-bar {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-elevated);
  }

  .overlay-label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .overlay-provider-pills {
    display: flex;
    gap: 4px;
    flex-wrap: wrap;
  }

  .overlay-provider-btn {
    height: 26px;
    padding: 0 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
  }

  .overlay-provider-btn.selected {
    background: var(--brand-subtle);
    border-color: var(--brand-border);
    color: var(--brand-text);
    font-weight: 600;
  }
</style>
