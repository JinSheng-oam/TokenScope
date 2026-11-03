<script lang="ts">
  import { onMount } from 'svelte';
  import { listen } from '@tauri-apps/api/event';
  import {
    disconnectHubDevice,
    deleteSession as deleteSessionUsage,
    getAgentStatus,
    getAutostartEnabled,
    getDashboardSnapshot,
    getSessionAnalysis,
    isTauriRuntime,
    listHubDevices,
    pairHubDevice,
    renameHubDevice,
    rotateHubDeviceToken,
    setAutostartEnabled,
    startAgent,
  } from './lib/api';
  import { formatCost, formatNumber, formatPercent, percentOf, relativeTime } from './lib/format';
  import { icons } from './lib/icons';
  import { BOOTSTRAP_STAGE } from './lib/status';
  import { applyTheme, getInitialTheme, initThemeListener, type ThemeMode } from './lib/theme';
import type { AgentStatus, DashboardRange, DashboardSnapshot, RemoteDevice, SessionAnalysisPage, SessionUsage } from './lib/types';

  type Tab = 'overview' | 'devices' | 'tools' | 'models' | 'sessions' | 'projects' | 'quota' | 'settings';
  let activeTab: Tab = 'overview';
  let range: DashboardRange = '7d';
  let sessionOffset = 0;
  let sessionAnalysis: SessionAnalysisPage | null = null;
  let sessionSearch = '';
  let sessionToolFilter = '';
  let sessionModelFilter = '';
  let sessionProjectFilter = '';
  let sessionLoading = false;
  let sessionError = '';
  let sessionSearchTimer: ReturnType<typeof setTimeout> | undefined;
  let sessionRequestSequence = 0;
  let pendingSessionDeleteKey = '';
  let deletingSessionKey = '';
  let sessionActionMessage = '';
  let sessionActionError = '';
  let customStart = '';
  let customEnd = '';
  let customOpen = false;
  let todayIso = '';
  let currentTheme: ThemeMode = 'dark';

  let snapshot: DashboardSnapshot | null = null;
  let agent: AgentStatus | null = null;
  let loading = true;
  let refreshing = false;
  let errorMessage = '';
  let runtimeAvailable = isTauriRuntime();
  let agentStarted = false;
  let unlistenDashboard: (() => void) | undefined;

  // Autostart State
  let autostartEnabled = false;
  let autostartBusy = false;
  let autostartError = '';

  // Hub Pairing & Devices
  let hubUrl = '';
  let pairingCode = '';
  let hubDisplayName = '';
  let remoteDevices: RemoteDevice[] = [];
  let hubBusy = false;
  let hubError = '';
  let hubSuccess = '';
  let hubPaired = false;
  let remoteSelectedDevice = 'all';

  // Sort States
  let modelSortBy: 'tokens' | 'cost' = 'tokens';

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

  // Derived View Helpers
  $: local = snapshot?.local;
  $: chartValues = snapshot?.daily ?? [];
  $: chartMax = Math.max(1, ...chartValues.map((item) => item.total_tokens));
  $: chartPoints = chartValues.map((item, index) => ({
    ...item,
    x: chartValues.length <= 1 ? 400 : 25 + (index / (chartValues.length - 1)) * 750,
    y: 160 - (item.total_tokens / chartMax) * 130,
  }));
  $: chartPath = chartPoints.map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.x} ${point.y}`).join(' ');

  $: topToolTotal = Math.max(1, ...(snapshot?.by_tool ?? []).map((item) => item.total_tokens));
  $: topModelTotal = Math.max(1, ...(snapshot?.by_model ?? []).map((item) => item.total_tokens));
  $: sessionPage = sessionAnalysis?.page;
  $: sessionPageStart = sessionPage && sessionPage.total > 0 ? sessionPage.offset + 1 : 0;
  $: sessionPageEnd = Math.min((sessionPage?.offset ?? 0) + (sessionPage?.sessions.length ?? 0), sessionPage?.total ?? 0);
  $: sessionFilterOptions = sessionAnalysis?.filter_options ?? { tools: [], models: [], projects: [] };
  $: hasSessionFilters = Boolean(sessionSearch.trim() || sessionToolFilter || sessionModelFilter || sessionProjectFilter);
  $: antigravityUsage = (snapshot?.by_tool ?? []).find((item) => item.key === 'antigravity');

  $: selectableRemoteDevices = remoteDevices.filter(
    (d) => d.device_id !== snapshot?.identity?.device_id && d.revoked_at_ms === null
  );

  $: remoteToolRows = (snapshot?.remote_by_tool ?? []).filter(
    (row) => remoteSelectedDevice === 'all' || row.owner_device_id === remoteSelectedDevice
  );
  $: remoteModelRows = (snapshot?.remote_by_model ?? []).filter(
    (row) => remoteSelectedDevice === 'all' || row.owner_device_id === remoteSelectedDevice
  );
  $: remoteProjectRows = (snapshot?.remote_by_project ?? []).filter(
    (row) => remoteSelectedDevice === 'all' || row.owner_device_id === remoteSelectedDevice
  );

  $: sortedModels = [...(snapshot?.by_model ?? [])].sort((a, b) => {
    if (modelSortBy === 'cost') {
      return (b.estimated_cost_microusd ?? 0) - (a.estimated_cost_microusd ?? 0);
    }
    return b.total_tokens - a.total_tokens;
  });

  // Calculate Cache Savings Rate
  $: cacheTokens = (local?.cache_read_tokens ?? 0) + (local?.cache_write_tokens ?? 0);
  $: cacheInputTokens = (local?.input_tokens ?? 0) + (local?.cache_read_tokens ?? 0) + (local?.cache_write_tokens ?? 0);
  $: cacheHitRate = cacheInputTokens ? (((local?.cache_read_tokens ?? 0) / cacheInputTokens) * 100).toFixed(1) : '0.0';

  // Precision Ratio
  $: exactRatio = local?.record_count
    ? ((local.exact_records / local.record_count) * 100).toFixed(1)
    : '—';
  $: rangeLabel =
    range === 'all'
      ? '全部'
      : range === 'today'
        ? '今天'
        : range === '7d'
          ? '7 天'
          : range === '30d'
            ? '30 天'
            : `${customStart} 至 ${customEnd}`;

  function toIsoDate(date: Date): string {
    const year = date.getFullYear();
    const month = String(date.getMonth() + 1).padStart(2, '0');
    const day = String(date.getDate()).padStart(2, '0');
    return `${year}-${month}-${day}`;
  }

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
        if (!hubDisplayName && snapshot.identity?.display_name) {
          hubDisplayName = snapshot.identity.display_name;
        }
        if (activeTab === 'sessions') void loadSessionAnalysis();
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
    if (sessionSearchTimer) window.clearTimeout(sessionSearchTimer);
    sessionSearchTimer = undefined;
    sessionRequestSequence += 1;
    range = next;
    sessionOffset = 0;
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
    if (sessionSearchTimer) window.clearTimeout(sessionSearchTimer);
    sessionSearchTimer = undefined;
    sessionRequestSequence += 1;
    range = 'custom';
    sessionOffset = 0;
    void loadDashboard();
  }

  function handleSessionPage(nextOffset: number) {
    if (nextOffset < 0 || nextOffset === sessionOffset) return;
    sessionOffset = nextOffset;
    void loadSessionAnalysis();
  }

  function handleSelectTab(nextTab: Tab) {
    if (activeTab === nextTab) return;
    activeTab = nextTab;
    if (nextTab === 'sessions') void loadSessionAnalysis();
  }

  function scheduleSessionAnalysis() {
    sessionRequestSequence += 1;
    sessionOffset = 0;
    if (sessionSearchTimer) window.clearTimeout(sessionSearchTimer);
    sessionSearchTimer = window.setTimeout(() => {
      sessionSearchTimer = undefined;
      void loadSessionAnalysis();
    }, 220);
  }

  function clearSessionFilters() {
    sessionSearch = '';
    sessionToolFilter = '';
    sessionModelFilter = '';
    sessionProjectFilter = '';
    scheduleSessionAnalysis();
  }

  function sessionProjectLabel(projectId: string): string {
    return sessionFilterOptions.projects.find((option) => option.key === projectId)?.label
      ?? '项目名称未提供';
  }

  async function loadSessionAnalysis() {
    if (!runtimeAvailable || activeTab !== 'sessions') return;
    const requestSequence = ++sessionRequestSequence;
    sessionLoading = true;
    sessionError = '';
    const filters = {
      search: sessionSearch,
      toolId: sessionToolFilter,
      modelId: sessionModelFilter,
      projectId: sessionProjectFilter,
    };
    const loadPage = (offset: number) => getSessionAnalysis(
      range,
      offset,
      filters,
      range === 'custom' ? customStart : undefined,
      range === 'custom' ? customEnd : undefined
    );
    try {
      let result = await loadPage(sessionOffset);
      if (requestSequence !== sessionRequestSequence) return;
      const lastValidOffset = result.page.total === 0
        ? 0
        : Math.floor((result.page.total - 1) / result.page.limit) * result.page.limit;
      if (sessionOffset > lastValidOffset) {
        sessionOffset = lastValidOffset;
        result = await loadPage(sessionOffset);
        if (requestSequence !== sessionRequestSequence) return;
      }
      sessionAnalysis = result;
    } catch (error) {
      if (requestSequence === sessionRequestSequence) {
        sessionError = error instanceof Error ? error.message : String(error);
      }
    } finally {
      if (requestSequence === sessionRequestSequence) sessionLoading = false;
    }
  }

  function sessionActionKey(session: Pick<SessionUsage, 'tool_id' | 'session_id'>): string {
    return `${session.tool_id}\u001f${session.session_id}`;
  }

  function requestSessionDelete(session: SessionUsage) {
    sessionActionMessage = '';
    sessionActionError = '';
    pendingSessionDeleteKey = sessionActionKey(session);
  }

  function cancelSessionDelete() {
    pendingSessionDeleteKey = '';
    sessionActionError = '';
  }

  async function confirmSessionDelete(session: SessionUsage) {
    const key = sessionActionKey(session);
    if (deletingSessionKey) return;
    deletingSessionKey = key;
    sessionActionError = '';
    sessionActionMessage = '';
    try {
      const deletedRecords = await deleteSessionUsage(session.tool_id, session.session_id);
      pendingSessionDeleteKey = '';
      sessionActionMessage = `会话已从本机统计移除（${formatNumber(deletedRecords)} 条记录）；原应用聊天未改动。`;
      await loadDashboard(false);
    } catch (error) {
      sessionActionError = error instanceof Error ? error.message : String(error);
    } finally {
      deletingSessionKey = '';
    }
  }

  function quotaWindowLabel(durationMinutes: number): string {
    if (durationMinutes === 300) return '5 小时周期';
    if (durationMinutes === 10_080) return '每周周期';
    if (durationMinutes === 1_440) return '每日周期';
    return `${formatNumber(durationMinutes)} 分钟周期`;
  }

  function antigravityWindowLabel(windowId: string): string {
    if (windowId === 'five_hour') return '5 小时周期';
    if (windowId === 'weekly') return '每周周期';
    if (windowId === 'daily') return '每日周期';
    return '其他周期';
  }

  function quotaIsStale(observedAtMs: number): boolean {
    return Date.now() - observedAtMs > 15 * 60_000;
  }

  function handleSwitchTheme(mode: ThemeMode) {
    currentTheme = mode;
    applyTheme(mode);
  }

  async function handleToggleAutostart() {
    if (autostartBusy || !runtimeAvailable) return;
    autostartBusy = true;
    autostartError = '';
    try {
      autostartEnabled = await setAutostartEnabled(!autostartEnabled);
    } catch (error) {
      autostartError = error instanceof Error ? error.message : String(error);
    } finally {
      autostartBusy = false;
    }
  }

  async function handleRefreshHubDevices() {
    if (!runtimeAvailable || !hubUrl.trim()) return;
    hubBusy = true;
    hubError = '';
    hubSuccess = '';
    try {
      const result = await listHubDevices(hubUrl.trim());
      remoteDevices = result.devices;
      hubPaired = true;
      window.localStorage.setItem('tokenscope.hubUrl', hubUrl.trim());
    } catch (error) {
      hubError = error instanceof Error ? error.message : String(error);
      if (hubError.includes('尚未与该 Hub 配对') || hubError.includes('尚未保存')) {
        hubPaired = false;
        remoteDevices = [];
      }
    } finally {
      hubBusy = false;
    }
  }

  async function handlePairWithHub() {
    if (hubBusy || !runtimeAvailable) return;
    hubBusy = true;
    hubError = '';
    hubSuccess = '';
    try {
      const result = await pairHubDevice(hubUrl.trim(), pairingCode.trim(), hubDisplayName.trim());
      hubUrl = result.hubOrigin;
      window.localStorage.setItem('tokenscope.hubUrl', hubUrl);
      pairingCode = '';
      hubPaired = true;
      hubSuccess = `设备 ${result.deviceId} 配对成功，凭证已保存至操作系统安全密码库。`;
      const list = await listHubDevices(hubUrl);
      remoteDevices = list.devices;
    } catch (error) {
      hubError = error instanceof Error ? error.message : String(error);
      if (hubError.includes('配对请求连接中断') || hubError.includes('配对码无效')) {
        pairingCode = '';
      }
    } finally {
      hubBusy = false;
    }
  }

  async function handleRenameHubDevice() {
    if (hubBusy || !runtimeAvailable || !hubDisplayName.trim()) return;
    hubBusy = true;
    hubError = '';
    hubSuccess = '';
    try {
      const name = await renameHubDevice(hubUrl.trim(), hubDisplayName.trim());
      hubSuccess = `本机在 Hub 中的名称已更新为“${name}”。`;
      const list = await listHubDevices(hubUrl.trim());
      remoteDevices = list.devices;
    } catch (error) {
      hubError = error instanceof Error ? error.message : String(error);
    } finally {
      hubBusy = false;
    }
  }

  async function handleRotateHubCredential() {
    if (hubBusy || !runtimeAvailable || !window.confirm('轮换后旧设备凭证立即失效。确定继续吗？')) return;
    hubBusy = true;
    hubError = '';
    hubSuccess = '';
    try {
      await rotateHubDeviceToken(hubUrl.trim());
      hubSuccess = '设备凭证已轮换，并已更新到系统安全密码库。';
      const list = await listHubDevices(hubUrl.trim());
      remoteDevices = list.devices;
    } catch (error) {
      hubError = error instanceof Error ? error.message : String(error);
    } finally {
      hubBusy = false;
    }
  }

  async function handleDisconnectHub() {
    if (hubBusy || !runtimeAvailable || !window.confirm('这只会删除本机保存的 Hub 凭证。确定断开吗？')) return;
    hubBusy = true;
    hubError = '';
    hubSuccess = '';
    try {
      await disconnectHubDevice(hubUrl.trim());
      remoteDevices = [];
      hubPaired = false;
      hubSuccess = '本机 Hub 凭证已从系统安全密码库彻底删除。';
    } catch (error) {
      hubError = error instanceof Error ? error.message : String(error);
    } finally {
      hubBusy = false;
    }
  }

  function formatCountdown(resetAtMs: number | null): string {
    if (resetAtMs === null) return '未提供重置时间';
    const diff = resetAtMs - Date.now();
    if (diff <= 0) return '即将重置';
    const hours = Math.floor(diff / 3_600_000);
    const mins = Math.floor((diff % 3_600_000) / 60_000);
    const secs = Math.floor((diff % 60_000) / 1000);
    return `${hours.toString().padStart(2, '0')}:${mins.toString().padStart(2, '0')}:${secs.toString().padStart(2, '0')} 后重置`;
  }

  onMount(() => {
    currentTheme = getInitialTheme();
    applyTheme(currentTheme);
    const cleanThemeListener = initThemeListener(() => {});

    todayIso = toIsoDate(new Date());
    if (!customEnd) customEnd = todayIso;
    if (!customStart) customStart = toIsoDate(new Date(Date.now() - 6 * 86_400_000));

    if (runtimeAvailable) {
      void getAutostartEnabled().then((v) => (autostartEnabled = v)).catch(() => {});
      hubUrl = window.localStorage.getItem('tokenscope.hubUrl') ?? '';
      void listen<{ protocol_version: number; snapshot: DashboardSnapshot }>('dashboard-updated', (event) => {
        const next = event.payload?.snapshot;
        if (!next || next.range !== range) return;
        if (range === 'custom' && next.range_start_ms !== null && snapshotStartIso(next.range_start_ms) !== customStart) return;
        snapshot = next;
        errorMessage = '';
        if (activeTab === 'sessions') void loadSessionAnalysis();
      }).then((unlisten) => {
        unlistenDashboard = unlisten;
      });
    }

    void loadDashboard().then(() => {
      if (hubUrl) void handleRefreshHubDevices();
    });

    // The Agent pushes a fresh projection whenever the WAL changes; this slow
    // timer is only a safety net for a restarted Agent or a missed event.
    const timer = window.setInterval(() => void loadDashboard(false), 300_000);
    return () => {
      window.clearInterval(timer);
      if (sessionSearchTimer) window.clearTimeout(sessionSearchTimer);
      unlistenDashboard?.();
      cleanThemeListener();
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
          class:active={currentTheme === 'dark'}
          aria-pressed={currentTheme === 'dark'}
          onclick={() => handleSwitchTheme('dark')}
          title="深色模式"
        >
          {@html icons.moon}
        </button>
        <button
          class="theme-button"
          class:active={currentTheme === 'light'}
          aria-pressed={currentTheme === 'light'}
          onclick={() => handleSwitchTheme('light')}
          title="浅色模式"
        >
          {@html icons.sun}
        </button>
        <button
          class="theme-button"
          class:active={currentTheme === 'system'}
          aria-pressed={currentTheme === 'system'}
          onclick={() => handleSwitchTheme('system')}
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
    <section class="page-content">
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
          <div class="page-header">
            <h1>使用总览</h1>
            <p>
              统计时区 {snapshot.time_zone} · 生成于 {relativeTime(snapshot.generated_at_ms)} · 仅汇集物理单机有效事实
            </p>
          </div>

          <!-- 4 Metric Cards -->
          <div class="metric-grid">
            <article class="metric-card hero-card">
              <div class="metric-head">
                <span>总 Token 消耗</span>
                <span class="precision-chip exact">
                  {@html icons.shield} {exactRatio}% 精确
                </span>
              </div>
              <div class="metric-val tabular">
                {formatNumber(local?.total_tokens)}
              </div>
              <div class="metric-foot">
                <span>估算成本：<strong>{formatCost(local?.estimated_cost_microusd)}</strong></span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>输入 Token</span>
                <span class="metric-icon-wrap">{@html icons.arrowDown}</span>
              </div>
              <div class="metric-val tabular">
                {formatNumber(local?.input_tokens)}
              </div>
              <div class="metric-foot">
                <span>不含缓存读取与写入</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>输出 Token</span>
                <span class="metric-icon-wrap">{@html icons.arrowUp}</span>
              </div>
              <div class="metric-val tabular">
                {formatNumber(local?.output_tokens)}
              </div>
              <div class="metric-foot">
                <span>含推理 {formatNumber(local?.reasoning_tokens)} (作为输出子集)</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>缓存读写 Token</span>
                <span class="precision-chip exact">读取占输入 {cacheHitRate}%</span>
              </div>
              <div class="metric-val tabular">
                {formatNumber(cacheTokens)}
              </div>
              <div class="metric-foot">
                <span>读 {formatNumber(local?.cache_read_tokens)} · 写 {formatNumber(local?.cache_write_tokens)}</span>
              </div>
            </article>
          </div>

          <!-- Data Precision Integrity Guardrail -->
          <div class="integrity-banner">
            <div class="integrity-left">
              <span class="integrity-item">精确记录：<strong>{formatNumber(local?.exact_records)}</strong> 笔</span>
              <span class="integrity-item">聚合粗粒度：<strong>{formatNumber(local?.aggregate_records)}</strong> 笔</span>
              <span class="integrity-item">显式估算：<strong>{formatNumber(local?.estimated_records)}</strong> 笔</span>
              <span class="integrity-item">精度未知：<strong>{formatNumber(local?.unknown_precision_records)}</strong> 笔</span>
              <span class="integrity-item">已计价：<strong>{formatNumber(local?.priced_records)}</strong> 笔</span>
            </div>
            <div class="integrity-right">
              隔离保护：未归属用量 {formatNumber(snapshot.unassigned.total_tokens)} Token，避免跨设备重复累加
            </div>
          </div>

          <!-- Trend Chart & Tool Rankings (Two Column Grid) -->
          <div class="two-col-grid">
            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>Token 使用趋势</h2>
                  <p>按本机时区自然日汇集（时间未知记录不进入日趋势）</p>
                </div>
                <span class="panel-tag">{snapshot.daily.length} 天样本</span>
              </div>

              {#if chartValues.length > 0}
                <div class="chart-wrap">
                  <svg class="trend-svg" viewBox="0 0 800 180" role="img" aria-label="每日 Token 使用曲线">
                    <defs>
                      <linearGradient id="trendGradient" x1="0" x2="0" y1="0" y2="1">
                        <stop offset="0%" stop-color="var(--brand-primary)" stop-opacity="0.32" />
                        <stop offset="100%" stop-color="var(--brand-primary)" stop-opacity="0" />
                      </linearGradient>
                    </defs>
                    {#each [30, 75, 120, 160] as y}
                      <line x1="20" x2="780" y1={y} y2={y} stroke="var(--chart-grid)" stroke-dasharray="4 6" />
                    {/each}
                    {#if chartPoints.length > 1}
                      <path
                        d={`${chartPath} L ${chartPoints[chartPoints.length - 1].x} 165 L ${chartPoints[0].x} 165 Z`}
                        fill="url(#trendGradient)"
                      />
                      <path
                        d={chartPath}
                        fill="none"
                        stroke="var(--brand-primary)"
                        stroke-width="2.5"
                        stroke-linecap="round"
                        stroke-linejoin="round"
                      />
                      {#each chartPoints as point}
                        <circle cx={point.x} cy={point.y} r="3.5" fill="var(--brand-primary)" />
                      {/each}
                    {/if}
                  </svg>
                  <div class="chart-axis-labels">
                    <span>{chartValues[0]?.date ?? ''}</span>
                    <span>{chartValues[chartValues.length - 1]?.date ?? ''}</span>
                  </div>
                </div>
              {:else}
                <div class="empty-box">当前时间范围暂无可绘制的用量记录</div>
              {/if}
            </section>

            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>工具用量分布</h2>
                  <p>按工具归属汇总本机真实用量</p>
                </div>
                <button class="panel-tag" onclick={() => (activeTab = 'tools')}>查看明细 →</button>
              </div>

              {#if snapshot.by_tool.length > 0}
                <div class="rank-list">
                  {#each snapshot.by_tool.slice(0, 5) as tool, idx}
                    <div class="rank-item">
                      <div class="rank-row">
                        <span class="rank-title">
                          <span class="rank-num">0{idx + 1}</span>
                          {tool.label}
                        </span>
                        <span class="rank-value tabular">{formatNumber(tool.total_tokens)}</span>
                      </div>
                      <div class="rank-track">
                        <div class="rank-fill" style={`width: ${percentOf(tool.total_tokens, topToolTotal)}%`}></div>
                      </div>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">暂无工具消耗记录</div>
              {/if}
            </section>
          </div>

        <!-- TAB 2: MULTI-DEVICE (多设备网络) -->
        {:else if activeTab === 'devices'}
          <div class="page-header">
            <h1>多设备网络</h1>
            <p>基于自建私有 Hub 的去中心化同步状态；严格限定 device_local 物理单机归属</p>
          </div>

          <!-- Cross-device Summary -->
          <div class="device-summary-strip">
            <div class="summary-pill-card">
              <span>全设备 Token 总计</span>
              <strong class="tabular">{formatNumber((local?.total_tokens ?? 0) + snapshot.remote.total_tokens)}</strong>
            </div>
            <div class="summary-pill-card">
              <span>本机设备用量</span>
              <strong class="tabular">{formatNumber(local?.total_tokens)}</strong>
            </div>
            <div class="summary-pill-card">
              <span>远端设备缓存</span>
              <strong class="tabular">{formatNumber(snapshot.remote.total_tokens)}</strong>
            </div>
          </div>

          <!-- Device Topology Matrix -->
          <div class="panel-head">
            <div>
              <h2>设备拓扑与在线节点</h2>
              <p>最近活跃代表最后一次访问 Hub 的时间戳；非持久心跳长连接</p>
            </div>
            <span class="panel-tag">{remoteDevices.length + 1} 台注册设备</span>
          </div>

          <div class="device-matrix-grid">
            <!-- This Machine Card -->
            <article class="device-matrix-card this-machine">
              <div class="device-card-top">
                <div class="device-avatar-wrap">
                  <div class="device-os-icon">{@html icons.windows}</div>
                  <div class="device-name-block">
                    <strong>{snapshot.identity?.display_name ?? '本机设备'}</strong>
                    <small>本机节点 · {snapshot.identity?.device_id ?? '未分配'}</small>
                  </div>
                </div>
                <span class="precision-chip exact">THIS MACHINE</span>
              </div>
              <div class="device-token-total">
                <span>本机 Token 消耗</span>
                <strong class="tabular">{formatNumber(local?.total_tokens)}</strong>
              </div>
              <div class="metric-foot">
                <span>时区：{snapshot.time_zone} · DB：v{snapshot.schema_version}</span>
              </div>
            </article>

            <!-- Remote Devices Cards -->
            {#each remoteDevices as dev}
              {#if dev.device_id !== snapshot.identity?.device_id}
                <article class="device-matrix-card">
                  <div class="device-card-top">
                    <div class="device-avatar-wrap">
                      <div class="device-os-icon">
                        {#if dev.os_name === 'macos'}
                          {@html icons.macos}
                        {:else if dev.os_name === 'linux'}
                          {@html icons.linux}
                        {:else}
                          {@html icons.windows}
                        {/if}
                      </div>
                      <div class="device-name-block">
                        <strong>{dev.display_name}</strong>
                        <small>{dev.device_id}</small>
                      </div>
                    </div>
                    <span class="precision-chip quota">{dev.revoked_at_ms ? '已注销' : 'HUB CACHED'}</span>
                  </div>
                  <div class="device-token-total">
                    <span>远端 Token 缓存</span>
                    <strong class="tabular">
                      {formatNumber(snapshot.remote_by_device.find((d) => d.owner_device_id === dev.device_id)?.total_tokens ?? 0)}
                    </strong>
                  </div>
                  <div class="metric-foot">
                    <span>{dev.os_name} · v{dev.app_version} · 上报：{relativeTime(dev.last_seen_ms)}</span>
                  </div>
                </article>
              {/if}
            {/each}
          </div>

          <!-- Single Device Drilldown -->
          <section class="panel-card" style="margin-bottom: 24px;">
            <div class="panel-head">
              <div>
                <h2>远端设备维度透视</h2>
                <p>穿透查看指定远端设备的工具、模型与项目贡献分布</p>
              </div>
              <label class="field-label" style="flex-direction: row; align-items: center; gap: 8px;">
                <span>选择设备：</span>
                <select class="field-select" bind:value={remoteSelectedDevice}>
                  <option value="all">全部远端设备汇总</option>
                  {#each selectableRemoteDevices as d}
                    <option value={d.device_id}>{d.display_name} ({d.device_id})</option>
                  {/each}
                </select>
              </label>
            </div>

            <div class="two-col-grid" style="margin-bottom: 0;">
              <div>
                <h3 style="font-size: 12px; margin-top: 0; color: var(--text-secondary);">远端工具分布</h3>
                {#if remoteToolRows.length > 0}
                  <div class="rank-list">
                    {#each remoteToolRows.slice(0, 6) as row}
                      <div class="rank-item">
                        <div class="rank-row">
                          <span>{row.label}</span>
                          <strong class="tabular">{formatNumber(row.total_tokens)}</strong>
                        </div>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="empty-box">暂无远端工具记录</div>
                {/if}
              </div>

              <div>
                <h3 style="font-size: 12px; margin-top: 0; color: var(--text-secondary);">远端模型分布</h3>
                {#if remoteModelRows.length > 0}
                  <div class="rank-list">
                    {#each remoteModelRows.slice(0, 6) as row}
                      <div class="rank-item">
                        <div class="rank-row">
                          <span>{row.label}</span>
                          <strong class="tabular">{formatNumber(row.total_tokens)}</strong>
                        </div>
                      </div>
                    {/each}
                  </div>
                {:else}
                  <div class="empty-box">暂无远端模型记录</div>
                {/if}
              </div>
            </div>
          </section>

          <!-- Hub Pairing Management -->
          <section class="panel-card">
            <div class="panel-head">
              <div>
                <h2>Hub 连接与设备配对</h2>
                <p>凭证严格安全存储在操作系统原生安全密码库，SQLite 零残留</p>
              </div>
              <span class="precision-chip exact">KEYRING PROTECTED</span>
            </div>

            <div class="form-grid">
              <label class="field-label">
                <span>Hub 服务节点 (HTTPS 或 127.0.0.1)</span>
                <input class="field-input" type="url" bind:value={hubUrl} placeholder="https://hub.example.com" />
              </label>
              <label class="field-label">
                <span>一次性安全配对码 (52 位)</span>
                <input class="field-input" type="password" bind:value={pairingCode} placeholder="粘贴由 Hub 生成的配对码" />
              </label>
              <label class="field-label">
                <span>本机在 Hub 中的别名</span>
                <input class="field-input" type="text" bind:value={hubDisplayName} placeholder="例如：办公室 Windows" />
              </label>
            </div>

            <div style="display: flex; gap: 10px; align-items: center; flex-wrap: wrap;">
              <button
                class="btn-primary"
                disabled={hubBusy || !hubUrl.trim() || !pairingCode.trim()}
                onclick={() => void handlePairWithHub()}
              >
                {@html icons.shield} 配对此设备
              </button>
              <button class="btn-secondary" disabled={hubBusy || !hubUrl.trim()} onclick={() => void handleRefreshHubDevices()}>
                {@html icons.refresh} 刷新设备列表
              </button>
              {#if hubPaired}
                <button class="btn-secondary" disabled={hubBusy} onclick={() => void handleRenameHubDevice()}>
                  {@html icons.edit} 重命名本机
                </button>
                <button class="btn-secondary" disabled={hubBusy} onclick={() => void handleRotateHubCredential()}>
                  {@html icons.key} 轮换安全凭证
                </button>
                <button class="btn-danger" disabled={hubBusy} onclick={() => void handleDisconnectHub()}>
                  {@html icons.unlink} 断开连接
                </button>
              {/if}
            </div>

            {#if hubError}
              <p style="color: var(--color-danger); margin: 12px 0 0; font-size: 12px;">{hubError}</p>
            {/if}
            {#if hubSuccess}
              <p style="color: var(--color-exact); margin: 12px 0 0; font-size: 12px;">{hubSuccess}</p>
            {/if}
          </section>

        <!-- TAB 3: TOOLS ANALYSIS (工具分析) -->
        {:else if activeTab === 'tools'}
          <div class="page-header">
            <h1>工具分析</h1>
            <p>已接入 AI 工具的原生解析器监控、扫描健康状态与用量汇总</p>
          </div>

          <section class="panel-card" style="margin-bottom: 20px;">
            <div class="panel-head">
              <h2>工具用量详细矩阵</h2>
              <span class="panel-tag">{snapshot.by_tool.length} 个本地来源</span>
            </div>
            {#if snapshot.by_tool.length > 0}
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>工具名称</th>
                      <th>Token 总量</th>
                      <th>用量记录数</th>
                      <th>估算成本</th>
                      <th>全机占比</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each snapshot.by_tool as tool}
                      <tr>
                        <td><strong>{tool.label}</strong></td>
                        <td class="tabular">{formatNumber(tool.total_tokens)}</td>
                        <td class="tabular">{formatNumber(tool.record_count)}</td>
                        <td class="tabular">{formatCost(tool.estimated_cost_microusd)}</td>
                        <td>
                          <div class="rank-track" style="width: 100px;">
                            <div class="rank-fill" style={`width: ${percentOf(tool.total_tokens, local?.total_tokens ?? 0)}%`}></div>
                          </div>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="empty-box">暂无工具统计记录</div>
            {/if}
          </section>

          <!-- Collector Health -->
          <section class="panel-card">
            <div class="panel-head">
              <h2>采集器健康度与检查点</h2>
              <p>各工具最近扫描与完整事务写入时间戳</p>
            </div>
            {#if snapshot.source_health.length > 0}
              <div class="form-grid">
                {#each snapshot.source_health as source}
                  <div class="metric-card">
                    <div class="metric-head">
                      <strong>{source.tool_id}</strong>
                      <span class="precision-chip" class:exact={source.status === 'complete'} class:danger={source.status === 'degraded'}>
                        {source.status === 'complete' ? '正常' : source.status === 'partial' ? '部分' : '异常'}
                      </span>
                    </div>
                    <div style="margin-top: 8px; font-size: 11.5px; color: var(--text-secondary);">
                      <div>最近扫描：{relativeTime(source.last_scan_ms)}</div>
                      <div>完整扫描：{relativeTime(source.last_complete_scan_ms)}</div>
                      <div>来源文件数：{source.source_count}</div>
                    </div>
                  </div>
                {/each}
              </div>
            {:else}
              <div class="empty-box">暂无来源检查点记录</div>
            {/if}
          </section>

        <!-- TAB 4: MODELS ANALYSIS (模型分析) -->
        {:else if activeTab === 'models'}
          <div class="page-header">
            <div style="display: flex; justify-content: space-between; align-items: flex-end;">
              <div>
                <h1>模型排行与用量分析</h1>
                <p>严格遵循 Unicode NFC 规范化；真实来源未提供之模型不进行主观猜测</p>
              </div>
              <div class="range-capsule">
                <button class="range-btn" class:selected={modelSortBy === 'tokens'} onclick={() => (modelSortBy = 'tokens')}>
                  按 Token 排序
                </button>
                <button class="range-btn" class:selected={modelSortBy === 'cost'} onclick={() => (modelSortBy = 'cost')}>
                  按 估算成本 排序
                </button>
              </div>
            </div>
          </div>

          <section class="panel-card">
            {#if sortedModels.length > 0}
              <div class="rank-list">
                {#each sortedModels as item, index}
                  <div class="rank-item" style="padding: 10px 0; border-bottom: 1px solid var(--border-subtle);">
                    <div class="rank-row">
                      <span class="rank-title">
                        <span class="rank-num tabular">{String(index + 1).padStart(2, '0')}</span>
                        <strong>{item.label}</strong>
                      </span>
                      <span class="rank-value tabular">
                        {formatNumber(item.total_tokens)} Token
                        <span style="font-size: 11px; color: var(--text-muted); margin-left: 8px;">
                          ({formatCost(item.estimated_cost_microusd)})
                        </span>
                      </span>
                    </div>
                    <div class="rank-track">
                      <div class="rank-fill" style={`width: ${percentOf(item.total_tokens, topModelTotal)}%`}></div>
                    </div>
                    <div class="metric-foot">
                      <span>用量记录 {formatNumber(item.record_count)} 笔 · 精确 {formatNumber(item.exact_records)} · 聚合 {formatNumber(item.aggregate_records)} · 估算 {formatNumber(item.estimated_records)}</span>
                    </div>
                  </div>
                {/each}
              </div>
            {:else}
              <div class="empty-box">暂无模型记录</div>
            {/if}
          </section>

          <section class="panel-card" style="margin-top: 20px;">
            <div class="panel-head">
              <div>
                <h2>模型调用量</h2>
                <p>按来源展示可验证的响应次数；不同来源的时间范围会分别标注</p>
              </div>
              <span class="precision-chip quota">响应活动</span>
            </div>
            {#if snapshot.model_activity.length > 0}
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr><th>来源</th><th>模型</th><th>调用量</th><th>统计范围</th></tr>
                  </thead>
                  <tbody>
                    {#each snapshot.model_activity as activity}
                      <tr>
                        <td>{activity.tool_id === 'codex' ? 'Codex' : activity.tool_id === 'antigravity' ? 'Antigravity' : activity.tool_id}</td>
                        <td><strong>{activity.model_id === 'unknown-model' ? '未识别模型' : activity.model_id}</strong></td>
                        <td class="tabular">{formatNumber(activity.response_count)}</td>
                        <td>{activity.time_scope === 'selected_range' ? '当前时间范围' : '来源累计'}</td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="empty-box">暂无可验证的模型响应次数；历史聚合 Token 记录不会被当作调用次数。</div>
            {/if}
          </section>

        {:else if activeTab === 'sessions'}
          <div class="page-header">
            <h1>会话分析</h1>
            <p>按工具、模型和项目筛选；可搜索会话名称、ID、模型或项目标识。仅查询本机统计，不读取聊天正文。</p>
          </div>

          <section class="panel-card">
            <div class="panel-head">
              <div>
                <h2>会话 Token 使用</h2>
                <p>{#if sessionPage}显示 {formatNumber(sessionPageStart)}–{formatNumber(sessionPageEnd)} / {formatNumber(sessionPage.total)} 个匹配会话{:else if sessionLoading}正在查询会话…{:else}当前范围：{rangeLabel}{/if}</p>
              </div>
              <span class="panel-tag">每页 {sessionPage?.limit ?? 50}</span>
            </div>

            <div class="session-filter-panel">
              <div class="session-filter-row">
                <label class="session-filter-field session-search-field">
                  <span>搜索会话</span>
                  <input
                    type="search"
                    bind:value={sessionSearch}
                    oninput={scheduleSessionAnalysis}
                    placeholder="名称、会话 ID、模型或项目"
                    aria-label="搜索会话名称、会话 ID、模型或项目"
                  />
                </label>
                <label class="session-filter-field">
                  <span>工具</span>
                  <select bind:value={sessionToolFilter} onchange={scheduleSessionAnalysis} aria-label="按工具筛选会话">
                    <option value="">全部工具</option>
                    {#each sessionFilterOptions.tools as option}
                      <option value={option.key}>{option.label} · {formatNumber(option.session_count)}</option>
                    {/each}
                  </select>
                </label>
                <label class="session-filter-field">
                  <span>模型</span>
                  <select bind:value={sessionModelFilter} onchange={scheduleSessionAnalysis} aria-label="按模型筛选会话">
                    <option value="">全部模型</option>
                    {#each sessionFilterOptions.models as option}
                      <option value={option.key}>{option.label} · {formatNumber(option.session_count)}</option>
                    {/each}
                  </select>
                </label>
                <label class="session-filter-field">
                  <span>项目</span>
                  <select bind:value={sessionProjectFilter} onchange={scheduleSessionAnalysis} aria-label="按项目筛选会话">
                    <option value="">全部项目</option>
                    {#each sessionFilterOptions.projects as option}
                      <option value={option.key}>{option.label} · {formatNumber(option.session_count)}</option>
                    {/each}
                  </select>
                </label>
                <button class="btn-secondary session-clear-btn" disabled={!hasSessionFilters} onclick={clearSessionFilters}>清除筛选</button>
              </div>
              <div class="session-filter-meta">
                <span>多模型或多项目会话按“曾使用该项”匹配；Token 显示整段会话合计。</span>
                <span class="session-filter-count" role="status">{sessionLoading ? '正在更新结果…' : `匹配 ${formatNumber(sessionPage?.total ?? 0)} 个会话`}</span>
              </div>
            </div>

            {#if sessionActionError}
              <div class="session-action-notice error" role="alert">{sessionActionError}</div>
            {:else if sessionActionMessage}
              <div class="session-action-notice success" role="status">{sessionActionMessage}</div>
            {/if}
            {#if sessionError}
              <div class="session-action-notice error" role="alert">{sessionError}</div>
            {/if}

            {#if sessionPage && sessionPage.sessions.length > 0}
              <div class="table-wrap">
                <table class="session-table">
                  <thead>
                    <tr>
                      <th>会话</th><th>来源</th><th>模型使用</th><th>Token 使用</th><th>精度与记录</th><th>响应次数</th><th>最近事件</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each sessionPage.sessions as session}
                      <tr>
                        <td>
                          <div class="session-name" title={session.display_name ?? undefined}>{session.display_name ?? '未命名会话'}</div>
                          <code title={session.session_id}>…{session.session_id.slice(-12)}</code>
                          <div class="session-facet-list" aria-label="项目分类">
                            {#if session.project_ids.length > 0}
                              {#each session.project_ids.slice(0, 2) as projectId}
                                <span class="session-facet-chip" title={sessionProjectLabel(projectId)}>{sessionProjectLabel(projectId)}</span>
                              {/each}
                              {#if session.project_ids.length > 2}<span class="session-facet-chip muted">+{session.project_ids.length - 2}</span>{/if}
                            {:else}
                              <span class="session-facet-chip muted">未关联项目</span>
                            {/if}
                          </div>
                          <div class="session-row-actions">
                            {#if pendingSessionDeleteKey === sessionActionKey(session)}
                              <div class="session-action-confirm">
                                <span>仅影响本机统计</span>
                                <button
                                  class="btn-danger session-action-btn"
                                  disabled={deletingSessionKey !== ''}
                                  onclick={() => void confirmSessionDelete(session)}
                                >
                                  {deletingSessionKey === sessionActionKey(session) ? '删除中…' : '确认删除'}
                                </button>
                                <button
                                  class="btn-secondary session-action-btn"
                                  disabled={deletingSessionKey !== ''}
                                  onclick={cancelSessionDelete}
                                >取消</button>
                              </div>
                            {:else}
                              <button
                                class="btn-danger session-delete-btn"
                                aria-label={`从本机统计删除会话 ${session.display_name ?? session.session_id}`}
                                title="只删除 TokenScope 本机统计，不删除原应用聊天"
                                disabled={refreshing || deletingSessionKey !== ''}
                                onclick={() => requestSessionDelete(session)}
                              >删除</button>
                            {/if}
                          </div>
                        </td>
                        <td>{session.tool_id === 'codex' ? 'Codex' : session.tool_id === 'claude' ? 'Claude' : session.tool_id === 'antigravity' ? 'Antigravity' : session.tool_id === 'deepseek-harness' ? 'DeepSeek Harness' : session.tool_id}</td>
                        <td class="session-models">
                          {#if session.models.length > 0}
                            {#each session.models.slice(0, 4) as model}
                              <div class="session-model-row">
                                <span class="session-model-name" title={model.model_id ?? '未识别模型'}>{model.model_id ?? '未识别模型'}</span>
                                <span class="tabular">{formatNumber(model.total_tokens)}</span>
                              </div>
                            {/each}
                            {#if session.models.length > 4}
                              <div class="session-precision">另有 {session.models.length - 4} 个模型</div>
                            {/if}
                          {:else}
                            <span class="text-muted">来源未提供</span>
                          {/if}
                        </td>
                        <td class="tabular"><strong>{formatNumber(session.total_tokens)}</strong></td>
                        <td>
                          <span>{formatNumber(session.usage_record_count)} 笔记录</span>
                          <div class="session-precision">
                            精确 {formatNumber(session.exact_records)} · 聚合 {formatNumber(session.aggregate_records)} · 估算 {formatNumber(session.estimated_records)}
                            {#if session.unknown_precision_records > 0} · 未知 {formatNumber(session.unknown_precision_records)}{/if}
                            {#if session.untimed_records > 0} · 无时间 {formatNumber(session.untimed_records)}{/if}
                          </div>
                        </td>
                        <td class="tabular">
                          {#if session.known_response_count !== null}
                            {formatNumber(session.known_response_count)}
                            <div class="session-precision">{session.tool_id === 'antigravity' ? '来源累计' : '当前范围'}{#if session.unknown_response_count} · 未识别 {formatNumber(session.unknown_response_count)}{/if}</div>
                          {:else}
                            <span class="text-muted">来源未提供</span>
                          {/if}
                        </td>
                        <td>
                          {session.last_event_at_ms === null ? '事件时间未知' : relativeTime(session.last_event_at_ms)}
                          {#if session.tool_id === 'antigravity' && session.activity_observed_at_ms !== null}
                            <div class="session-precision">来源检查 {relativeTime(session.activity_observed_at_ms)}</div>
                          {/if}
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
              <div class="session-pagination">
                <span>Antigravity 响应次数为来源累计值；Token 用量按当前日期范围汇总。</span>
                <div aria-label="会话分页">
                  <button class="btn-secondary" disabled={sessionOffset === 0 || refreshing} onclick={() => handleSessionPage(Math.max(0, sessionOffset - (sessionPage?.limit ?? 50)))}>上一页</button>
                  <button class="btn-secondary" disabled={!sessionPage || sessionOffset + sessionPage.limit >= sessionPage.total || refreshing} onclick={() => handleSessionPage(sessionOffset + (sessionPage?.limit ?? 50))}>下一页</button>
                </div>
              </div>
            {:else if sessionLoading && !sessionAnalysis}
              <div class="session-empty-state" role="status">正在读取本机会话统计…</div>
            {:else if sessionPage && sessionPage.total === 0 && hasSessionFilters}
              <div class="session-empty-state">
                <strong>没有匹配的会话</strong>
                <p>调整搜索内容或分类条件，或清除筛选查看当前范围的全部会话。</p>
                <button class="btn-secondary" onclick={clearSessionFilters}>清除筛选</button>
              </div>
            {:else if sessionPage}
              <div class="session-empty-state">
                <strong>这个时间范围内没有会话用量</strong>
                <p>切换到“全部”可查看历史记录，以及来源未提供事件时间的用量。</p>
                <button class="btn-secondary" disabled={range === 'all'} onclick={() => handleSelectRange('all')}>查看全部日期</button>
              </div>
            {:else if sessionError}
              <div class="session-empty-state">
                <strong>会话数据暂时无法读取</strong>
                <p>请重试查询；本机统计数据不会因此被更改。</p>
                <button class="btn-secondary" disabled={sessionLoading} onclick={() => void loadSessionAnalysis()}>重试</button>
              </div>
            {:else}
              <div class="session-empty-state" role="status">选择“会话分析”后会读取当前日期范围的会话。</div>
            {/if}
          </section>

        <!-- TAB 5: PROJECTS (项目分析) -->
        {:else if activeTab === 'projects'}
          <div class="page-header">
            <h1>项目分析</h1>
            <p>基于脱敏项目标识符归集；源码路径与仓库名留在本地，绝不上传</p>
          </div>

          <section class="panel-card">
            {#if snapshot.by_project.length > 0}
              <div class="table-wrap">
                <table>
                  <thead>
                    <tr>
                      <th>项目名称</th>
                      <th>Token 消耗量</th>
                      <th>用量记录数</th>
                      <th>全机份额</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each snapshot.by_project as prj}
                      <tr>
                        <td>{prj.label}</td>
                        <td class="tabular">{formatNumber(prj.total_tokens)}</td>
                        <td class="tabular">{formatNumber(prj.record_count)}</td>
                        <td>
                          <div class="rank-track" style="width: 120px;">
                            <div class="rank-fill" style={`width: ${percentOf(prj.total_tokens, local?.total_tokens ?? 0)}%`}></div>
                          </div>
                        </td>
                      </tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="empty-box">当前本地来源尚未解析出项目标识</div>
            {/if}
          </section>

        <!-- PROVIDER QUOTAS AND ACTIVITY -->
        {:else if activeTab === 'quota'}
          <div class="page-header">
            <h1>额度与活动</h1>
            <p>Codex 通过本机 Codex app-server 读取；Antigravity 通过仅限回环地址的 RPC 读取。额度、响应次数与 Token 用量分别统计。</p>
          </div>

          <section class="panel-card quota-section">
            <div class="panel-head">
              <div>
                <h2>Codex 可用额度</h2>
                <p>由 Codex 本地 app-server 返回的计划限额窗口</p>
              </div>
              <span class="precision-chip quota">CODEX APP-SERVER</span>
            </div>
            {#if snapshot.codex_quota.length > 0}
              <div class="quota-grid">
                {#each snapshot.codex_quota as q}
                  <article class="quota-item-card">
                    <div class="quota-item-head">
                      <span class="quota-pool-name">Codex</span>
                      <span class="panel-tag">{quotaWindowLabel(q.window_duration_minutes)}</span>
                    </div>
                    <div class="quota-gauge-val tabular">{formatPercent(q.remaining_basis_points)}</div>
                    <div class="rank-track quota-track" aria-label="可用额度百分比">
                      <div class="rank-fill" style={`width: ${q.remaining_basis_points === null ? 0 : q.remaining_basis_points / 100}%; background-color: var(--color-quota);`}></div>
                    </div>
                    <div class="quota-reset-countdown">{@html icons.quota}<span>{formatCountdown(q.reset_at_ms)}</span></div>
                    <div class="quota-observed" class:stale={quotaIsStale(q.observed_at_ms)}>
                      {quotaIsStale(q.observed_at_ms) ? '可能已过期' : '最近观测'} · {relativeTime(q.observed_at_ms)}
                    </div>
                  </article>
                {/each}
              </div>
            {:else}
              <div class="empty-box">尚无 Codex 额度快照。请确认本机 Codex CLI 已安装并登录，然后刷新。</div>
            {/if}
          </section>

          <section class="panel-card quota-section">
            <div class="panel-head">
              <div>
                <h2>Antigravity 可用额度</h2>
                <p>来自本机 127.0.0.1 RPC；未知值保留为“未知”，不按 0 处理</p>
              </div>
              <span class="precision-chip quota">LOOPBACK RPC</span>
            </div>
            {#if snapshot.antigravity_quota.length > 0}
              <div class="quota-grid">
                {#each snapshot.antigravity_quota as q}
                  <article class="quota-item-card">
                    <div class="quota-item-head">
                      <span class="quota-pool-name">{q.pool_id.replaceAll('_', ' ')}</span>
                      <span class="panel-tag">{antigravityWindowLabel(q.window_id)}</span>
                    </div>
                    <div class="quota-gauge-val tabular">{formatPercent(q.remaining_basis_points)}</div>
                    <div class="rank-track quota-track" aria-label="可用额度百分比">
                      <div class="rank-fill" style={`width: ${q.remaining_basis_points === null ? 0 : q.remaining_basis_points / 100}%; background-color: var(--color-quota);`}></div>
                    </div>
                    <div class="quota-reset-countdown">{@html icons.quota}<span>{formatCountdown(q.reset_at_ms)}</span></div>
                    <div class="quota-observed" class:stale={quotaIsStale(q.observed_at_ms)}>
                      {quotaIsStale(q.observed_at_ms) ? '可能已过期' : '最近观测'} · {relativeTime(q.observed_at_ms)}
                    </div>
                  </article>
                {/each}
              </div>
            {:else}
              <div class="empty-box">尚无 Antigravity 额度快照。请确认 Antigravity IDE 或 CLI 正在运行，然后刷新。</div>
            {/if}
          </section>

          <section class="panel-card quota-section">
            <div class="panel-head">
              <div>
                <h2>Antigravity Token 用量</h2>
                <p>仅显示来源提供的生成 Token 计数；系统提示计数若存在则列为聚合，额度不会换算成 Token</p>
              </div>
              <span class="precision-chip exact">本地用量事实</span>
            </div>
            {#if antigravityUsage}
              <div class="device-summary-strip quota-summary-strip">
                <div class="summary-pill-card"><span>Token 总量</span><strong class="tabular">{formatNumber(antigravityUsage.total_tokens)}</strong></div>
                <div class="summary-pill-card"><span>来源记录</span><strong class="tabular">{formatNumber(antigravityUsage.record_count)}</strong></div>
                <div class="summary-pill-card"><span>精确 / 聚合</span><strong class="tabular">{formatNumber(antigravityUsage.exact_records)} / {formatNumber(antigravityUsage.aggregate_records)}</strong></div>
                <div class="summary-pill-card"><span>显式估算</span><strong class="tabular">{formatNumber(antigravityUsage.estimated_records)}</strong></div>
              </div>
              {#if antigravityUsage.untimed_records > 0}
                <p class="usage-note">{formatNumber(antigravityUsage.untimed_records)} 条记录没有可靠事件时间，因此不会进入所选日期范围和日趋势。</p>
              {/if}
            {:else}
              <div class="empty-box">当前范围没有可计量的 Antigravity Token 记录。不会根据响应次数或额度推算用量。</div>
            {/if}
          </section>

          <section class="panel-card quota-section">
            <div class="panel-head">
              <div>
                <h2>模型响应次数</h2>
                <p>Antigravity 来源累计值；它与 Token 计量独立</p>
              </div>
              <span class="panel-tag">活动账本</span>
            </div>
            <div class="device-summary-strip quota-summary-strip">
              <div class="summary-pill-card"><span>已识别模型响应</span><strong class="tabular">{formatNumber(snapshot.antigravity_activity.known_turns)}</strong></div>
              <div class="summary-pill-card"><span>未识别响应</span><strong class="tabular">{formatNumber(snapshot.antigravity_activity.unknown_turns)}</strong></div>
              <div class="summary-pill-card"><span>来源会话数</span><strong class="tabular">{formatNumber(snapshot.antigravity_activity.conversations)}</strong></div>
            </div>
            {#if snapshot.model_activity.some((item) => item.tool_id === 'antigravity')}
              <div class="table-wrap activity-table">
                <table>
                  <thead><tr><th>模型</th><th>响应次数</th><th>统计范围</th></tr></thead>
                  <tbody>
                    {#each snapshot.model_activity.filter((item) => item.tool_id === 'antigravity') as item}
                      <tr><td><strong>{item.model_id}</strong></td><td class="tabular">{formatNumber(item.response_count)}</td><td>来源累计</td></tr>
                    {/each}
                  </tbody>
                </table>
              </div>
            {:else}
              <div class="empty-box">暂无已识别模型的活动记录</div>
            {/if}
          </section>

        <!-- TAB 7: SETTINGS (系统设置) -->
        {:else if activeTab === 'settings'}
          <div class="page-header">
            <h1>系统设置与偏好</h1>
            <p>控制系统常驻生命周期、主题外观与本地采集行为</p>
          </div>

          <!-- Theme Settings -->
          <section class="panel-card" style="margin-bottom: 20px;">
            <div class="panel-head">
              <h2>界面主题外观</h2>
              <span class="panel-tag">Linear Craft</span>
            </div>
            <div style="display: flex; gap: 12px;">
              <button
                class="btn-secondary"
                class:selected={currentTheme === 'dark'}
                onclick={() => handleSwitchTheme('dark')}
              >
                {@html icons.moon} 深色模式 (Obsidian)
              </button>
              <button
                class="btn-secondary"
                class:selected={currentTheme === 'light'}
                onclick={() => handleSwitchTheme('light')}
              >
                {@html icons.sun} 浅色模式 (Soft Zinc)
              </button>
              <button
                class="btn-secondary"
                class:selected={currentTheme === 'system'}
                onclick={() => handleSwitchTheme('system')}
              >
                {@html icons.laptop} 跟随操作系统
              </button>
            </div>
          </section>

          <!-- Autostart & Lifecycle -->
          <section class="panel-card" style="margin-bottom: 20px;">
            <div class="panel-head">
              <div>
                <h2>系统登录时自启动 (--background)</h2>
                <p>遵循 ADR-0005 规范：启动后仅在系统托盘静默常驻，不创建 WebView，内存占用 ≤25 MiB</p>
              </div>
              <button
                class="btn-primary"
                disabled={autostartBusy || !runtimeAvailable}
                onclick={() => void handleToggleAutostart()}
              >
                {autostartBusy ? '处理中…' : autostartEnabled ? '已启用 · 点击关闭' : '未启用 · 点击开启'}
              </button>
            </div>
            {#if autostartError}
              <p style="color: var(--color-danger); font-size: 12px;">{autostartError}</p>
            {/if}
          </section>

          <!-- Supervisor Health Diagnostics -->
          <section class="panel-card">
            <div class="panel-head">
              <h2>本地 Agent 守护进程诊断</h2>
              <span class="precision-chip exact">{agent?.running ? 'RUNNING' : 'STOPPED'}</span>
            </div>
            <div style="font-size: 12.5px; line-height: 1.8; color: var(--text-secondary);">
              <div>进程状态：<strong>{agent?.running ? '正常运行中' : '未连接'}</strong></div>
              <div>二进制就绪：<strong>{agent?.binaryFound ? '已部署 (resources/tokenscope-agent)' : '未找到'}</strong></div>
              <div>SQLite WAL 状态：<strong>{agent?.databaseReady ? '已就绪 (tokenscope.db)' : '初始化中'}</strong></div>
              <div>桌面里程碑：<strong>{BOOTSTRAP_STAGE}</strong></div>
            </div>
          </section>
        {/if}
      {/if}
    </section>
  </main>
</div>
