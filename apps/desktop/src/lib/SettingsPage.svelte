<script lang="ts">
  import { onMount } from 'svelte';
import { BOOTSTRAP_STAGE } from '../lib/status';
  import { formatNumber } from '../lib/format';
  import type { ThemeMode } from '../lib/theme';
  import { icons } from '../lib/icons';
  import type { AgentStatus, DiagnosticReport, DiagnosticSeverity } from '../lib/types';
  import { REFRESH_INTERVAL_OPTIONS } from './settingsController';

  export let currentTheme: ThemeMode;
  export let autostartEnabled: boolean;
  export let autostartLoaded: boolean;
  export let autostartBusy: boolean;
  export let autostartError: string;
  export let runtimeAvailable: boolean;
  export let agent: AgentStatus | null;
  export let onSwitchTheme: (mode: ThemeMode) => void;
  export let onToggleAutostart: () => Promise<void>;
  export let onRefreshAutostart: () => Promise<void>;
  export let rollingNumbersEnabled = true;
  export let onToggleRollingNumbers: () => void = () => {};
  export let refreshIntervalMs = 300_000;
  export let onSetRefreshInterval: (intervalMs: number) => void = () => {};
  export let onRunDiagnostics: () => Promise<DiagnosticReport>;

  let diagnostics: DiagnosticReport | null = null;
  let diagnosticsBusy = false;
  let diagnosticsError = '';
  let diagnosticsCopied = false;

  onMount(() => { void onRefreshAutostart(); });

  const refreshOptions = REFRESH_INTERVAL_OPTIONS.map((value) => ({
    value,
    label:
      value === 0
        ? '仅事件推送'
        : value < 60_000
          ? `${value / 1000} 秒`
          : `${value / 60_000} 分钟`,
  }));

  async function runDiagnostics() {
    if (diagnosticsBusy || !runtimeAvailable) return;
    diagnosticsBusy = true;
    diagnosticsError = '';
    try {
      diagnostics = await onRunDiagnostics();
      diagnosticsCopied = false;
    } catch (error) {
      diagnosticsError = error instanceof Error ? error.message : String(error);
    } finally {
      diagnosticsBusy = false;
    }
  }

  async function copyDiagnostics() {
    if (!diagnostics) return;
    try {
      await navigator.clipboard.writeText(JSON.stringify(diagnostics, null, 2));
      diagnosticsCopied = true;
      window.setTimeout(() => (diagnosticsCopied = false), 1800);
    } catch (error) {
      diagnosticsError = `复制诊断报告失败：${error instanceof Error ? error.message : String(error)}`;
    }
  }

  function severityLabel(severity: DiagnosticSeverity): string {
    if (severity === 'error') return '错误';
    if (severity === 'warning') return '警告';
    if (severity === 'info') return '提示';
    return '正常';
  }

  function formatBytes(value: number | null | undefined): string {
    const bytes = value ?? 0;
    if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1)} GB`;
    if (bytes >= 1024 ** 2) return `${(bytes / 1024 ** 2).toFixed(1)} MB`;
    if (bytes >= 1024) return `${(bytes / 1024).toFixed(1)} KB`;
    return `${bytes} B`;
  }

  function formatTime(value: number | null | undefined): string {
    return value ? new Date(value).toLocaleString('zh-CN', { hour12: false }) : '暂无';
  }

  function shortDeviceId(value: string | null | undefined): string {
    if (!value) return '未建立';
    return value.length > 14 ? `${value.slice(0, 8)}…${value.slice(-4)}` : value;
  }

  function refreshLabel(intervalMs: number): string {
    if (intervalMs <= 0) return '仅事件推送';
    return intervalMs < 60_000 ? `${intervalMs / 1000} 秒` : `${intervalMs / 60_000} 分钟`;
  }
</script>

<div class="settings-page">
  <div class="page-header settings-header">
    <div>
      <h1>系统设置</h1>
      <p>调整外观、数据更新方式，并查看本机采集与运行状态。</p>
    </div>
  </div>

  <div class="settings-preferences">
    <section class="panel-card settings-card">
      <div class="panel-head">
        <div>
          <h2>外观与显示</h2>
          <p>选择应用主题，并控制仪表盘数字动效。</p>
        </div>
      </div>
      <div class="theme-options" role="group" aria-label="应用主题">
        <button
          class="theme-option"
          class:selected={currentTheme === 'dark'}
          aria-pressed={currentTheme === 'dark'}
          onclick={() => onSwitchTheme('dark')}
        >
          {@html icons.moon}
          <span><strong>深色</strong><small>Obsidian</small></span>
        </button>
        <button
          class="theme-option"
          class:selected={currentTheme === 'light'}
          aria-pressed={currentTheme === 'light'}
          onclick={() => onSwitchTheme('light')}
        >
          {@html icons.sun}
          <span><strong>浅色</strong><small>Soft Zinc</small></span>
        </button>
        <button
          class="theme-option"
          class:selected={currentTheme === 'system'}
          aria-pressed={currentTheme === 'system'}
          onclick={() => onSwitchTheme('system')}
        >
          {@html icons.laptop}
          <span><strong>跟随系统</strong><small>自动切换</small></span>
        </button>
      </div>
      <div class="setting-row rolling-setting">
        <div>
          <strong>核心指标数字滚动动效</strong>
          <p>数值更新时显示平滑过渡动画。</p>
        </div>
        <button
          class="btn-secondary"
          class:selected={rollingNumbersEnabled}
          aria-pressed={rollingNumbersEnabled}
          onclick={onToggleRollingNumbers}
        >
          {rollingNumbersEnabled ? '已开启' : '已关闭'}
        </button>
      </div>
    </section>

    <section class="panel-card settings-card">
      <div class="panel-head">
        <div>
          <h2>数据更新</h2>
          <p>设置仪表盘自动拉取数据的频率。</p>
        </div>
        <span class="precision-chip aggregate">{refreshLabel(refreshIntervalMs)}</span>
      </div>
      <div class="refresh-options" role="group" aria-label="数据更新速率">
        {#each refreshOptions as option}
          <button
            class="btn-secondary"
            class:selected={refreshIntervalMs === option.value}
            aria-pressed={refreshIntervalMs === option.value}
            onclick={() => onSetRefreshInterval(option.value)}
          >
            {option.label}
          </button>
        {/each}
      </div>
      <p class="setting-note">仅事件推送模式下，数据只会在收到更新通知时刷新。</p>
    </section>
  </div>

  <section class="panel-card settings-card settings-section">
    <div class="autostart-row">
      <div class="autostart-copy">
        <h2>开机自启动</h2>
        <p>登录系统后在后台启动采集器，默认不显示窗口。</p>
      </div>
      <div class="autostart-control">
        <span id="autostart-status" class="autostart-status" aria-live="polite">
          {!runtimeAvailable ? '仅桌面端可用' : autostartBusy ? '处理中…' : !autostartLoaded ? '状态未知' : autostartEnabled ? '已开启' : '已关闭'}
        </span>
        <button
          type="button"
          class="autostart-switch"
          role="switch"
          aria-label="开机自启动"
          aria-describedby="autostart-status"
          aria-checked={autostartEnabled}
          disabled={autostartBusy || !autostartLoaded || !runtimeAvailable}
          onclick={() => void onToggleAutostart()}
        ><span></span></button>
      </div>
    </div>
    {#if autostartError}
      <div class="autostart-error" role="alert">
        <span>{autostartError}</span>
        <button class="btn-secondary" disabled={autostartBusy} onclick={() => void onRefreshAutostart()}>重新读取</button>
      </div>
    {/if}
  </section>

  <section class="panel-card settings-card settings-section">
    <div class="panel-head">
      <div>
        <h2>采集状态</h2>
        <p>本机采集器及其依赖组件的运行情况。</p>
      </div>
      <span class="precision-chip" class:exact={agent?.running} class:danger={!agent?.running}>
        {agent?.running ? '运行中' : '已停止'}
      </span>
    </div>
    <div class="device-summary-strip settings-status-grid">
      <div class="summary-pill-card">
        <span class="summary-label">守护进程</span>
        <div class="status-value">
          <span class="source-status-dot" class:complete={agent?.running} class:degraded={!agent?.running}></span>
          <strong>{agent?.running ? '正常运行中' : '未连接'}</strong>
        </div>
      </div>
      <div class="summary-pill-card">
        <span class="summary-label">Agent 二进制</span>
        <strong>{agent?.binaryFound ? '已部署' : '未找到'}</strong>
        <span class="summary-foot">resources/tokenscope-agent</span>
      </div>
      <div class="summary-pill-card">
        <span class="summary-label">SQLite WAL 数据库</span>
        <strong>{agent?.databaseReady ? '就绪（WAL 模式）' : '初始化中'}</strong>
        <span class="summary-foot">tokenscope.db</span>
      </div>
      <div class="summary-pill-card">
        <span class="summary-label">桌面版本里程碑</span>
        <strong class="tabular">{BOOTSTRAP_STAGE}</strong>
        <span class="summary-foot">当前版本</span>
      </div>
    </div>
  </section>

  <!-- Full Health Diagnostics -->
  <section class="panel-card diagnostic-panel settings-card settings-section">
    <div class="panel-head">
      <div>
        <h2>系统诊断</h2>
        <p>检查数据库、磁盘空间和采集组件，并生成可复制的诊断报告。</p>
      </div>
      <button
        class="btn-primary"
        disabled={diagnosticsBusy || !runtimeAvailable}
        onclick={() => void runDiagnostics()}
      >
        {diagnosticsBusy ? '诊断中…' : diagnostics ? '重新诊断' : '运行诊断'}
      </button>
    </div>

            {#if diagnosticsError}
              <p class="diagnostic-error">{diagnosticsError}</p>
            {/if}

            {#if diagnostics}
              <div class="diagnostic-summary">
                <span class="diagnostic-badge {diagnostics.overall}">{severityLabel(diagnostics.overall)}</span>
                <span>{diagnostics.checks.length} 项检查 · 生成于 {formatTime(diagnostics.generated_at_ms)}</span>
                <button class="btn-secondary diagnostic-copy" onclick={() => void copyDiagnostics()}>
                  {diagnosticsCopied ? '已复制' : '复制报告'}
                </button>
              </div>

              <div class="diagnostic-facts">
                <div class="wide">
                  <span>数据库路径</span>
                  <strong title={diagnostics.facts.database_path}>{diagnostics.facts.database_path}</strong>
                </div>
                <div><span>数据库大小</span><strong>{formatBytes(diagnostics.facts.database_bytes)}</strong></div>
                <div><span>WAL</span><strong>{formatBytes(diagnostics.facts.wal_bytes)}</strong></div>
                <div><span>Schema</span><strong>v{diagnostics.facts.schema_version ?? '—'} / v{diagnostics.facts.expected_schema_version}</strong></div>
                <div><span>设备 ID</span><strong class="mono" title={diagnostics.facts.device_id ?? ''}>{shortDeviceId(diagnostics.facts.device_id)}</strong></div>
                <div><span>有效记录</span><strong>{formatNumber(diagnostics.facts.record_count)}</strong></div>
                <div><span>累计 Token</span><strong>{formatNumber(diagnostics.facts.total_tokens)}</strong></div>
                <div><span>来源检查点</span><strong>{formatNumber(diagnostics.facts.source_count)}</strong></div>
                <div><span>最近用量</span><strong>{formatTime(diagnostics.facts.last_event_at_ms)}</strong></div>
                <div><span>磁盘剩余</span><strong>{diagnostics.facts.free_disk_bytes == null ? '未知' : formatBytes(diagnostics.facts.free_disk_bytes)}</strong></div>
                <div><span>Agent</span><strong>{diagnostics.facts.agent_running ? '运行中' : '未运行'}</strong></div>
              </div>

              <div class="diagnostic-check-list">
                {#each diagnostics.checks as check}
                  <div class="diagnostic-check {check.severity}">
                    <span class="diagnostic-dot"></span>
                    <div class="diagnostic-check-main">
                      <div class="diagnostic-check-title">
                        <strong>{check.title}</strong>
                        <span class="diagnostic-severity {check.severity}">{severityLabel(check.severity)}</span>
                      </div>
                      <p>{check.detail}</p>
                      {#if check.action}
                        <p class="diagnostic-action">建议：{check.action}</p>
                      {/if}
                    </div>
                  </div>
                {/each}
              </div>
            {:else if !diagnosticsBusy}
              <div class="empty-box">尚未运行诊断。升级后、发现统计异常或采集器停止时，建议先运行一次。</div>
            {/if}
          </section>
</div>

<style>
  .settings-page {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .settings-header {
    margin-bottom: 0;
  }

  .settings-preferences {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(280px, 0.8fr);
    gap: 14px;
  }

  .settings-card {
    min-width: 0;
    margin: 0;
  }

  .settings-card .panel-head {
    margin-bottom: 12px;
  }

  .theme-options {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    gap: 8px;
  }

  .theme-option {
    display: flex;
    align-items: center;
    gap: 9px;
    min-width: 0;
    min-height: 54px;
    padding: 8px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    text-align: left;
    transition: border-color var(--transition-fast), background-color var(--transition-fast), color var(--transition-fast);
  }

  .theme-option:hover {
    border-color: var(--border-strong);
    background: var(--bg-card-hover);
  }

  .theme-option.selected {
    border-color: var(--brand-border);
    background: var(--brand-subtle);
    color: var(--brand-primary);
  }

  .theme-option :global(svg) {
    width: 17px;
    height: 17px;
    flex: 0 0 auto;
  }

  .theme-option span {
    display: flex;
    flex-direction: column;
    min-width: 0;
    gap: 2px;
  }

  .theme-option strong {
    color: var(--text-primary);
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
  }

  .theme-option small {
    color: var(--text-muted);
    font-size: 10px;
    white-space: nowrap;
  }

  .setting-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .rolling-setting {
    margin-top: 14px;
    padding-top: 13px;
    border-top: 1px solid var(--border-subtle);
  }

  .setting-row strong {
    color: var(--text-primary);
    font-size: 12.5px;
  }

  .setting-row p,
  .setting-note {
    margin: 4px 0 0;
    color: var(--text-muted);
    font-size: 11px;
    line-height: 1.45;
  }

  .setting-row .btn-secondary {
    min-width: 76px;
  }

  .setting-note {
    margin-top: 11px;
  }

  .settings-status-grid {
    grid-template-columns: repeat(auto-fit, minmax(160px, 1fr));
    margin: 0;
  }

  .settings-status-grid .summary-pill-card {
    padding: 12px 14px;
    box-shadow: none;
    background: var(--bg-secondary);
  }

  .settings-status-grid .summary-pill-card strong {
    margin-top: 6px;
    font-size: 14px;
  }

  .settings-status-grid .summary-pill-card .summary-foot {
    margin-top: 4px;
    color: var(--text-muted);
    font-size: 10px;
  }

  .settings-status-grid .status-value {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 6px;
  }

  .settings-status-grid .status-value strong {
    margin: 0;
  }

  .autostart-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 12px 20px;
  }

  .autostart-copy h2 {
    margin: 0;
  }

  .autostart-copy p {
    margin: 5px 0 0;
    color: var(--text-muted);
    font-size: 12px;
  }

  .autostart-control {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .autostart-status {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .autostart-switch {
    display: flex;
    align-items: center;
    width: 42px;
    height: 24px;
    padding: 2px;
    border: 1px solid var(--border-strong);
    border-radius: 999px;
    background: var(--bg-secondary);
    cursor: pointer;
  }

  .autostart-switch span {
    display: block;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--text-muted);
    transition: transform 150ms ease, background-color 150ms ease;
  }

  .autostart-switch[aria-checked='true'] {
    border-color: var(--brand-strong);
    background: var(--brand-strong);
  }

  .autostart-switch[aria-checked='true'] span {
    background: #fff;
    transform: translateX(18px);
  }

  .autostart-switch:focus-visible {
    outline: 2px solid var(--brand-primary);
    outline-offset: 3px;
  }

  .autostart-switch:disabled {
    cursor: not-allowed;
    opacity: 0.6;
  }

  .autostart-error {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 12px;
    margin-top: 12px;
    color: var(--color-danger);
    font-size: 12px;
  }

  @media (prefers-reduced-motion: reduce) {
    .autostart-switch span { transition: none; }
  }
  .refresh-options {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }

  .refresh-options .btn-secondary {
    min-width: 76px;
  }

  .diagnostic-panel {
    margin-top: 0;
  }

  @media (max-width: 820px) {
    .settings-preferences {
      grid-template-columns: 1fr;
    }
  }

  @media (max-width: 520px) {
    .theme-options {
      grid-template-columns: 1fr;
    }

    .theme-option {
      min-height: 46px;
    }

    .settings-card {
      padding: 14px;
    }
  }

  .diagnostic-error {
    margin: 0 0 10px;
    padding: 9px 11px;
    border: 1px solid var(--color-danger-border);
    border-radius: var(--radius-sm);
    background: var(--color-danger-bg);
    color: var(--color-danger);
    font-size: 12px;
  }

  .diagnostic-summary {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-wrap: wrap;
    padding: 10px 0 12px;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-secondary);
    font-size: 12px;
  }

  .diagnostic-badge,
  .diagnostic-severity {
    display: inline-flex;
    align-items: center;
    border-radius: var(--radius-pill);
    font-size: 11px;
    font-weight: 600;
    line-height: 1;
    padding: 4px 8px;
    white-space: nowrap;
  }

  .diagnostic-badge.ok,
  .diagnostic-severity.ok {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border: 1px solid var(--color-exact-border);
  }

  .diagnostic-badge.info,
  .diagnostic-severity.info {
    color: var(--color-quota);
    background: var(--color-quota-bg);
    border: 1px solid var(--color-quota-border);
  }

  .diagnostic-badge.warning,
  .diagnostic-severity.warning {
    color: var(--color-estimated);
    background: var(--color-estimated-bg);
    border: 1px solid var(--color-estimated-border);
  }

  .diagnostic-badge.error,
  .diagnostic-severity.error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border: 1px solid var(--color-danger-border);
  }

  .diagnostic-copy {
    margin-left: auto;
    min-height: 28px;
    padding: 0 10px;
  }

  .diagnostic-facts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(150px, 1fr));
    gap: 10px 16px;
    padding: 12px 0;
    border-bottom: 1px solid var(--border-subtle);
  }

  .diagnostic-facts .wide {
    grid-column: 1 / -1;
  }

  .diagnostic-facts > div {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .diagnostic-facts span {
    color: var(--text-muted);
    font-size: 11px;
  }

  .diagnostic-facts strong {
    color: var(--text-primary);
    font-size: 12.5px;
    font-weight: 600;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .diagnostic-facts .mono {
    font-family: var(--font-mono);
  }

  .diagnostic-check-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-top: 12px;
  }

  .diagnostic-check {
    display: flex;
    gap: 9px;
    padding: 9px 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
  }

  .diagnostic-dot {
    width: 7px;
    height: 7px;
    margin-top: 5px;
    border-radius: 50%;
    background: var(--color-exact);
    flex-shrink: 0;
  }

  .diagnostic-check.info .diagnostic-dot {
    background: var(--color-quota);
  }

  .diagnostic-check.warning .diagnostic-dot {
    background: var(--color-estimated);
  }

  .diagnostic-check.error .diagnostic-dot {
    background: var(--color-danger);
  }

  .diagnostic-check-main {
    min-width: 0;
  }

  .diagnostic-check-title {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .diagnostic-check-title strong {
    color: var(--text-primary);
    font-size: 12.5px;
  }

  .diagnostic-check-main p {
    margin: 4px 0 0;
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.55;
    overflow-wrap: anywhere;
  }

  .diagnostic-check-main .diagnostic-action {
    color: var(--text-muted);
    font-size: 11.5px;
  }
</style>
