<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { icons } from './icons';
  import { ALL_PROVIDERS } from './providerBadges';
  import type { Id, Provider } from './agentCustomizationEntries';
  import type { SkillSummary, UpdateInfo } from './agentSkillSourceTypes';
  import {
    filterSkillItems,
    groupSkillItemsBySource,
    isItemEnabledInScope,
    isSkillAnalyzable,
    type SkillAiFilter,
    type SkillFilterState,
    type SkillLibraryItem,
    type SkillSourceFilter,
    type SkillStatusFilter,
    type SkillSyncMode,
    type SkillViewMode,
  } from './skillLibraryTypes';
  import {
    getProviderConfiguredSyncMode,
    loadProviderSyncPrefs,
    setProviderConfiguredSyncMode,
    syncModeLabel,
    type ProviderSyncPrefs,
  } from './skillSyncPrefs';
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
  import {
    createSkillBatchUpdater,
    emptyBatchUpdateProgress,
    type BatchUpdateProgress,
  } from './skillBatchUpdater';
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
      { id: 'codex-skills', label: 'Codex skills' },
      { id: 'shared-skills', label: '共享 skills' },
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
    showBuiltin: false,
  };

  let providerSyncPrefs: ProviderSyncPrefs = loadProviderSyncPrefs();
  let showSyncSettings = false;

  let multiSelect = false;
  let selectedKeys = new Set<string>();
  let bulkTargetProviders = new Set<Provider>(ALL_PROVIDERS.map((p) => p.id));
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

  // Batch Update Checker & Updater state
  let updateProgress: BatchUpdateProgress = emptyBatchUpdateProgress();

  const batchController = createSkillBatchController({
    analyze: (item) => invoke<SkillSummary>('generate_skill_summary', { id: item.primary_id }),
    onItemSuccess: (item, summary) => {
      items = items.map((entry) => (entry.key === item.key ? { ...entry, summary } : entry));
    },
    onChange: (next) => {
      batchProgress = next;
    },
  });

  const batchUpdater = createSkillBatchUpdater({
    checkOne: (item) => invoke<UpdateInfo>('check_skill_update', { id: item.primary_id }),
    updateOne: (item) => invoke<string>('update_skill_source', { id: item.primary_id }),
    onChange: (next) => {
      updateProgress = next;
    },
    onFinishedUpdate: async () => {
      await refresh();
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

  $: userItems = items.filter((i) => !i.builtin);
  $: builtinCount = items.length - userItems.length;
  $: totalCount = filter.showBuiltin ? items.length : userItems.length;
  $: dirty = detailDirty;
  $: failedKeySet = new Set(batchProgress.failedList.map((f) => f.key));
  $: filteredItems = filterSkillItems(items, filter, failedKeySet);
  $: groupedItems = grouped ? groupSkillItemsBySource(filteredItems) : [];
  $: activeItem = activeItemKey ? items.find((i) => i.key === activeItemKey) ?? null : null;
  $: analyzableItems = (filter.showBuiltin ? items : userItems).filter(isSkillAnalyzable);
  $: unanalyzedCount = analyzableItems.filter((i) => i.summary === null).length;
  $: batchTargetCount = selectBatchTargets(
    filter.showBuiltin ? items : userItems,
    batchMode,
    selectedKeys,
    batchPrefs.skipAnalyzedOnSelected
  ).length;
  $: providerRootOptions = providerRoots[targetProvider].map((r) => ({ key: r.id, label: r.label }));
  $: activeProviderLabel =
    filter.provider === 'all'
      ? ''
      : ALL_PROVIDERS.find((p) => p.id === filter.provider)?.label ?? filter.provider;

  async function refresh() {
    if (!runtimeAvailable) return;
    loading = true;
    error = '';
    try {
      items = await invoke<SkillLibraryItem[]>('list_skill_library');
      if (activeItemKey && !items.some((i) => i.key === activeItemKey)) {
        activeItemKey = null;
      }
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

  async function handleChangeProviderSyncMode(targetProviderId: Provider, mode: SkillSyncMode) {
    providerSyncPrefs = setProviderConfiguredSyncMode(providerSyncPrefs, targetProviderId, mode);
    const label = ALL_PROVIDERS.find((p) => p.id === targetProviderId)?.label ?? targetProviderId;
    busy = true;
    error = '';
    notice = '';
    try {
      const converted = await invoke<number>('apply_provider_sync_mode', {
        targetProvider: targetProviderId,
        syncMode: mode,
      });
      await refresh();
      notice =
        converted > 0
          ? `已将 ${label} 设为「${syncModeLabel(mode)}」，并转换了 ${converted} 个已启用技能。`
          : `已将 ${label} 的同步方式设为「${syncModeLabel(mode)}」。`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleProviderEnabled(item: SkillLibraryItem, targetProviderId: Provider, enabled: boolean) {
    if (busy || item.builtin) return;
    const syncMode = getProviderConfiguredSyncMode(providerSyncPrefs, targetProviderId);
    busy = true;
    error = '';
    notice = '';
    try {
      await invoke('set_skill_provider_enabled', {
        sourceId: item.primary_id,
        targetProvider: targetProviderId,
        enabled,
        syncMode,
      });
      await refresh();
      const label = ALL_PROVIDERS.find((p) => p.id === targetProviderId)?.label ?? targetProviderId;
      notice = enabled
        ? `已在 ${label} 启用 ${item.name}（${syncModeLabel(syncMode)}）`
        : `已在 ${label} 禁用 ${item.name}`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleCardEnabled(item: SkillLibraryItem) {
    if (busy || item.builtin) return;
    if (filter.provider !== 'all') {
      const currentlyOn = isItemEnabledInScope(item, filter.provider);
      await toggleProviderEnabled(item, filter.provider, !currentlyOn);
      return;
    }

    const nextEnabled = !item.enabled;
    busy = true;
    error = '';
    notice = '';
    try {
      await invoke('set_skill_all_enabled', {
        sourceId: item.primary_id,
        enabled: nextEnabled,
        syncMode: 'link',
        targetProviders: ALL_PROVIDERS.map((p) => p.id),
        providerSyncModes: providerSyncPrefs,
      });
      await refresh();
      notice = nextEnabled ? `已启用 ${item.name}` : `已禁用 ${item.name}`;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function toggleBulkTargetProvider(p: Provider) {
    const next = new Set(bulkTargetProviders);
    if (next.has(p)) next.delete(p);
    else next.add(p);
    bulkTargetProviders = next;
  }

  async function bulkApplyProviders(enable: boolean) {
    if (busy || selectedKeys.size === 0) return;
    const targetProviders =
      filter.provider !== 'all'
        ? [filter.provider]
        : Array.from(bulkTargetProviders);
    if (targetProviders.length === 0) {
      error = '请至少选择一个目标应用工具。';
      return;
    }
    const targets = items.filter((i) => selectedKeys.has(i.key) && !i.builtin);
    if (targets.length === 0) return;

    busy = true;
    error = '';
    notice = '';
    let okCount = 0;
    const errors: string[] = [];
    try {
      for (const item of targets) {
        for (const tp of targetProviders) {
          const syncMode = getProviderConfiguredSyncMode(providerSyncPrefs, tp);
          try {
            await invoke('set_skill_provider_enabled', {
              sourceId: item.primary_id,
              targetProvider: tp,
              enabled: enable,
              syncMode,
            });
            okCount += 1;
          } catch (e) {
            errors.push(`${item.name}/${tp}: ${String(e)}`);
          }
        }
      }
      await refresh();
      notice = enable
        ? `已批量启用 ${targets.length} 个技能到 ${targetProviders.length} 个应用（${okCount} 项成功）。`
        : `已批量在 ${targetProviders.length} 个应用中禁用 ${targets.length} 个技能。`;
      if (errors.length > 0) {
        error = `部分操作失败：${errors.slice(0, 2).join('；')}`;
      }
    } finally {
      busy = false;
    }
  }

  async function handleCheckAllUpdates() {
    error = '';
    notice = '';
    const scope = multiSelect && selectedKeys.size > 0
      ? userItems.filter((i) => selectedKeys.has(i.key))
      : userItems;
    await batchUpdater.checkBatch(scope, 3);
    if (updateProgress.availableCount > 0) {
      notice = `检查完成：发现 ${updateProgress.availableCount} 个技能有新版本可更新。`;
    } else if (updateProgress.errorCount > 0) {
      notice = `检查完成：暂无可更新项（${updateProgress.errorCount} 项检查出错）。`;
    } else {
      notice = '检查完成：所有已纳管技能均已是最新版本。';
    }
  }

  async function handleUpdateAllAvailable() {
    error = '';
    notice = '';
    const onlyKeys = multiSelect && selectedKeys.size > 0 ? selectedKeys : undefined;
    await batchUpdater.updateAvailable(userItems, 2, onlyKeys);
    if (updateProgress.updatedCount > 0) {
      notice = `更新完成：已成功更新 ${updateProgress.updatedCount} 个技能。`;
    }
    if (updateProgress.errorCount > 0) {
      error = `有 ${updateProgress.errorCount} 个技能更新失败，请查看详情或重试。`;
    }
  }

  async function handleUpdateSingle(item: SkillLibraryItem) {
    error = '';
    notice = '';
    await batchUpdater.updateSingle(item);
    const st = updateProgress.items[item.key];
    if (st?.status === 'updated') {
      notice = `${item.name}：${st.detail}`;
    } else if (st?.status === 'error') {
      error = `${item.name} 更新失败：${st.detail}`;
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
    await batchController.start(filter.showBuiltin ? items : userItems, {
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

  function toggleShowBuiltin() {
    const next = !filter.showBuiltin;
    filter = {
      ...filter,
      showBuiltin: next,
      source: !next && filter.source === 'builtin' ? 'all' : filter.source,
    };
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
    batchUpdater.stop();
  });
</script>

{#if activeItem}
  <SkillDetailView
    item={activeItem}
    batchState={batchProgress.items[activeItem.key]}
    {providerSyncPrefs}
    {hasAiConfig}
    bind:dirty={detailDirty}
    onClose={() => { activeItemKey = null; detailDirty = false; }}
    onRefreshLibrary={refresh}
    onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
    onToggleEnabled={toggleCardEnabled}
    onToggleProvider={toggleProviderEnabled}
    onChangeProviderSyncMode={handleChangeProviderSyncMode}
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
            {activeProviderLabel ? `${activeProviderLabel} 已启用` : '已启用'}
          </button>
          <button
            type="button"
            class="seg-btn"
            class:active={filter.status === 'disabled'}
            onclick={() => setStatusFilter('disabled')}
          >
            {activeProviderLabel ? `${activeProviderLabel} 未启用` : '未启用'}
          </button>
        </div>
      </div>

      <div class="toolbar-right">
        <button
          type="button"
          class="toolbar-btn"
          disabled={updateProgress.phase !== 'idle' || userItems.length === 0}
          onclick={() => void handleCheckAllUpdates()}
          title="批量检查所有技能是否有远程更新"
        >
          <span class="btn-svg" class:spin={updateProgress.phase === 'checking'} aria-hidden="true">{@html icons.refresh}</span>
          <span>
            {updateProgress.phase === 'checking'
              ? `检查中 ${updateProgress.completed}/${updateProgress.total}`
              : '检查全部'}
          </span>
        </button>

        <button
          type="button"
          class="toolbar-btn update-all-btn"
          class:has-updates={updateProgress.availableCount > 0}
          disabled={updateProgress.phase !== 'idle' || updateProgress.availableCount === 0}
          onclick={() => void handleUpdateAllAvailable()}
          title="批量更新所有检测到新版本的技能"
        >
          <span>
            {updateProgress.phase === 'updating'
              ? `更新中 ${updateProgress.completed}/${updateProgress.total}`
              : `全部更新 (${updateProgress.availableCount})`}
          </span>
        </button>

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

    <!-- Toolbar Row 2: Source, AI status, Global Sync Mode & App Scope Selector -->
    <div class="filter-chips-row">
      <div class="chips-group">
        {#each [
          { id: 'local', label: '本地' },
          { id: 'import', label: '导入' },
          { id: 'git', label: 'Git' },
          { id: 'skills.sh', label: 'skills.sh' },
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
          class="filter-chip"
          class:active={filter.ai === 'analyzed'}
          onclick={() => toggleAiFilter('analyzed')}
        >
          已分析
        </button>
        <button
          type="button"
          class="filter-chip"
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

        {#if builtinCount > 0}
          <span class="chips-divider"></span>
          <button
            type="button"
            class="filter-chip"
            class:active={filter.showBuiltin}
            title="默认隐藏各工具自带的内置技能，点击可展开查看"
            onclick={toggleShowBuiltin}
          >
            显示内置 ({builtinCount})
          </button>
        {/if}
      </div>

      <div class="chips-group right-controls">
        <button
          type="button"
          class="filter-chip"
          class:active={showSyncSettings}
          onclick={() => (showSyncSettings = !showSyncSettings)}
          title="为每个应用单独设置静态链接或文件复制"
        >
          同步方式
        </button>

        <span class="chips-divider"></span>

        <div class="app-scope-group" role="group" aria-label="按应用单独管理启停">
          <button
            type="button"
            class="provider-filter-chip"
            class:active={filter.provider === 'all'}
            onclick={() => (filter = { ...filter, provider: 'all' })}
          >
            全部应用
          </button>
          {#each ALL_PROVIDERS as p (p.id)}
            <button
              type="button"
              class="provider-filter-chip"
              class:active={filter.provider === p.id}
              style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}
              title={`切换到 ${p.label} 视图（当前：${syncModeLabel(getProviderConfiguredSyncMode(providerSyncPrefs, p.id))}）`}
              onclick={() => setProviderFilter(p.id)}
            >
              <span class="pf-icon">{@html p.icon}</span>
              <span>{p.shortLabel}</span>
            </button>
          {/each}
        </div>
      </div>
    </div>

    {#if showSyncSettings}
      <div class="provider-sync-panel" role="region" aria-label="各应用同步方式设置">
        <div class="psp-head">
          <strong>各应用同步方式</strong>
          <button type="button" class="scope-clear-btn" onclick={() => (showSyncSettings = false)}>收起</button>
        </div>
        <div class="psp-grid">
          {#each ALL_PROVIDERS as p (p.id)}
            {@const mode = getProviderConfiguredSyncMode(providerSyncPrefs, p.id)}
            <div class="psp-item">
              <div class="psp-app">
                <span class="pf-icon" style={`--p-color: ${p.color}`}>{@html p.icon}</span>
                <span>{p.label}</span>
              </div>
              <div class="global-sync-toggle">
                <button
                  type="button"
                  class="sync-opt"
                  class:active={mode === 'link'}
                  disabled={busy}
                  onclick={() => void handleChangeProviderSyncMode(p.id, 'link')}
                >
                  静态链接
                </button>
                <button
                  type="button"
                  class="sync-opt"
                  class:active={mode === 'copy'}
                  disabled={busy}
                  onclick={() => void handleChangeProviderSyncMode(p.id, 'copy')}
                >
                  文件复制
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    {#if filter.provider !== 'all'}
      {@const scopedProvider = filter.provider}
      {@const scopedMode = getProviderConfiguredSyncMode(providerSyncPrefs, scopedProvider)}
      <div class="scope-hint-bar">
        <span>当前按 <strong>{activeProviderLabel}</strong> 管理：卡片开关直接控制该应用的启停。</span>
        <div class="scope-right">
          <div class="global-sync-toggle" title={`设置 ${activeProviderLabel} 的同步方式`}>
            <span class="sync-label">{activeProviderLabel} 同步</span>
            <button
              type="button"
              class="sync-opt"
              class:active={scopedMode === 'link'}
              disabled={busy}
              onclick={() => void handleChangeProviderSyncMode(scopedProvider, 'link')}
            >
              静态链接
            </button>
            <button
              type="button"
              class="sync-opt"
              class:active={scopedMode === 'copy'}
              disabled={busy}
              onclick={() => void handleChangeProviderSyncMode(scopedProvider, 'copy')}
            >
              文件复制
            </button>
          </div>
          <button type="button" class="scope-clear-btn" onclick={() => (filter = { ...filter, provider: 'all' })}>
            返回全部应用
          </button>
        </div>
      </div>
    {/if}

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
          {#if filter.provider === 'all'}
            <div class="bulk-app-picker" title="选择批量启用或禁用的目标应用">
              {#each ALL_PROVIDERS as p (p.id)}
                <button
                  type="button"
                  class="bulk-app-chip"
                  class:active={bulkTargetProviders.has(p.id)}
                  style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}
                  onclick={() => toggleBulkTargetProvider(p.id)}
                >
                  <span class="pf-icon">{@html p.icon}</span>
                  <span>{p.shortLabel}</span>
                </button>
              {/each}
            </div>
          {/if}

          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={selectedKeys.size === 0 || busy}
            onclick={() => void bulkApplyProviders(true)}
          >
            {activeProviderLabel ? `启用到 ${activeProviderLabel}` : '批量启用'}
          </button>
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={selectedKeys.size === 0 || busy}
            onclick={() => void bulkApplyProviders(false)}
          >
            {activeProviderLabel ? `在 ${activeProviderLabel} 禁用` : '批量禁用'}
          </button>
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={selectedKeys.size === 0 || updateProgress.phase !== 'idle'}
            onclick={() => void handleCheckAllUpdates()}
          >
            检查更新
          </button>
          <button
            type="button"
            class="btn-primary btn-xs"
            disabled={selectedKeys.size === 0 || batchProgress.running}
            onclick={() => openBatchAnalysis('selected')}
          >
            批量 AI 分析
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

    {#if loading && items.length > 0}
      <div class="skill-loading-bar" role="status" aria-label="正在刷新技能库"></div>
    {/if}

    {#if loading && items.length === 0}
      <div class="skill-loading-shell" role="status" aria-live="polite" aria-label="正在加载技能库">
        <div class="skill-loading-bar"></div>
        {#if grouped}
          <div class="groups-container">
            {#each [{ w: '140px', count: 3 }, { w: '172px', count: 3 }] as grpSk, gIdx}
              <div class="source-group-section">
                <div class="sk-group-head" style={`--delay: ${gIdx * 90}ms`}>
                  <span class="sk-bone sk-dot-sm"></span>
                  <span class="sk-bone sk-group-title" style={`width: ${grpSk.w}`}></span>
                  <span class="sk-bone sk-count-pill"></span>
                </div>
                {#if viewMode === 'grid'}
                  <div class="skills-grid">
                    {#each Array(grpSk.count) as _, cIdx}
                      {@const idx = gIdx * 3 + cIdx}
                      <div class="skeleton-skill-card" style={`--delay: ${idx * 55}ms`}>
                        <div class="sk-card-head">
                          <span class="sk-bone sk-status-dot"></span>
                          <span class="sk-bone sk-card-title" style={`width: ${['48%', '62%', '42%', '56%', '50%', '44%'][idx % 6]}`}></span>
                          <span class="sk-bone sk-switch-pill"></span>
                        </div>
                        <div class="sk-card-body">
                          <span class="sk-bone sk-desc-line" style={`width: ${['92%', '84%', '96%', '88%', '90%', '82%'][idx % 6]}`}></span>
                          <span class="sk-bone sk-desc-line" style={`width: ${['68%', '58%', '74%', '62%', '66%', '54%'][idx % 6]}`}></span>
                        </div>
                        <div class="sk-card-foot">
                          <span class="sk-bone sk-source-pill"></span>
                          <div class="sk-provider-row">
                            {#each Array(5) as _, pIdx}
                              <span class="sk-bone sk-provider-dot" style={`--delay: ${idx * 55 + pIdx * 35}ms`}></span>
                            {/each}
                          </div>
                        </div>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="skills-list">
                    {#each Array(grpSk.count) as _, cIdx}
                      {@const idx = gIdx * 3 + cIdx}
                      <div class="skeleton-skill-row" style={`--delay: ${idx * 50}ms`}>
                        <span class="sk-bone sk-status-dot"></span>
                        <span class="sk-bone sk-row-title" style={`width: ${['128px', '156px', '112px', '144px', '132px', '120px'][idx % 6]}`}></span>
                        <span class="sk-bone sk-row-desc" style={`width: ${['42%', '36%', '46%', '38%', '44%', '34%'][idx % 6]}`}></span>
                        <div class="sk-provider-row sk-row-right">
                          {#each Array(5) as _, pIdx}
                            <span class="sk-bone sk-provider-dot" style={`--delay: ${idx * 50 + pIdx * 30}ms`}></span>
                          {/each}
                          <span class="sk-bone sk-source-pill"></span>
                          <span class="sk-bone sk-switch-pill"></span>
                        </div>
                      </div>
                    {/each}
                  </div>
                {/if}
              </div>
            {/each}
          </div>
        {:else if viewMode === 'grid'}
          <div class="skills-grid">
            {#each Array(9) as _, idx}
              <div class="skeleton-skill-card" style={`--delay: ${idx * 50}ms`}>
                <div class="sk-card-head">
                  <span class="sk-bone sk-status-dot"></span>
                  <span class="sk-bone sk-card-title" style={`width: ${['46%', '60%', '40%', '54%', '48%', '64%', '44%', '58%', '50%'][idx]}`}></span>
                  <span class="sk-bone sk-switch-pill"></span>
                </div>
                <div class="sk-card-body">
                  <span class="sk-bone sk-desc-line" style={`width: ${['94%', '86%', '92%', '88%', '96%', '84%', '90%', '86%', '92%'][idx]}`}></span>
                  <span class="sk-bone sk-desc-line" style={`width: ${['64%', '56%', '72%', '60%', '68%', '52%', '66%', '58%', '62%'][idx]}`}></span>
                </div>
                <div class="sk-card-foot">
                  <span class="sk-bone sk-source-pill"></span>
                  <div class="sk-provider-row">
                    {#each Array(5) as _, pIdx}
                      <span class="sk-bone sk-provider-dot" style={`--delay: ${idx * 50 + pIdx * 30}ms`}></span>
                    {/each}
                  </div>
                </div>
              </div>
            {/each}
          </div>
        {:else}
          <div class="skills-list">
            {#each Array(8) as _, idx}
              <div class="skeleton-skill-row" style={`--delay: ${idx * 45}ms`}>
                <span class="sk-bone sk-status-dot"></span>
                <span class="sk-bone sk-row-title" style={`width: ${['136px', '160px', '118px', '148px', '128px', '152px', '124px', '142px'][idx]}`}></span>
                <span class="sk-bone sk-row-desc" style={`width: ${['44%', '36%', '48%', '40%', '46%', '34%', '42%', '38%'][idx]}`}></span>
                <div class="sk-provider-row sk-row-right">
                  {#each Array(5) as _, pIdx}
                    <span class="sk-bone sk-provider-dot" style={`--delay: ${idx * 45 + pIdx * 28}ms`}></span>
                  {/each}
                  <span class="sk-bone sk-source-pill"></span>
                  <span class="sk-bone sk-switch-pill"></span>
                </div>
              </div>
            {/each}
          </div>
        {/if}
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
                      providerScope={filter.provider}
                      {providerSyncPrefs}
                      batchState={batchProgress.items[item.key]}
                      updateState={updateProgress.items[item.key]}
                      {multiSelect}
                      selected={selectedKeys.has(item.key)}
                      {busy}
                      onOpen={(target) => { activeItemKey = target.key; }}
                      onToggleEnabled={(target) => void toggleCardEnabled(target)}
                      onToggleProvider={(target, p, nextOn) => void toggleProviderEnabled(target, p, nextOn)}
                      onToggleSelect={toggleSelectKey}
                      onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
                      onUpdateSingle={(target) => void handleUpdateSingle(target)}
                    />
                  {/each}
                </div>
              {:else}
                <div class="skills-list">
                  {#each grp.items as item (item.key)}
                    <SkillListRow
                      {item}
                      providerScope={filter.provider}
                      {providerSyncPrefs}
                      batchState={batchProgress.items[item.key]}
                      updateState={updateProgress.items[item.key]}
                      {multiSelect}
                      selected={selectedKeys.has(item.key)}
                      {busy}
                      onOpen={(target) => { activeItemKey = target.key; }}
                      onToggleEnabled={(target) => void toggleCardEnabled(target)}
                      onToggleProvider={(target, p, nextOn) => void toggleProviderEnabled(target, p, nextOn)}
                      onToggleSelect={toggleSelectKey}
                      onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
                      onUpdateSingle={(target) => void handleUpdateSingle(target)}
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
            providerScope={filter.provider}
            {providerSyncPrefs}
            batchState={batchProgress.items[item.key]}
            updateState={updateProgress.items[item.key]}
            {multiSelect}
            selected={selectedKeys.has(item.key)}
            {busy}
            onOpen={(target) => { activeItemKey = target.key; }}
            onToggleEnabled={(target) => void toggleCardEnabled(target)}
            onToggleProvider={(target, p, nextOn) => void toggleProviderEnabled(target, p, nextOn)}
            onToggleSelect={toggleSelectKey}
            onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
            onUpdateSingle={(target) => void handleUpdateSingle(target)}
          />
        {/each}
      </div>
    {:else}
      <div class="skills-list">
        {#each filteredItems as item (item.key)}
          <SkillListRow
            {item}
            providerScope={filter.provider}
            {providerSyncPrefs}
            batchState={batchProgress.items[item.key]}
            updateState={updateProgress.items[item.key]}
            {multiSelect}
            selected={selectedKeys.has(item.key)}
            {busy}
            onOpen={(target) => { activeItemKey = target.key; }}
            onToggleEnabled={(target) => void toggleCardEnabled(target)}
            onToggleProvider={(target, p, nextOn) => void toggleProviderEnabled(target, p, nextOn)}
            onToggleSelect={toggleSelectKey}
            onAnalyzeSingle={(target) => void analyzeSingleItem(target)}
            onUpdateSingle={(target) => void handleUpdateSingle(target)}
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
    gap: 8px;
    flex-wrap: wrap;
  }

  .toolbar-left,
  .toolbar-right {
    display: flex;
    align-items: center;
    gap: 7px;
    flex-wrap: wrap;
  }

  .search-box {
    position: relative;
    width: clamp(180px, 22vw, 250px);
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
    padding: 0 9px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    cursor: pointer;
    white-space: nowrap;
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
    padding: 0 9px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
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

  .toolbar-btn:disabled {
    opacity: 0.52;
    cursor: not-allowed;
  }

  .toolbar-btn.active {
    color: var(--brand-text);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .update-all-btn.has-updates {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
    font-weight: 600;
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
    min-width: 17px;
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
    gap: 8px 12px;
    flex-wrap: wrap;
  }

  .chips-group,
  .app-scope-group {
    display: flex;
    align-items: center;
    gap: 5px;
    flex-wrap: wrap;
  }

  .chips-divider {
    width: 1px;
    height: 14px;
    background: var(--border-subtle);
    margin: 0 2px;
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

  .global-sync-toggle {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 2px 3px 2px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-card);
  }

  .sync-label {
    font-size: 10.5px;
    color: var(--text-muted);
    margin-right: 3px;
  }

  .sync-opt {
    height: 20px;
    padding: 0 7px;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
  }

  .sync-opt.active {
    background: var(--brand-subtle);
    color: var(--brand-text);
    font-weight: 600;
  }

  .provider-filter-chip,
  .bulk-app-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .provider-filter-chip:hover,
  .bulk-app-chip:hover {
    border-color: var(--border-default);
    color: var(--text-primary);
  }

  .provider-filter-chip.active,
  .bulk-app-chip.active {
    border-color: var(--p-color, var(--brand-border));
    background: var(--p-bg, var(--brand-subtle));
    color: var(--text-primary);
    font-weight: 600;
  }

  .pf-icon {
    display: inline-flex;
    width: 12px;
    height: 12px;
    color: var(--p-color);
  }

  .scope-hint-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    padding: 7px 12px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--brand-subtle) 50%, var(--bg-card));
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .scope-right {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
  }

  .provider-sync-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
  }

  .psp-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 12px;
    color: var(--text-primary);
  }

  .psp-grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(225px, 1fr));
    gap: 8px;
  }

  .psp-item {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-elevated);
  }

  .psp-app {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
  }

  .scope-hint-bar strong {
    color: var(--brand-strong);
  }

  .scope-clear-btn {
    border: none;
    background: transparent;
    color: var(--brand-text);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
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
  .bulk-actions,
  .bulk-app-picker {
    display: flex;
    align-items: center;
    gap: 6px;
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

  .skill-loading-shell {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .skill-loading-bar {
    position: relative;
    height: 2px;
    width: 100%;
    border-radius: 999px;
    background: var(--border-subtle);
    overflow: hidden;
  }

  .skill-loading-bar::after {
    content: '';
    position: absolute;
    inset: 0 auto 0 0;
    width: 36%;
    border-radius: 999px;
    background: linear-gradient(90deg, transparent 0%, var(--brand-primary) 50%, transparent 100%);
    animation: skill-progress-slide 1.25s cubic-bezier(0.4, 0, 0.2, 1) infinite;
  }

  @keyframes skill-progress-slide {
    0% { transform: translateX(-110%); }
    100% { transform: translateX(310%); }
  }

  @keyframes skill-sk-enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .sk-group-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 4px 2px 8px;
    animation: skill-sk-enter 220ms ease both;
    animation-delay: var(--delay, 0ms);
  }

  .sk-dot-sm {
    width: 12px;
    height: 12px;
    border-radius: 4px;
  }

  .sk-group-title {
    height: 13px;
    border-radius: 5px;
  }

  .sk-count-pill {
    width: 24px;
    height: 16px;
    border-radius: 999px;
  }

  .skeleton-skill-card {
    min-height: 124px;
    padding: 13px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    display: flex;
    flex-direction: column;
    gap: 10px;
    animation: skill-sk-enter 240ms ease both;
    animation-delay: var(--delay, 0ms);
  }

  .sk-card-head {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .sk-status-dot {
    width: 8px;
    height: 8px;
    border-radius: 999px;
    flex-shrink: 0;
  }

  .sk-card-title {
    height: 14px;
    border-radius: 5px;
  }

  .sk-switch-pill {
    width: 30px;
    height: 17px;
    border-radius: 999px;
    margin-left: auto;
    flex-shrink: 0;
  }

  .sk-card-body {
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .sk-desc-line {
    height: 11px;
    border-radius: 4px;
  }

  .sk-card-foot {
    margin-top: auto;
    padding-top: 6px;
    border-top: 1px solid var(--border-subtle);
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .sk-source-pill {
    width: 58px;
    height: 16px;
    border-radius: 999px;
    flex-shrink: 0;
  }

  .sk-provider-row {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .sk-provider-dot {
    width: 18px;
    height: 18px;
    border-radius: 5px;
    flex-shrink: 0;
  }

  .skeleton-skill-row {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 8px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    animation: skill-sk-enter 220ms ease both;
    animation-delay: var(--delay, 0ms);
  }

  .sk-row-title {
    height: 13px;
    border-radius: 5px;
    flex-shrink: 0;
  }

  .sk-row-desc {
    height: 11px;
    border-radius: 4px;
  }

  .sk-row-right {
    margin-left: auto;
    gap: 8px;
  }

  @media (prefers-reduced-motion: reduce) {
    .skill-loading-bar::after,
    .sk-group-head,
    .skeleton-skill-card,
    .skeleton-skill-row {
      animation: none;
    }
  }

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
