<script lang="ts">
import { BOOTSTRAP_STAGE } from '../lib/status';
  import { formatNumber } from '../lib/format';
  import type { ThemeMode } from '../lib/theme';
  import { icons } from '../lib/icons';
  import type { AgentStatus, DiagnosticReport, DiagnosticSeverity } from '../lib/types';

  export let currentTheme: ThemeMode;
  export let autostartEnabled: boolean;
  export let autostartBusy: boolean;
  export let autostartError: string;
  export let runtimeAvailable: boolean;
  export let agent: AgentStatus | null;
  export let onSwitchTheme: (mode: ThemeMode) => void;
  export let onToggleAutostart: () => Promise<void>;
  export let onRunDiagnostics: () => Promise<DiagnosticReport>;

  let diagnostics: DiagnosticReport | null = null;
  let diagnosticsBusy = false;
  let diagnosticsError = '';
  let diagnosticsCopied = false;

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
</script>

<div class="page-header">
            <h1>系统设置与偏好</h1>
            <p>控制系统常驻生命周期、主题外观与本地采集行为</p>
          </div>

          <!-- Theme Settings -->
          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="panel-head">
              <h2>界面主题外观</h2>
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
          </section>

          <!-- Autostart & Lifecycle -->
          <section class="panel-card" style="margin-bottom: 16px;">
            <div class="panel-head">
              <div>
                <h2>系统登录时自启动 (--background)</h2>
                <p>遵循 ADR-0005 规范：启动后仅在系统托盘静默常驻，不创建 WebView，内存占用 ≤25 MiB</p>
              </div>
              <button
                class="btn-primary"
                disabled={autostartBusy || !runtimeAvailable}
                onclick={() => void onToggleAutostart()}
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

          <!-- Full Health Diagnostics -->
          <section class="panel-card diagnostic-panel">
            <div class="panel-head">
              <div>
                <h2>系统诊断</h2>
                <p>只读检查数据库、设备身份、Schema、采集检查点、同步队列和磁盘余量</p>
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
