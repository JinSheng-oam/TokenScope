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

<div class="page-header">
            <h1>系统设置</h1>
          </div>

          <!-- Theme Settings -->
          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="panel-head">
              <h2>主题</h2>
              <span class="panel-tag">Linear Craft</span>
            </div>
            <div style="display: flex; gap: 10px; flex-wrap: wrap;">
              <button
                class="btn-secondary"
                class:selected={currentTheme === 'dark'}
                aria-pressed={currentTheme === 'dark'}
                onclick={() => onSwitchTheme('dark')}
              >
                {@html icons.moon} 深色模式 (Obsidian)
              </button>
              <button
                class="btn-secondary"
                class:selected={currentTheme === 'light'}
                aria-pressed={currentTheme === 'light'}
                onclick={() => onSwitchTheme('light')}
              >
                {@html icons.sun} 浅色模式 (Soft Zinc)
              </button>
              <button
                class="btn-secondary"
                class:selected={currentTheme === 'system'}
                aria-pressed={currentTheme === 'system'}
                onclick={() => onSwitchTheme('system')}
              >
                {@html icons.laptop} 跟随操作系统
              </button>
            </div>

            <div style="display: flex; align-items: center; justify-content: space-between; gap: 12px; margin-top: 14px; padding-top: 14px; border-top: 1px solid var(--border-subtle);">
              <div>
                <div style="font-size: 13px; font-weight: 600; color: var(--text-primary);">核心指标数字滚动动效</div>
              </div>
              <button
                class="btn-secondary"
                class:selected={rollingNumbersEnabled}
                aria-pressed={rollingNumbersEnabled}
                onclick={onToggleRollingNumbers}
              >
                {rollingNumbersEnabled ? '已开启 · 点击关闭' : '已关闭 · 点击开启'}
              </button>
            </div>
          </section>

          <!-- Refresh Interval -->
          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="panel-head">
              <div>
                <h2>刷新间隔</h2>
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
          </section>

          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="autostart-row">
              <div class="autostart-copy">
                <h2>开机自启动</h2>
                <p>登录系统后在后台采集，窗口默认不显示。</p>
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

          <!-- Supervisor Health Diagnostics -->
          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="panel-head">
              <div>
                <h2>采集状态</h2>
              </div>
              <span class="precision-chip" class:exact={agent?.running} class:danger={!agent?.running}>
                {agent?.running ? 'RUNNING' : 'STOPPED'}
              </span>
            </div>
            <div class="device-summary-strip" style="margin-bottom: 0;">
              <div class="summary-pill-card">
                <span class="summary-label">守护进程状态</span>
                <div style="display: flex; align-items: center; gap: 7px; margin-top: 6px;">
                  <span class="source-status-dot" class:complete={agent?.running} class:degraded={!agent?.running}></span>
                  <strong style="margin: 0; font-size: 14px;">{agent?.running ? '正常运行中' : '未连接'}</strong>
                </div>
              </div>
              <div class="summary-pill-card">
                <span class="summary-label">Agent 二进制</span>
                <strong style="margin-top: 6px; font-size: 14px;">{agent?.binaryFound ? '已部署' : '未找到'}</strong>
                <span class="summary-foot">resources/tokenscope-agent</span>
              </div>
              <div class="summary-pill-card">
                <span class="summary-label">SQLite WAL 数据库</span>
                <strong style="margin-top: 6px; font-size: 14px;">{agent?.databaseReady ? '就绪 (WAL 模式)' : '初始化中'}</strong>
                <span class="summary-foot">tokenscope.db</span>
              </div>
              <div class="summary-pill-card">
                <span class="summary-label">桌面版本里程碑</span>
                <strong style="margin-top: 6px; font-size: 14px;" class="tabular">{BOOTSTRAP_STAGE}</strong>
                <span class="summary-foot">Linear Craft UI</span>
              </div>
            </div>
          </section>

          <!-- Full Health Diagnostics -->
          <section class="panel-card diagnostic-panel">
            <div class="panel-head">
              <div>
                <h2>系统诊断</h2>
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

<style>
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
    margin-top: 16px;
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
