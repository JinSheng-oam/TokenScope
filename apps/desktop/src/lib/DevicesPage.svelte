<script lang="ts">
import { formatNumber, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import type { DashboardSnapshot, RemoteDevice, RemoteUsageBreakdown, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
  export let remoteDevices: RemoteDevice[];
  export let selectableRemoteDevices: RemoteDevice[];
  export let remoteSelectedDevice: string;
  export let remoteToolRows: RemoteUsageBreakdown[];
  export let remoteModelRows: RemoteUsageBreakdown[];
  export let hubUrl: string;
  export let pairingCode: string;
  export let hubDisplayName: string;
  export let hubBusy: boolean;
  export let hubError: string;
  export let hubSuccess: string;
  export let hubPaired: boolean;
  export let onPairWithHub: () => Promise<void>;
  export let onRefreshHubDevices: () => Promise<void>;
  export let onRenameHubDevice: () => Promise<void>;
  export let onRotateHubCredential: () => Promise<void>;
  export let onDisconnectHub: () => Promise<void>;
</script>

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
          <section class="panel-card" style="margin-bottom: 20px;">
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
                onclick={() => void onPairWithHub()}
              >
                {@html icons.shield} 配对此设备
              </button>
              <button class="btn-secondary" disabled={hubBusy || !hubUrl.trim()} onclick={() => void onRefreshHubDevices()}>
                {@html icons.refresh} 刷新设备列表
              </button>
              {#if hubPaired}
                <button class="btn-secondary" disabled={hubBusy} onclick={() => void onRenameHubDevice()}>
                  {@html icons.edit} 重命名本机
                </button>
                <button class="btn-secondary" disabled={hubBusy} onclick={() => void onRotateHubCredential()}>
                  {@html icons.key} 轮换安全凭证
                </button>
                <button class="btn-danger" disabled={hubBusy} onclick={() => void onDisconnectHub()}>
                  {@html icons.unlink} 断开连接
                </button>
              {/if}
            </div>

            {#if hubError}
              <p role="alert" style="color: var(--color-danger); margin: 12px 0 0; font-size: 12px;">{hubError}</p>
            {/if}
            {#if hubSuccess}
              <p role="status" aria-live="polite" style="color: var(--color-exact); margin: 12px 0 0; font-size: 12px;">{hubSuccess}</p>
            {/if}
          </section>
