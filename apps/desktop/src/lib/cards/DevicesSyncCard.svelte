<script lang="ts">
  import { formatCompactTokens } from '../dashboardView';
  import { percentOf, relativeTime } from '../format';
  import { liveMinute } from '../liveClock';
  import { buildDeviceComparison } from '../overviewCards';
  import type { DashboardSnapshot, RemoteDevice } from '../types';

  let {
    cardRows = 5,
    snapshot,
    remoteDevices = [],
    onNavigateDevices = () => {},
  }: {
    cardRows?: number;
    snapshot: DashboardSnapshot;
    remoteDevices?: RemoteDevice[];
    onNavigateDevices?: () => void;
  } = $props();

  function syncStatusLabel(status: DashboardSnapshot['sync_status']['last_status']): {
    text: string;
    tone: 'ok' | 'warn' | 'error' | 'muted';
  } {
    switch (status) {
      case 'success':
        return { text: '同步正常', tone: 'ok' };
      case 'idle':
        return { text: '已就绪', tone: 'ok' };
      case 'deferred':
        return { text: '延迟重试', tone: 'warn' };
      case 'blocked':
      case 'auth_required':
        return { text: '需要认证', tone: 'warn' };
      case 'error':
        return { text: '同步异常', tone: 'error' };
      case 'never':
      default:
        return { text: '未同步', tone: 'muted' };
    }
  }

  let deviceRows = $derived(buildDeviceComparison(snapshot, remoteDevices));
  let topDeviceTokens = $derived(Math.max(1, ...deviceRows.map((d) => d.totalTokens)));
  let syncMeta = $derived(syncStatusLabel(snapshot.sync_status.last_status));
</script>

<section class="panel-card devices-card">
  <div class="panel-head">
    <div>
      <h2>多设备与同步</h2>
    </div>
    <button type="button" class="panel-tag" onclick={onNavigateDevices}>设备网络 →</button>
  </div>

  <div class="sync-inline-strip">
    <span class={`sync-pill tone-${syncMeta.tone}`}>
      <span class="sync-dot"></span>
      {syncMeta.text}
    </span>
    <span class="sync-inline-meta tabular">
      {#if snapshot.sync_status.pending > 0}
        待上传 {snapshot.sync_status.pending} 笔
      {:else if snapshot.sync_status.last_success_ms}
        上次同步 {relativeTime(snapshot.sync_status.last_success_ms, $liveMinute)}
      {:else}
        {deviceRows.length} 台活跃设备
      {/if}
    </span>
  </div>

  {#if deviceRows.length > 0}
    <div class="rank-list">
      {#each deviceRows.slice(0, cardRows) as dev}
        <div class="rank-item">
          <div class="rank-row">
            <span class="rank-title">
              <strong title={dev.name}>{dev.name}</strong>
              {#if dev.isLocal}
                <span class="device-local-chip">本机</span>
              {/if}
            </span>
            <span class="rank-meta">
              <span class="rank-value tabular">{formatCompactTokens(dev.totalTokens)}</span>
              <span class="rank-share tabular">{dev.sharePct.toFixed(1)}%</span>
            </span>
          </div>
          <div class="rank-track">
            <div class="rank-fill device-fill" style={`width: ${percentOf(dev.totalTokens, topDeviceTokens)}%`}></div>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty-box">暂无设备用量数据</div>
  {/if}
</section>

<style>
  .sync-inline-strip {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 10px;
    margin-bottom: 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    font-size: 11px;
  }

  .sync-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-weight: 600;
  }

  .sync-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .sync-pill.tone-ok {
    color: var(--color-exact);
  }

  .sync-pill.tone-ok .sync-dot {
    background: var(--color-exact);
  }

  .sync-pill.tone-warn {
    color: var(--color-estimated);
  }

  .sync-pill.tone-warn .sync-dot {
    background: var(--color-estimated);
  }

  .sync-pill.tone-error {
    color: var(--color-danger);
  }

  .sync-pill.tone-error .sync-dot {
    background: var(--color-danger);
  }

  .sync-pill.tone-muted {
    color: var(--text-muted);
  }

  .sync-inline-meta {
    color: var(--text-muted);
    font-size: 11px;
  }

  .device-local-chip {
    display: inline-flex;
    align-items: center;
    padding: 0 5px;
    margin-left: 5px;
    border-radius: 4px;
    font-size: 9.5px;
    font-weight: 600;
    background: var(--brand-subtle);
    color: var(--brand-strong);
    vertical-align: middle;
  }

  .rank-fill.device-fill {
    background: linear-gradient(90deg, var(--accent-device-start) 0%, var(--accent-device-end) 100%);
  }
</style>
