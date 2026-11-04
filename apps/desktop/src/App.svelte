<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import {
    buildDashboardProjection,
    type ModelSortBy,
    type TrendViewMode,
  } from './lib/dashboardView';
  import {
    getAgentStatus,
    getDashboardSnapshot,
    isTauriRuntime,
    runDiagnostics,
    startAgent,
  } from './lib/api';
  import { toIsoDate, type HeatmapCell } from './lib/heatmap';
  import { createHubController } from './lib/hubController';
  import { icons } from './lib/icons';
  import { createSessionController } from './lib/sessionController';
  import { createSettingsController } from './lib/settingsController';
  import { BOOTSTRAP_STAGE } from './lib/status';
  import type { AgentStatus, DashboardRange, DashboardSnapshot } from './lib/types';
  import DevicesPage from './lib/DevicesPage.svelte';
  import ModelsPage from './lib/ModelsPage.svelte';
  import OverviewPage from './lib/OverviewPage.svelte';
  import ProjectsPage from './lib/ProjectsPage.svelte';
  import QuotaPage from './lib/QuotaPage.svelte';
  import SessionsPage from './lib/SessionsPage.svelte';
  import SettingsPage from './lib/SettingsPage.svelte';
  import ToolsPage from './lib/ToolsPage.svelte';

  type Tab = 'overview' | 'devices' | 'tools' | 'models' | 'sessions' | 'projects' | 'quota' | 'settings';
  let activeTab: Tab = 'overview';
  let range: DashboardRange = 'all';
  let customStart = '';
  let customEnd = '';
  let customOpen = false;
  let todayIso = '';

  let snapshot: DashboardSnapshot | null = null;
  let agent: AgentStatus | null = null;
  let loading = true;
  let refreshing = false;
  let errorMessage = '';
  let runtimeAvailable = isTauriRuntime();
  let agentStarted = false;
  let unlistenDashboard: (() => void) | undefined;

  let remoteSelectedDevice = 'all';

  const sessionController = createSessionController({
    isRuntimeAvailable: () => runtimeAvailable,
    isActive: () => activeTab === 'sessions',
    range: () => range,
    customStart: () => customStart,
    customEnd: () => customEnd,
    refreshDashboard: (showSpinner) => loadDashboard(showSpinner),
  });
  const sessionState = sessionController.state;
  const hubController = createHubController(() => runtimeAvailable);
  const hubState = hubController.state;
  const settingsController = createSettingsController(() => runtimeAvailable);
  const settingsState = settingsController.state;

  // Sort & Trend View States
  let modelSortBy: ModelSortBy = 'tokens';
  let trendViewMode: TrendViewMode = 'curve';
  let hoveredCurveIndex: number | null = null;
  let heatmapAllDaily: DashboardSnapshot['daily'] | null = null;
  let hoveredHeatmapCell: HeatmapCell | null = null;

  async function ensureHeatmapAllDaily() {
    if (!runtimeAvailable) return;
    if (range === 'all' && snapshot?.daily) {
      heatmapAllDaily = snapshot.daily;
      return;
    }
    try {
      const fullSnap = await getDashboardSnapshot('all', 0);
      heatmapAllDaily = fullSnap.daily;
    } catch {
      // Fallback to current snapshot.daily silently
    }
  }

  function handleSwitchTrendView(mode: TrendViewMode) {
    trendViewMode = mode;
    hoveredCurveIndex = null;
    hoveredHeatmapCell = null;
    if (mode === 'heatmap') {
      void ensureHeatmapAllDaily();
    }
  }

  // Navigation Items
  const navItems: { id: Tab; label: string; iconKey: keyof typeof icons }[] = [
    { id: 'overview', label: '使用总览', iconKey: 'dashboard' },
    { id: 'devices', label: '多设备网络', iconKey: 'devices' },
    { id: 'tools', label: '工具分析', iconKey: 'tools' },
    { id: 'models', label: '模型排行', iconKey: 'models' },
    { id: 'projects', label: '项目分析', iconKey: 'projects' },
    { id: 'sessions', label: '会话分析', iconKey: 'sessions' },
    { id: 'quota', label: '额度与活动', iconKey: 'quota' },
    { id: 'settings', label: '系统设置', iconKey: 'settings' },
  ];

  // All dashboard-specific projection rules live behind this pure view module.
  $: dashboardView = buildDashboardProjection({
    snapshot,
    heatmapDaily: heatmapAllDaily,
    todayIso,
    hoveredCurveIndex,
    remoteSelectedDevice,
    modelSortBy,
    range,
    customStart,
    customEnd,
  });

  $: sessionPage = $sessionState.analysis?.page;
  $: sessionPageStart = sessionPage && sessionPage.total > 0 ? sessionPage.offset + 1 : 0;
  $: sessionPageEnd = Math.min((sessionPage?.offset ?? 0) + (sessionPage?.sessions.length ?? 0), sessionPage?.total ?? 0);
  $: sessionFilterOptions = $sessionState.analysis?.filter_options ?? { tools: [], models: [], projects: [] };
  $: hasSessionFilters = Boolean($sessionState.search.trim() || $sessionState.toolFilter || $sessionState.modelFilter || $sessionState.projectFilter);

  $: selectableRemoteDevices = $hubState.devices.filter(
    (d) => d.device_id !== snapshot?.identity?.device_id && d.revoked_at_ms === null
  );

  function snapshotStartIso(ms: number): string {
    return toIsoDate(new Date(ms));
  }

  function fetchSnapshot() {
    return getDashboardSnapshot(
      range,
      0,
      range === 'custom' ? customStart : undefined,
      range === 'custom' ? customEnd : undefined
    );
  }

  async function loadDashboard(showSpinner = true) {
    if (!runtimeAvailable) {
      errorMessage = '请在 TokenScope 桌面端运行环境查看实时统计。';
      loading = false;
      return;
    }
    if (showSpinner) refreshing = true;
    errorMessage = '';
    try {
      const timeZone = Intl.DateTimeFormat().resolvedOptions().timeZone || 'UTC';
      let currentAgent: AgentStatus;
      if (!agentStarted) {
        currentAgent = await startAgent(timeZone);
        agentStarted = true;
      } else {
        currentAgent = await getAgentStatus();
        if (!currentAgent.running && currentAgent.binaryFound) {
          currentAgent = await startAgent(timeZone);
        }
      }
      agent = currentAgent;
      if (!currentAgent.running) {
        errorMessage = currentAgent.message;
        snapshot = null;
      } else if (!currentAgent.databaseReady) {
        errorMessage = 'Agent 已启动，正在初始化本地 SQLite WAL 数据库…';
        snapshot = null;
      } else {
        snapshot = await fetchSnapshot();
        if (range === 'all') {
          heatmapAllDaily = snapshot.daily;
        } else if (trendViewMode === 'heatmap') {
          void ensureHeatmapAllDaily();
        }
        if (!$hubState.displayName && snapshot.identity?.display_name) {
          hubController.setDisplayName(snapshot.identity.display_name);
        }
        if (activeTab === 'sessions') void sessionController.load();
      }
    } catch (error) {
      errorMessage = error instanceof Error ? error.message : String(error);
    } finally {
      loading = false;
      refreshing = false;
    }
  }

  function handleSelectRange(next: DashboardRange) {
    if (next === 'custom') {
      customOpen = true;
      return;
    }
    if (range === next) return;
    sessionController.resetForRangeChange();
    range = next;
    void loadDashboard();
  }

  function applyCustomRange() {
    if (!customStart || !customEnd || customStart > customEnd) {
      errorMessage = '请选择有效的起止日期';
      return;
    }
    if (customEnd > todayIso) {
      errorMessage = '结束日期不能晚于今天';
      return;
    }
    customOpen = false;
    sessionController.resetForRangeChange();
    range = 'custom';
    void loadDashboard();
  }

  function handleSelectTab(nextTab: Tab) {
    if (activeTab === nextTab) return;
    activeTab = nextTab;
    if (nextTab === 'sessions') void sessionController.load();
  }

  onMount(() => {
    const cleanSettingsListener = settingsController.mount();

    todayIso = toIsoDate(new Date());
    if (!customEnd) customEnd = todayIso;
    if (!customStart) customStart = toIsoDate(new Date(Date.now() - 6 * 86_400_000));

    if (runtimeAvailable) {
      hubController.restoreOrigin();
      void listen<{ protocol_version: number; snapshot: DashboardSnapshot }>('dashboard-updated', (event) => {
        const next = event.payload?.snapshot;
        if (!next || next.range !== range) return;
        if (range === 'custom' && next.range_start_ms !== null && snapshotStartIso(next.range_start_ms) !== customStart) return;
        snapshot = next;
        errorMessage = '';
        if (activeTab === 'sessions') void sessionController.load();
      }).then((unlisten) => {
        unlistenDashboard = unlisten;
      });
    }

    void loadDashboard().then(() => {
      if ($hubState.hubUrl) void hubController.refreshDevices();
    });

    // The Agent pushes a fresh projection whenever the WAL changes; this slow
    // timer is only a safety net for a restarted Agent or a missed event.
    const timer = window.setInterval(() => void loadDashboard(false), 300_000);
    return () => {
      window.clearInterval(timer);
      sessionController.dispose();
      unlistenDashboard?.();
      cleanSettingsListener();
    };
  });
