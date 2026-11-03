<script lang="ts">
import { BOOTSTRAP_STAGE } from '../lib/status';
  import type { ThemeMode } from '../lib/theme';
  import { icons } from '../lib/icons';
  import type { AgentStatus } from '../lib/types';

  export let currentTheme: ThemeMode;
  export let autostartEnabled: boolean;
  export let autostartBusy: boolean;
  export let autostartError: string;
  export let runtimeAvailable: boolean;
  export let agent: AgentStatus | null;
  export let onSwitchTheme: (mode: ThemeMode) => void;
  export let onToggleAutostart: () => Promise<void>;
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
