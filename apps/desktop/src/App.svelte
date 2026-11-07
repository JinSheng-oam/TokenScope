<script lang="ts">
  import DateRangePicker from './lib/DateRangePicker.svelte';
  import { createRequestGate, dashboardRequestKey } from './lib/dashboardRequests';
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import {
    buildDashboardProjection,
    filterActiveAntigravityQuotas,
    formatCompactTokens,
    type DeviceScope,
    type ModelSortBy,
    type TrendViewMode,
  } from './lib/dashboardView';
  import {
    getAgentStatus,
    getDashboardSnapshot,
    getRecentSessionActivity,
    isTauriRuntime,
    refreshCodexQuotaNow,
    runDiagnostics,
    startAgent,
  } from './lib/api';
  import { toIsoDate, type HeatmapCell } from './lib/heatmap';
  import { createHubController } from './lib/hubController';
  import { createWebDavController } from './lib/webdavController';
  import { icons } from './lib/icons';
  import { createSessionController } from './lib/sessionController';
  import { createSettingsController } from './lib/settingsController';
  import { BOOTSTRAP_STAGE } from './lib/status';
  import type { AgentStatus, CodexQuota, DashboardObservations, DashboardRange, DashboardSnapshot, RemoteDevice, SessionActivity } from './lib/types';
  import DevicesPage from './lib/DevicesPage.svelte';
  import ModelsPage from './lib/ModelsPage.svelte';
  import OverviewPage from './lib/OverviewPage.svelte';
  import ProjectsPage from './lib/ProjectsPage.svelte';
  import QuotaPage from './lib/QuotaPage.svelte';
  import SessionsPage from './lib/SessionsPage.svelte';
  import SettingsPage from './lib/SettingsPage.svelte';
  import ToolsPage from './lib/ToolsPage.svelte';
  import AgentCustomizations from './lib/AgentCustomizations.svelte';

  type Tab = 'overview' | 'devices' | 'tools' | 'models' | 'sessions' | 'projects' | 'quota' | 'settings' | 'customizations';
  let activeTab: Tab = 'overview';
  let customizationDirty = false;
  let range: DashboardRange = 'all';
  let customStart = '';
  let customEnd = '';
  let customOpen = false;
  let todayIso = '';

  let snapshot: DashboardSnapshot | null = null;
  let agent: AgentStatus | null = null;
  let loading = true;
  let refreshing = false;
  let codexQuotaRefreshing = false;
  let codexQuotaError = '';
  let codexQuotaOverride: CodexQuota[] | null = null;
  let sessionActivity: SessionActivity[] = [];
  let activityPolling = false;
  let activityTimer: number | undefined;
  let quotaTimer: number | undefined;
  let lastQuotaAttemptAt = 0;
  let errorMessage = '';
  let runtimeAvailable = isTauriRuntime();
  let agentStarted = false;
  let latestObservations: DashboardObservations | null = null;
  let unlistenObservations: (() => void) | undefined;
  let disposed = false;
  let unlistenDashboard: (() => void) | undefined;
  let refreshTimer: number | undefined;
  let appliedRefreshIntervalMs = -1;
  const dashboardRequests = createRequestGate();
  let appliedSnapshotKey = '';
  let heatmapRequest = 0;

  let remoteSelectedDevice = 'all';
  let deviceScope: DeviceScope = (typeof localStorage !== 'undefined' && localStorage.getItem('tokenscope.deviceScope')) || 'all';
  let deviceMenuOpen = false;

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
  const webdavController = createWebDavController(() => runtimeAvailable);
  const webdavState = webdavController.state;
  const settingsController = createSettingsController(() => runtimeAvailable);
  const settingsState = settingsController.state;

  // Sort & Trend View States
  let modelSortBy: ModelSortBy = 'tokens';
  let trendViewMode: TrendViewMode = 'curve';
  let hoveredCurveIndex: number | null = null;
  let heatmapSnapshot: DashboardSnapshot | null = null;
  let hoveredHeatmapCell: HeatmapCell | null = null;

  $: activeAntigravityQuotas = filterActiveAntigravityQuotas(snapshot?.antigravity_quota ?? []);
  $: exhaustedQuotaCount = (
    ((codexQuotaOverride ?? snapshot?.codex_quota ?? []).filter((q) => q.remaining_basis_points === 0).length) +
    activeAntigravityQuotas.filter((q) => q.remaining_basis_points !== null && q.remaining_basis_points === 0).length
  );
  $: warningQuotaCount = (
    ((codexQuotaOverride ?? snapshot?.codex_quota ?? []).filter((q) => q.remaining_basis_points !== null && q.remaining_basis_points > 0 && q.remaining_basis_points < 2000).length) +
    activeAntigravityQuotas.filter((q) => q.remaining_basis_points !== null && q.remaining_basis_points > 0 && q.remaining_basis_points < 2000).length
  );

  async function ensureHeatmapAllDaily() {
    if (!runtimeAvailable) return;
    if (range === 'all' && snapshot?.daily) {
      heatmapSnapshot = snapshot;
      return;
    }
    const request = ++heatmapRequest;
    try {
      const fullSnap = await getDashboardSnapshot('all', 0, undefined, undefined, false);
      if (request === heatmapRequest && (!heatmapSnapshot || fullSnap.generated_at_ms >= heatmapSnapshot.generated_at_ms)) heatmapSnapshot = fullSnap;
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
    { id: 'customizations', label: 'Agent 配置', iconKey: 'tools' },
    { id: 'settings', label: '系统设置', iconKey: 'settings' },
  ];

  // All dashboard-specific projection rules live behind this pure view module.
  $: dashboardView = buildDashboardProjection({
    snapshot,
    heatmapSnapshot,
    todayIso,
    hoveredCurveIndex,
    remoteSelectedDevice,
    deviceScope,
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

  $: allRemoteDevices = (() => {
    const map = new Map<string, RemoteDevice>();
    for (const d of $hubState.devices) {
      map.set(d.device_id, d);
    }
    for (const d of $webdavState.devices) {
      const existing = map.get(d.device_id);
      if (!existing || (d.last_seen_ms ?? 0) >= (existing.last_seen_ms ?? 0)) {
        map.set(d.device_id, d);
      }
    }
    for (const r of snapshot?.remote_by_device ?? []) {
      if (!map.has(r.owner_device_id) && r.owner_device_id !== snapshot?.identity?.device_id) {
        map.set(r.owner_device_id, {
          device_id: r.owner_device_id,
          display_name: `远端设备 (${r.owner_device_id.slice(0, 6)})`,
          os_name: 'other',
          app_version: 'synced',
          time_zone: snapshot?.time_zone ?? 'UTC',
          created_at_ms: 0,
          last_seen_ms: null,
          revoked_at_ms: null,
        });
      }
    }
    return Array.from(map.values());
  })();

  $: deviceScopeLabel = (() => {
    const localName = snapshot?.identity?.display_name ?? '本机工作站';
    if (deviceScope === 'all') {
      return selectableRemoteDevices.length > 0 || (snapshot?.remote.total_tokens ?? 0) > 0
        ? `全部设备汇总 (${1 + Math.max(selectableRemoteDevices.length, snapshot?.remote_by_device.length ?? 0)}台)`
        : `全部设备 · ${localName}`;
    }
    if (deviceScope === 'local') {
      return `本机 · ${localName}`;
    }
    const found = selectableRemoteDevices.find((d) => d.device_id === deviceScope);
    return found ? `远端 · ${found.display_name}` : `远端 · ${deviceScope.slice(0, 6)}`;
  })();

  function selectDeviceScope(nextScope: DeviceScope) {
    deviceScope = nextScope;
    deviceMenuOpen = false;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('tokenscope.deviceScope', nextScope);
    }
  }

  $: selectableRemoteDevices = allRemoteDevices.filter(
    (d) => d.device_id !== snapshot?.identity?.device_id && d.revoked_at_ms === null
  );

  function fetchSnapshot(selectedRange: DashboardRange, start: string, end: string) {
    return getDashboardSnapshot(
      selectedRange,
      0,
      selectedRange === 'custom' ? start : undefined,
      selectedRange === 'custom' ? end : undefined
    );
  }

  function quotaCovers(stored: CodexQuota[], candidate: CodexQuota[]) {
    return candidate.length > 0 && candidate.every((q) => stored.some((s) =>
      s.source_id === q.source_id && s.pool_id === q.pool_id &&
      s.window_duration_minutes === q.window_duration_minutes && s.observed_at_ms >= q.observed_at_ms
    ));
  }

  function applySnapshot(next: DashboardSnapshot, key: string) {
    if (key !== dashboardRequestKey(range, customStart, customEnd)) return;
    if (appliedSnapshotKey === key && snapshot && snapshot.generated_at_ms > next.generated_at_ms) return;
    appliedSnapshotKey = key;
    if (latestObservations && latestObservations.observed_at_ms > next.generated_at_ms) {
      const { observed_at_ms, ...update } = latestObservations;
      next = { ...next, ...update };
    }
    snapshot = next;
    if (next.range === 'all') heatmapSnapshot = next;
    else if (trendViewMode === 'heatmap') void ensureHeatmapAllDaily();
    if (codexQuotaOverride && quotaCovers(next.codex_quota, codexQuotaOverride)) codexQuotaOverride = null;
  }

  async function pollSessionActivity() {
    if (!runtimeAvailable || (activeTab !== 'overview' && activeTab !== 'sessions') || document.visibilityState !== 'visible' || activityPolling) return;
    activityPolling = true;
    try {
      sessionActivity = await getRecentSessionActivity();
    } catch {
      sessionActivity = [];
    } finally {
      activityPolling = false;
    }
  }

  async function loadDashboard(showSpinner = true) {
    const request = dashboardRequests.begin();
    const selectedRange = range, start = customStart, end = customEnd;
    const key = dashboardRequestKey(selectedRange, start, end);
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
      if (!dashboardRequests.isCurrent(request)) return;
      agent = currentAgent;
      if (!currentAgent.running) {
        errorMessage = currentAgent.message;
        snapshot = null;
      } else if (!currentAgent.databaseReady) {
        errorMessage = 'Agent 已启动，正在初始化本地 SQLite WAL 数据库…';
        snapshot = null;
      } else {
        const nextSnapshot = await fetchSnapshot(selectedRange, start, end);
        if (!dashboardRequests.isCurrent(request)) return;
        applySnapshot(nextSnapshot, key);
        if (!$hubState.displayName && nextSnapshot.identity?.display_name) {
          hubController.setDisplayName(nextSnapshot.identity.display_name);
        }
        if (activeTab === 'sessions') void sessionController.load();
      }
    } catch (error) {
      if (dashboardRequests.isCurrent(request)) errorMessage = error instanceof Error ? error.message : String(error);
    } finally {
      if (dashboardRequests.isCurrent(request)) { loading = false; refreshing = false; }
    }
  }

  async function handleRefreshCodexQuota() {
    if (!runtimeAvailable || codexQuotaRefreshing) return;
    lastQuotaAttemptAt = Date.now();
    codexQuotaRefreshing = true;
    codexQuotaError = '';
    try {
      const result = await refreshCodexQuotaNow();
      const storedIsNewer = quotaCovers(snapshot?.codex_quota ?? [], result.snapshots);
      codexQuotaOverride = storedIsNewer ? null : result.snapshots;
      codexQuotaError = result.warning ?? '';
      if (result.persisted && snapshot && !storedIsNewer) {
        snapshot = { ...snapshot, codex_quota: result.snapshots };
      }
    } catch (error) {
      codexQuotaError = error instanceof Error ? error.message : String(error);
    } finally {
      codexQuotaRefreshing = false;
    }
  }

  function pollVisibleCodexQuota() {
    if (!runtimeAvailable || activeTab !== 'quota' || document.visibilityState !== 'visible' || codexQuotaRefreshing) return;
    const newest = Math.max(0, ...(codexQuotaOverride ?? snapshot?.codex_quota ?? []).map((q) => q.observed_at_ms));
    if (Date.now() - newest >= 60_000 && Date.now() - lastQuotaAttemptAt >= 60_000) void handleRefreshCodexQuota();
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
    if (activeTab === 'customizations' && customizationDirty && !window.confirm('当前文件有未保存的更改，放弃更改？')) return;
    activeTab = nextTab;
    if (nextTab === 'overview') void pollSessionActivity();
    if (nextTab === 'quota') pollVisibleCodexQuota();
    if (nextTab === 'sessions') {
      void pollSessionActivity();
      void sessionController.load();
    }
    if (nextTab === 'devices' && $webdavState.enabled) void webdavController.refreshDevices();
  }

  function applyRefreshInterval(intervalMs: number) {
    if (intervalMs === appliedRefreshIntervalMs) return;
    appliedRefreshIntervalMs = intervalMs;
    if (refreshTimer !== undefined) {
      window.clearInterval(refreshTimer);
      refreshTimer = undefined;
    }
    if (intervalMs > 0) {
      refreshTimer = window.setInterval(() => { if (document.visibilityState === 'visible') void loadDashboard(false); }, intervalMs);
    }
  }

  onMount(() => {
    const cleanSettingsListener = settingsController.mount();
    const unsubscribeRefreshInterval = settingsState.subscribe((current) => {
      applyRefreshInterval(current.refreshIntervalMs);
    });

    todayIso = toIsoDate(new Date());
    if (!customEnd) customEnd = todayIso;
    if (!customStart) customStart = toIsoDate(new Date(Date.now() - 6 * 86_400_000));

    if (runtimeAvailable) {
      hubController.restoreOrigin();
      void listen<{ protocol_version: number; request_key: string; snapshot: DashboardSnapshot }>('dashboard-updated', (event) => {
        const next = event.payload?.snapshot;
        if (!next || event.payload.request_key !== dashboardRequestKey(range, customStart, customEnd)) return;
        applySnapshot(next, event.payload.request_key);
        errorMessage = '';
        if (activeTab === 'sessions') void sessionController.load();
      }).then((unlisten) => {
        if (disposed) unlisten(); else unlistenDashboard = unlisten;
      });
    }

    if (runtimeAvailable) {
      void listen<DashboardObservations>('dashboard-observations-updated', ({payload}) => {
        if (latestObservations && payload.observed_at_ms < latestObservations.observed_at_ms) return;
        latestObservations = payload;
        if (snapshot && payload.observed_at_ms >= snapshot.generated_at_ms) {
          const { observed_at_ms, ...update } = payload;
          snapshot = { ...snapshot, ...update };
          if (codexQuotaOverride && quotaCovers(update.codex_quota,codexQuotaOverride)) codexQuotaOverride = null;
        }
      }).then((unlisten) => { if (disposed) unlisten(); else unlistenObservations = unlisten; });
    }

    void pollSessionActivity();
    activityTimer = window.setInterval(() => void pollSessionActivity(), 10_000);
    quotaTimer = window.setInterval(pollVisibleCodexQuota, 30_000);

    void loadDashboard().then(() => {
      if ($hubState.hubUrl) void hubController.refreshDevices();
      void webdavController.loadConfig();
    });

    return () => {
      disposed = true;
      dashboardRequests.invalidate();
      heatmapRequest += 1;
      unsubscribeRefreshInterval();
      if (activityTimer !== undefined) window.clearInterval(activityTimer);
      if (quotaTimer !== undefined) window.clearInterval(quotaTimer);
      if (refreshTimer !== undefined) {
        window.clearInterval(refreshTimer);
        refreshTimer = undefined;
      }
      sessionController.dispose();
      unlistenDashboard?.();
      unlistenObservations?.();
      cleanSettingsListener();
    };
  });
</script>

<svelte:head>
  <title>TokenScope · 跨设备 AI Token 监控</title>
</svelte:head>

<svelte:window
  onmousedown={(e) => {
    if (deviceMenuOpen && !(e.target as Element | null)?.closest('.device-switcher-wrap')) {
      deviceMenuOpen = false;
    }
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape' && deviceMenuOpen) {
      deviceMenuOpen = false;
    }
  }}