</script>

<svelte:head>
  <title>TokenScope · 跨设备 AI Token 监控</title>
</svelte:head>

<div class="app-shell">
  <!-- Sidebar -->
  <aside class="sidebar">
    <div class="brand-row" title="TokenScope · Local Analytics">
      <div class="brand-prism">
        {@html icons.sparkle}
      </div>
      <div class="brand-title">
        TokenScope
        <small>Local Analytics</small>
      </div>
    </div>

    <div class="nav-caption">工作空间</div>
    <nav aria-label="主导航">
      {#each navItems as item}
        <button
          class="nav-button"
          class:active={activeTab === item.id}
          aria-current={activeTab === item.id ? 'page' : undefined}
          aria-label={item.label}
          title={item.label}
          onclick={() => handleSelectTab(item.id)}
        >
          <span class="nav-icon">{@html icons[item.iconKey]}</span>
          <span class="nav-label">{item.label}</span>
          {#if item.id === 'quota' && ((snapshot?.antigravity_quota.length ?? 0) + (snapshot?.codex_quota.length ?? 0)) > 0}
            <span class="nav-badge">{(snapshot?.antigravity_quota.length ?? 0) + (snapshot?.codex_quota.length ?? 0)}</span>
          {/if}
        </button>
      {/each}
    </nav>

    <div class="sidebar-spacer"></div>

    <div class="sidebar-footer">
      <div class="agent-heart-row">
        <span class="ping-dot" class:online={agent?.running}></span>
        <span class="agent-label">{agent?.running ? 'Agent 正在运行' : 'Agent 未连接'}</span>
        <span class="milestone-tag">{BOOTSTRAP_STAGE}</span>
      </div>

      <!-- Theme Switcher -->
      <div class="theme-switch-row" title="切换显示主题">
        <button
          class="theme-button"
          class:active={$settingsState.theme === 'dark'}
          aria-pressed={$settingsState.theme === 'dark'}
          onclick={() => settingsController.setTheme('dark')}
          title="深色模式"
        >
          {@html icons.moon}
        </button>
        <button
          class="theme-button"
          class:active={$settingsState.theme === 'light'}
          aria-pressed={$settingsState.theme === 'light'}
          onclick={() => settingsController.setTheme('light')}
          title="浅色模式"
        >
          {@html icons.sun}
        </button>
        <button
          class="theme-button"
          class:active={$settingsState.theme === 'system'}
          aria-pressed={$settingsState.theme === 'system'}
          onclick={() => settingsController.setTheme('system')}
          title="跟随系统"
        >
          {@html icons.laptop}
        </button>
      </div>
    </div>
  </aside>

  <!-- Main Area -->
  <main class="main-area">
    <!-- Topbar -->
    <header class="topbar">
      <div class="breadcrumbs">
        <span>TokenScope</span>
        <span class="sep">/</span>
        <strong>{navItems.find((t) => t.id === activeTab)?.label}</strong>
      </div>

      <div class="topbar-actions">
        <div class="machine-capsule" title="当前连接的物理设备">
          {@html icons.cpu}
          <span>{snapshot?.identity?.display_name ?? '本机工作站'}</span>
        </div>

        <div class="range-capsule" role="group" aria-label="时间跨度">
          {#each [
            { value: 'today', label: '今天' },
            { value: '7d', label: '7 天' },
            { value: '30d', label: '30 天' },
            { value: 'all', label: '全部' },
            { value: 'custom', label: '自定义' },
          ] as opt}
            <button
              class="range-btn"
              class:selected={range === opt.value}
              aria-pressed={range === opt.value}
              onclick={() => handleSelectRange(opt.value as DashboardRange)}
            >
              {opt.label}
            </button>
          {/each}
        </div>

        <button
          class="icon-btn"
          aria-label="刷新数据"
          title="刷新统计"
          disabled={refreshing}
          onclick={() => void loadDashboard()}
        >
          <span class:spin={refreshing}>{@html icons.refresh}</span>
        </button>
      </div>
    </header>

    {#if customOpen}
      <div class="custom-range-bar" aria-label="自定义日期范围">
        <span class="custom-range-label">自定义范围</span>
        <input type="date" bind:value={customStart} max={customEnd || todayIso} aria-label="开始日期" />
        <span class="custom-range-sep">至</span>
        <input type="date" bind:value={customEnd} min={customStart} max={todayIso} aria-label="结束日期" />
        <button class="custom-range-apply" onclick={applyCustomRange}>应用</button>
        <button class="custom-range-close" onclick={() => (customOpen = false)}>收起</button>
      </div>
    {/if}

    <!-- Page Content Area -->
    <section class="page-content" class:fit-viewport={activeTab === 'overview' && !!snapshot && !loading}>
      {#if errorMessage}
        <div class="notice-box" role="alert">
          {@html icons.alert}
          <p>{errorMessage}</p>
          <button class="notice-btn" onclick={() => void loadDashboard()}>重试</button>
        </div>
      {/if}

      {#if loading}
        <div class="loading-state" role="status" aria-live="polite">
          <div class="loading-message">
            <span class="loading-indicator" aria-hidden="true"></span>
            <p>正在连接本地守护进程并读取 SQLite 快照…</p>
          </div>
          <div class="loading-skeleton-grid" aria-hidden="true">
            <span class="loading-skeleton"></span>
            <span class="loading-skeleton"></span>
            <span class="loading-skeleton"></span>
            <span class="loading-skeleton"></span>
          </div>
          <div class="loading-skeleton loading-skeleton-panel" aria-hidden="true"></div>
        </div>
      {:else if snapshot}
        <!-- TAB 1: OVERVIEW (DASHBOARD) -->
        {#if activeTab === 'overview'}
          <OverviewPage
            {snapshot}
            local={dashboardView.local}
            exactRatio={dashboardView.exactRatio}
            cacheHitRate={dashboardView.tokenShares.cacheHitRate}
            inputShareRate={dashboardView.tokenShares.inputShareRate}
            outputShareRate={dashboardView.tokenShares.outputShareRate}
            cacheShareRate={dashboardView.tokenShares.cacheShareRate}
            reasoningShareRate={dashboardView.tokenShares.reasoningShareRate}
            cacheTokens={dashboardView.tokenShares.cacheTokens}
            {trendViewMode}
            bind:hoveredCurveIndex
            bind:hoveredHeatmapCell
            chartValues={dashboardView.chartValues}
            chartMax={dashboardView.chartMax}
            chartPoints={dashboardView.chartPoints}
            hoveredCurvePoint={dashboardView.hoveredCurvePoint}
            heatmapCalendar={dashboardView.heatmapCalendar}
            topToolTotal={dashboardView.topToolTotal}
            onSwitchTrendView={handleSwitchTrendView}
            onNavigateTools={() => handleSelectTab('tools')}
            onNavigateSessions={() => handleSelectTab('sessions')}
          />
        <!-- TAB 2: MULTI-DEVICE (多设备网络) -->
        {:else if activeTab === 'devices'}
          <DevicesPage
            {snapshot}
            local={dashboardView.local}
            remoteDevices={$hubState.devices}
            {selectableRemoteDevices}
            bind:remoteSelectedDevice
            remoteToolRows={dashboardView.remoteToolRows}
            remoteModelRows={dashboardView.remoteModelRows}
            bind:hubUrl={$hubState.hubUrl}
            bind:pairingCode={$hubState.pairingCode}
            bind:hubDisplayName={$hubState.displayName}
            hubBusy={$hubState.busy}
            hubError={$hubState.error}
            hubSuccess={$hubState.success}
            hubPaired={$hubState.paired}
            onPairWithHub={hubController.pair}
            onRefreshHubDevices={hubController.refreshDevices}
            onRenameHubDevice={hubController.renameDevice}
            onRotateHubCredential={hubController.rotateCredential}
            onDisconnectHub={hubController.disconnect}
          />
        <!-- TAB 3: TOOLS ANALYSIS (工具分析) -->
        {:else if activeTab === 'tools'}
          <ToolsPage snapshot={snapshot} local={dashboardView.local} />
        <!-- TAB 4: MODELS ANALYSIS (模型分析) -->
        {:else if activeTab === 'models'}
          <ModelsPage
            {snapshot}
            sortedModels={dashboardView.sortedModels}
            topModelTotal={dashboardView.topModelTotal}
            bind:modelSortBy
          />
        {:else if activeTab === 'sessions'}
          <SessionsPage
            sessionAnalysis={$sessionState.analysis}
            {sessionPage}
            {sessionPageStart}
            {sessionPageEnd}
            {range}
            rangeLabel={dashboardView.rangeLabel}
            bind:sessionSearch={$sessionState.search}
            bind:sessionToolFilter={$sessionState.toolFilter}
            bind:sessionModelFilter={$sessionState.modelFilter}
            bind:sessionProjectFilter={$sessionState.projectFilter}
            sessionLoading={$sessionState.loading}
            sessionError={$sessionState.error}
            sessionOffset={$sessionState.offset}
            pendingSessionDeleteKey={$sessionState.pendingDeleteKey}
            deletingSessionKey={$sessionState.deletingKey}
            sessionActionMessage={$sessionState.actionMessage}
            sessionActionError={$sessionState.actionError}
            {sessionFilterOptions}
            {hasSessionFilters}
            {refreshing}
            sessionActionKey={sessionController.actionKey}
            onScheduleSessionAnalysis={sessionController.schedule}
            onClearSessionFilters={sessionController.clearFilters}
            onSessionPage={sessionController.setPage}
            getSessionProjectLabel={sessionController.projectLabel}
            onRequestSessionDelete={sessionController.requestDelete}
            onCancelSessionDelete={sessionController.cancelDelete}
            onConfirmSessionDelete={sessionController.confirmDelete}
            onSelectRange={handleSelectRange}
            onLoadSessionAnalysis={sessionController.load}
          />
        <!-- TAB 5: PROJECTS (项目分析) -->
        {:else if activeTab === 'projects'}
          <ProjectsPage snapshot={snapshot} local={dashboardView.local} />
        <!-- PROVIDER QUOTAS AND ACTIVITY -->
        {:else if activeTab === 'quota'}
          <QuotaPage snapshot={snapshot} antigravityUsage={dashboardView.antigravityUsage} />
        <!-- TAB 7: SETTINGS (系统设置) -->
        {:else if activeTab === 'settings'}
          <SettingsPage
            currentTheme={$settingsState.theme}
            autostartEnabled={$settingsState.autostartEnabled}
            autostartBusy={$settingsState.autostartBusy}
            autostartError={$settingsState.autostartError}
            {runtimeAvailable}
            {agent}
            onSwitchTheme={settingsController.setTheme}
            onToggleAutostart={settingsController.toggleAutostart}
            onRunDiagnostics={runDiagnostics}
          />
        {/if}
      {/if}
    </section>
  </main>
</div>