/>

<div class="app-shell">
  <!-- Sidebar -->
  <aside class="sidebar">
    <div class="brand-row" title="TokenScope">
      <div class="brand-prism">
        {@html icons.sparkle}
      </div>
      <div class="brand-title">
        TokenScope
      </div>
    </div>

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
          {#if item.id === 'quota' && exhaustedQuotaCount > 0}
            <span class="nav-badge alert" title={`${exhaustedQuotaCount} 个额度池已耗尽`}>{exhaustedQuotaCount}</span>
          {:else if item.id === 'quota' && warningQuotaCount > 0}
            <span class="nav-badge warn" title={`${warningQuotaCount} 个额度池即将用尽`}>{warningQuotaCount}</span>
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
        <div class="device-switcher-wrap">
          <button
            type="button"
            class="machine-capsule interactive"
            class:active={deviceMenuOpen || deviceScope !== 'local'}
            title="切换设备数据统计范围（可汇总全部设备或单看某台设备）"
            aria-haspopup="listbox"
            aria-expanded={deviceMenuOpen}
            onclick={() => (deviceMenuOpen = !deviceMenuOpen)}
          >
            {@html icons.cpu}
            <span>{deviceScopeLabel}</span>
            <svg class="capsule-chevron" class:rotated={deviceMenuOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
              <polyline points="6 9 12 15 18 9" />
            </svg>
          </button>

          {#if deviceMenuOpen}
            <div class="device-scope-dropdown" role="listbox" aria-label="选择统计设备范围">
              <button
                type="button"
                class="device-scope-option"
                class:selected={deviceScope === 'all'}
                role="option"
                aria-selected={deviceScope === 'all'}
                onclick={() => selectDeviceScope('all')}
              >
                <div class="scope-opt-main">
                  <span class="scope-opt-title">全部设备汇总</span>
                </div>
                <span class="scope-opt-badge tabular">
                  {formatCompactTokens((snapshot?.local.total_tokens ?? 0) + (snapshot?.remote.total_tokens ?? 0))}
                </span>
              </button>

              <button
                type="button"
                class="device-scope-option"
                class:selected={deviceScope === 'local'}
                role="option"
                aria-selected={deviceScope === 'local'}
                onclick={() => selectDeviceScope('local')}
              >
                <div class="scope-opt-main">
                  <span class="scope-opt-title">本机 · {snapshot?.identity?.display_name ?? 'This device'}</span>
                </div>
                <span class="scope-opt-badge tabular">
                  {formatCompactTokens(snapshot?.local.total_tokens ?? 0)}
                </span>
              </button>

              {#if selectableRemoteDevices.length > 0}
                <div class="device-scope-divider">已同步远端设备</div>
                {#each selectableRemoteDevices as rDev}
                  {@const rTokens = snapshot?.remote_by_device.find((x) => x.owner_device_id === rDev.device_id)?.total_tokens ?? 0}
                  <button
                    type="button"
                    class="device-scope-option"
                    class:selected={deviceScope === rDev.device_id}
                    role="option"
                    aria-selected={deviceScope === rDev.device_id}
                    onclick={() => selectDeviceScope(rDev.device_id)}
                  >
                    <div class="scope-opt-main">
                      <span class="scope-opt-title">{rDev.display_name}</span>
                      <span class="scope-opt-sub">ID: …{rDev.device_id.slice(-8)}</span>
                    </div>
                    <span class="scope-opt-badge tabular">{formatCompactTokens(rTokens)}</span>
                  </button>
                {/each}
              {/if}
            </div>
          {/if}
        </div>

        <div class="range-capsule" role="group" aria-label="时间跨度">
          {#each [
            { value: 'today', label: '今天' },
            { value: '7d', label: '7 天' },
            { value: '30d', label: '30 天' },
            { value: 'all', label: '全部' },
          ] as opt}
            <button
              class="range-btn"
              class:selected={range === opt.value}
              aria-pressed={range === opt.value}
              onclick={() => {
                customOpen = false;
                handleSelectRange(opt.value as DashboardRange);
              }}
            >
              {opt.label}
            </button>
          {/each}

          <div class="custom-range-wrap">
            <button
              class="range-btn"
              class:selected={range === 'custom'}
              aria-pressed={range === 'custom'}
              onclick={() => (customOpen = !customOpen)}
            >
              {#if range === 'custom' && customStart && customEnd}
                {customStart.slice(5)} ~ {customEnd.slice(5)}
              {:else}
                自定义
              {/if}
            </button>

            {#if customOpen}
              <DateRangePicker
                startDate={customStart}
                endDate={customEnd}
                maxDate={todayIso}
                onApply={({ start, end }) => {
                  customStart = start;
                  customEnd = end;
                  customOpen = false;
                  handleSelectRange('custom');
                  applyCustomRange();
                }}
                onClose={() => (customOpen = false)}
              />
            {/if}
          </div>
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

    <!-- Page Content Area -->
    <section class="page-content" class:fit-viewport={(activeTab === 'overview' && !!snapshot) || loading}>
      {#if errorMessage}
        <div class="notice-box" role="alert">
          {@html icons.alert}
          <p>{errorMessage}</p>
          <button class="notice-btn" onclick={() => void loadDashboard()}>重试</button>
        </div>
      {/if}

      {#if activeTab === 'customizations'}
        <AgentCustomizations {runtimeAvailable} bind:dirty={customizationDirty} />
      {:else if loading}
        <div class="loading-state" role="status" aria-live="polite">
          <div class="loading-status-bar">
            <div class="loading-message">
              <span class="loading-spinner-ring" aria-hidden="true">
                <span class="loading-spinner-core"></span>
              </span>
              <p>正在加载用量…</p>
            </div>
            <span class="loading-stage-pill" aria-hidden="true">WAL 并发只读通道</span>
          </div>

          <div class="loading-skeleton-grid" aria-hidden="true">
            {#each [
              { titleW: '38%', valW: '72%', subW: '44%', hero: true },
              { titleW: '42%', valW: '62%', subW: '36%', hero: false },
              { titleW: '40%', valW: '56%', subW: '40%', hero: false },
              { titleW: '44%', valW: '68%', subW: '48%', hero: false },
            ] as card, i}
              <div class="loading-skeleton-card" class:hero={card.hero} style={`--delay: ${i * 90}ms`}>
                <div class="sk-row-between">
                  <span class="sk-bone sk-title" style={`width: ${card.titleW}`}></span>
                  <span class="sk-bone sk-pill"></span>
                </div>
                <span class="sk-bone sk-value" style={`width: ${card.valW}`}></span>
                <span class="sk-bone sk-bar"></span>
                <div class="sk-row-between">
                  <span class="sk-bone sk-foot" style={`width: ${card.subW}`}></span>
                  <span class="sk-bone sk-foot" style="width: 24%"></span>
                </div>
              </div>
            {/each}
          </div>

          <div class="loading-integrity-skeleton" aria-hidden="true">
            <div class="sk-inline-group">
              <span class="sk-bone sk-tag" style="width: 96px"></span>
              <span class="sk-bone sk-tag" style="width: 104px"></span>
              <span class="sk-bone sk-tag" style="width: 84px"></span>
            </div>
            <span class="sk-bone sk-tag" style="width: 160px"></span>
          </div>

          <div class="loading-panels-grid" aria-hidden="true">
            <div class="loading-skeleton-panel" style="--delay: 120ms">
              <div class="sk-panel-head">
                <div class="sk-stack">
                  <span class="sk-bone sk-h2" style="width: 128px"></span>
                  <span class="sk-bone sk-sub" style="width: 220px"></span>
                </div>
                <span class="sk-bone sk-switch"></span>
              </div>
              <div class="sk-chart-bars">
                {#each [38, 56, 44, 74, 62, 88, 52, 68, 92, 60, 78, 84, 48, 70] as h, idx}
                  <span class="sk-chart-col" style={`height: ${h}%; --bar-delay: ${idx * 45}ms`}></span>
                {/each}
              </div>
            </div>

            <div class="loading-skeleton-panel" style="--delay: 220ms">
              <div class="sk-panel-head">
                <div class="sk-stack">
                  <span class="sk-bone sk-h2" style="width: 108px"></span>
                  <span class="sk-bone sk-sub" style="width: 156px"></span>
                </div>
                <span class="sk-bone sk-pill"></span>
              </div>
              <div class="sk-rank-list">
                {#each ['86%', '62%', '38%', '22%'] as w}
                  <div class="sk-rank-item">
                    <div class="sk-row-between">
                      <span class="sk-bone sk-title" style="width: 36%"></span>
                      <span class="sk-bone sk-title" style="width: 28%"></span>
                    </div>
                    <span class="sk-bone sk-bar" style={`width: ${w}`}></span>
                  </div>
                {/each}
              </div>
            </div>
          </div>

          <div class="loading-support-grid" aria-hidden="true">
            <div class="loading-skeleton-panel" style="--delay: 280ms">
              <div class="sk-panel-head">
                <div class="sk-stack">
                  <span class="sk-bone sk-h2" style="width: 92px"></span>
                  <span class="sk-bone sk-sub" style="width: 196px"></span>
                </div>
                <span class="sk-bone sk-pill"></span>
              </div>
              <div class="sk-session-list">
                {#each ['46%', '58%', '40%', '52%'] as titleW}
                  <div class="sk-session-row">
                    <div class="sk-stack" style="flex: 1; min-width: 0;">
                      <span class="sk-bone sk-title" style={`width: ${titleW}`}></span>
                      <span class="sk-bone sk-foot" style="width: 32%"></span>
                    </div>
                    <span class="sk-bone sk-title" style="width: 72px"></span>
                  </div>
                {/each}
              </div>
            </div>

            <div class="loading-skeleton-panel" style="--delay: 340ms">
              <div class="sk-panel-head">
                <div class="sk-stack">
                  <span class="sk-bone sk-h2" style="width: 96px"></span>
                  <span class="sk-bone sk-sub" style="width: 148px"></span>
                </div>
                <span class="sk-bone sk-pill"></span>
              </div>
              <div class="sk-session-list">
                {#each ['34%', '28%', '30%', '42%'] as nameW}
                  <div class="sk-session-row">
                    <span class="sk-bone sk-title" style={`width: ${nameW}`}></span>
                    <span class="sk-bone sk-foot" style="width: 76px"></span>
                    <span class="sk-bone sk-pill" style="width: 40px; height: 16px;"></span>
                  </div>
                {/each}
              </div>
            </div>
          </div>
        </div>
      {:else if snapshot}
        <!-- TAB 1: OVERVIEW (DASHBOARD) -->
        {#if activeTab === 'overview'}
          <OverviewPage
            {snapshot}
            {sessionActivity}
            local={dashboardView.local}
            tools={dashboardView.scopedTools}
            {deviceScope}
            {deviceScopeLabel}
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
            rollingNumbersEnabled={$settingsState.rollingNumbersEnabled}
            onNavigateTools={() => handleSelectTab('tools')}
            onNavigateSessions={() => handleSelectTab('sessions')}
          />
        <!-- TAB 2: MULTI-DEVICE (多设备网络) -->
        {:else if activeTab === 'devices'}
          <DevicesPage
            {snapshot}
            local={dashboardView.local}
            remoteDevices={allRemoteDevices}
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
            bind:webdavUrl={$webdavState.url}
            bind:webdavUsername={$webdavState.username}
            bind:webdavPassword={$webdavState.password}
            webdavEnabled={$webdavState.enabled}
            webdavBusy={$webdavState.busy}
            webdavTesting={$webdavState.testing}
            webdavSyncing={$webdavState.syncing}
            webdavError={$webdavState.error}
            webdavSuccess={$webdavState.success}
            webdavLastSyncAt={$webdavState.lastSyncAt}
            onTestWebDav={webdavController.testConnection}
            onSaveWebDav={webdavController.saveConfig}
            onSyncWebDavNow={webdavController.syncNow}
            onRefreshWebDavDevices={webdavController.refreshDevices}
            onDisconnectWebDav={webdavController.disconnect}
          />
        <!-- TAB 3: TOOLS ANALYSIS (工具分析) -->
        {:else if activeTab === 'tools'}
          <ToolsPage snapshot={snapshot} local={dashboardView.local} tools={dashboardView.scopedTools} />
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
            {sessionActivity}
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
            sessionPageSize={$sessionState.pageSize}
            onSetSessionPageSize={sessionController.setPageSize}
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
          <ProjectsPage snapshot={snapshot} local={dashboardView.local} projects={dashboardView.scopedProjects} />
        <!-- PROVIDER QUOTAS AND ACTIVITY -->
        {:else if activeTab === 'quota'}
          <QuotaPage
            snapshot={snapshot}
            antigravityUsage={dashboardView.antigravityUsage}
            {codexQuotaOverride}
            {codexQuotaRefreshing}
            {codexQuotaError}
            onRefreshCodexQuota={handleRefreshCodexQuota}
          />
        <!-- TAB 7: SETTINGS (系统设置) -->
        {:else if activeTab === 'settings'}
          <SettingsPage
            currentTheme={$settingsState.theme}
            rollingNumbersEnabled={$settingsState.rollingNumbersEnabled}
            refreshIntervalMs={$settingsState.refreshIntervalMs}
            autostartEnabled={$settingsState.autostartEnabled}
            autostartBusy={$settingsState.autostartBusy}
            autostartError={$settingsState.autostartError}
            {runtimeAvailable}
            {agent}
            onSwitchTheme={settingsController.setTheme}
            onToggleRollingNumbers={settingsController.toggleRollingNumbers}
            onSetRefreshInterval={settingsController.setRefreshInterval}
            onToggleAutostart={settingsController.toggleAutostart}
            onRunDiagnostics={runDiagnostics}
          />
        {/if}
      {/if}
    </section>
  </main>
</div>
