<script lang="ts">
  import CustomSelect from './CustomSelect.svelte';
  import { formatNumber, percentOf, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import type { DashboardSnapshot, RemoteDevice, RemoteUsageBreakdown, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
  export let remoteDevices: RemoteDevice[];
  export let selectableRemoteDevices: RemoteDevice[];
  export let remoteSelectedDevice: string;
  export let remoteToolRows: RemoteUsageBreakdown[];
  export let remoteModelRows: RemoteUsageBreakdown[];

  // Hub Props
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

  // WebDAV Props
  export let webdavUrl = '';
  export let webdavUsername = '';
  export let webdavPassword = '';
  export let webdavEnabled = false;
  export let webdavBusy = false;
  export let webdavTesting = false;
  export let webdavSyncing = false;
  export let webdavError = '';
  export let webdavSuccess = '';
  export let webdavLastSyncAt: number | null = null;
  export let onTestWebDav: () => Promise<boolean> = async () => false;
  export let onSaveWebDav: () => Promise<void> = async () => {};
  export let onSyncWebDavNow: () => Promise<void> = async () => {};
  export let onRefreshWebDavDevices: () => Promise<void> = async () => {};
  export let onDisconnectWebDav: () => Promise<void> = async () => {};

  let syncTab: 'webdav' | 'hub' = 'webdav';
  $: if (hubPaired && !webdavEnabled) {
    syncTab = 'hub';
  }

  let showPassword = false;
  let confirmingDisconnectWebDav = false;
  let confirmingDisconnectHub = false;
  let activePresetTip = '';

  function applyPreset(provider: 'jianguoyun' | 'nextcloud' | 'alist' | 'infinicloud') {
    switch (provider) {
      case 'jianguoyun':
        webdavUrl = 'https://dav.jianguoyun.com/dav/TokenScope/';
        activePresetTip = '坚果云提示：账号请填坚果云注册邮箱；密码请前往坚果云网页版『账户信息 -> 安全选项 -> 第三方应用管理』添加应用授权密码（禁止使用账户主密码）。';
        break;
      case 'nextcloud':
        webdavUrl = 'https://your-domain.com/remote.php/dav/files/USERNAME/TokenScope/';
        activePresetTip = 'Nextcloud 提示：请将 URL 中的 your-domain.com 与 USERNAME 替换为您自己的域名与用户名。建议在个人设置『安全』中创建专属应用密码。';
        break;
      case 'alist':
        webdavUrl = 'https://your-alist-domain.com/dav/TokenScope/';
        activePresetTip = 'Alist 提示：建议在 Alist 管理后台为 TokenScope 指定专用用户及只读/读写挂载目录路径。';
        break;
      case 'infinicloud':
        webdavUrl = 'https://<your-id>.teracloud.jp/dav/TokenScope/';
        activePresetTip = 'InfiniCLOUD 提示：请在官网个人设置中开启 WebDAV 访问权限，并获取对应的连接地址与连接密码。';
        break;
    }
  }

  $: allDeviceTotal = (local?.total_tokens ?? 0) + snapshot.remote.total_tokens;
  $: topRemoteToolTotal = Math.max(1, ...remoteToolRows.map((r) => r.total_tokens));
  $: topRemoteModelTotal = Math.max(1, ...remoteModelRows.map((r) => r.total_tokens));
  $: otherRemoteDevices = remoteDevices.filter((d) => d.device_id !== snapshot.identity?.device_id);
</script>

<div class="page-header">
  <div>
    <h1>多设备网络</h1>
    <p>基于 WebDAV 云盘或自建私有 Hub 的去中心化同步状态；严格限定 device_local 物理单机归属</p>
  </div>
</div>

<!-- Cross-device Summary -->
<div class="device-summary-strip">
  <div class="summary-pill-card">
    <span class="summary-label">全设备 Token 总计</span>
    <strong class="tabular">{formatNumber(allDeviceTotal)}</strong>
    <span class="summary-foot">含本机与 {remoteDevices.length} 台远端节点</span>
  </div>
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">本机设备用量</span>
      <span class="precision-chip exact">占 {percentOf(local?.total_tokens ?? 0, allDeviceTotal).toFixed(1)}%</span>
    </div>
    <strong class="tabular">{formatNumber(local?.total_tokens)}</strong>
    <span class="summary-foot">时区 {snapshot.time_zone} · DB v{snapshot.schema_version}</span>
  </div>
  <div class="summary-pill-card">
    <div class="summary-head-row">
      <span class="summary-label">远端设备缓存</span>
      <span class="precision-chip quota">占 {percentOf(snapshot.remote.total_tokens, allDeviceTotal).toFixed(1)}%</span>
    </div>
    <strong class="tabular">{formatNumber(snapshot.remote.total_tokens)}</strong>
    <span class="summary-foot">{otherRemoteDevices.length} 台外部活跃节点</span>
  </div>
</div>

<!-- Device Topology Matrix -->
<div class="panel-head" style="margin-top: 8px;">
  <div>
    <h2>设备拓扑与在线节点</h2>
    <p>最近活跃代表最后一次访问同步服务的时间戳；非持久心跳长连接</p>
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
      <div class="device-token-val">
        <strong class="tabular">{formatNumber(local?.total_tokens)}</strong>
        <span class="rank-share tabular">{percentOf(local?.total_tokens ?? 0, allDeviceTotal).toFixed(1)}%</span>
      </div>
    </div>
    <div class="metric-foot">
      <span>时区：{snapshot.time_zone} · 架构：物理单机事实</span>
    </div>
  </article>

  <!-- Remote Devices Cards -->
  {#each otherRemoteDevices as dev}
    {@const devTokens = snapshot.remote_by_device.find((d) => d.owner_device_id === dev.device_id)?.total_tokens ?? 0}
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
        <span class="precision-chip" class:quota={!dev.revoked_at_ms} class:danger={!!dev.revoked_at_ms}>
          {dev.revoked_at_ms ? '已注销' : 'SYNC CACHED'}
        </span>
      </div>
      <div class="device-token-total">
        <span>远端 Token 缓存</span>
        <div class="device-token-val">
          <strong class="tabular">{formatNumber(devTokens)}</strong>
          <span class="rank-share tabular">{percentOf(devTokens, allDeviceTotal).toFixed(1)}%</span>
        </div>
      </div>
      <div class="metric-foot">
        <span>{dev.os_name} · v{dev.app_version} · 上报：{relativeTime(dev.last_seen_ms)}</span>
      </div>
    </article>
  {/each}
</div>

{#if otherRemoteDevices.length === 0}
  <div class="device-empty-banner">
    <span>当前仅有本机节点。您可以在下方配置 WebDAV 云盘（如坚果云、Nextcloud）或自建 Hub，将多台工作机连接至同一同步源，自动汇聚全端 Token。</span>
  </div>
{/if}

<!-- Single Device Drilldown -->
<section class="panel-card" style="margin-bottom: 20px;">
  <div class="panel-head">
    <div>
      <h2>远端设备维度透视</h2>
      <p>穿透查看指定远端设备的工具、模型与项目贡献分布</p>
    </div>
    <div style="display: flex; align-items: center; gap: 8px; min-width: 220px;">
      <span style="font-size: 11px; color: var(--text-secondary); white-space: nowrap;">选择设备：</span>
      <div style="flex: 1; min-width: 0;">
        <CustomSelect
          value={remoteSelectedDevice}
          options={[
            { key: 'all', label: '全部远端设备汇总' },
            ...selectableRemoteDevices.map((d) => ({
              key: d.device_id,
              label: d.display_name,
              subtext: d.device_id.slice(-6),
            })),
          ]}
          ariaLabel="选择远端透视设备"
          onChange={(val) => { remoteSelectedDevice = val; }}
        />
      </div>
    </div>
  </div>

  {#if otherRemoteDevices.length === 0}
    <div class="drilldown-empty-state">
      <div class="drilldown-empty-icon">{@html icons.devices}</div>
      <h4>等待远端设备数据接入</h4>
      <p>当前网络仅检测到本机节点。在您的其它工作机（如 MacBook、家用电脑或服务器）上配置相同的 WebDAV 云盘，数据自动双向同步后，即可在此穿透分析各设备的工具、模型与 Token 消耗。</p>
    </div>
  {:else}
  <div class="two-col-grid" style="margin-bottom: 0;">
    <div>
      <h3 style="font-size: 12px; margin-top: 0; color: var(--text-secondary); margin-bottom: 10px;">远端工具分布</h3>
      {#if remoteToolRows.length > 0}
        <div class="rank-list">
          {#each remoteToolRows.slice(0, 6) as row, idx}
            <div class="rank-item">
              <div class="rank-row">
                <span class="rank-title">
                  <span class="rank-num tabular">0{idx + 1}</span>
                  <strong>{row.label}</strong>
                </span>
                <span class="rank-meta">
                  <span class="rank-value tabular">{formatNumber(row.total_tokens)}</span>
                  <span class="rank-share tabular">{percentOf(row.total_tokens, topRemoteToolTotal).toFixed(1)}%</span>
                </span>
              </div>
              <div class="rank-track">
                <div class="rank-fill" style={`width: ${percentOf(row.total_tokens, topRemoteToolTotal)}%`}></div>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="empty-box">暂无远端工具记录</div>
      {/if}
    </div>

    <div>
      <h3 style="font-size: 12px; margin-top: 0; color: var(--text-secondary); margin-bottom: 10px;">远端模型分布</h3>
      {#if remoteModelRows.length > 0}
        <div class="rank-list">
          {#each remoteModelRows.slice(0, 6) as row, idx}
            <div class="rank-item">
              <div class="rank-row">
                <span class="rank-title">
                  <span class="rank-num tabular">0{idx + 1}</span>
                  <strong title={row.label}>{row.label}</strong>
                </span>
                <span class="rank-meta">
                  <span class="rank-value tabular">{formatNumber(row.total_tokens)}</span>
                  <span class="rank-share tabular">{percentOf(row.total_tokens, topRemoteModelTotal).toFixed(1)}%</span>
                </span>
              </div>
              <div class="rank-track">
                <div class="rank-fill" style={`width: ${percentOf(row.total_tokens, topRemoteModelTotal)}%`}></div>
              </div>
            </div>
          {/each}
        </div>
      {:else}
        <div class="empty-box">暂无远端模型记录</div>
      {/if}
    </div>
  </div>
  {/if}
</section>

<!-- Multi-Device Sync Provider Management -->
<section class="panel-card">
  <div class="panel-head" style="margin-bottom: 16px;">
    <div>
      <h2>多设备同步服务配置</h2>
      <p>凭证严格加密存储在操作系统原生安全密码库（Keyring），SQLite 零残留</p>
    </div>
    <div class="sync-provider-tabs">
      <button
        class="sync-tab-btn"
        class:active={syncTab === 'webdav'}
        onclick={() => (syncTab = 'webdav')}
      >
        {@html icons.cloud} WebDAV 云盘同步
        {#if webdavEnabled}
          <span class="provider-badge active">已启用</span>
        {/if}
      </button>
      <button
        class="sync-tab-btn"
        class:active={syncTab === 'hub'}
        onclick={() => (syncTab = 'hub')}
      >
        {@html icons.shield} 自建 Hub 配对
        {#if hubPaired}
          <span class="provider-badge active">已连接</span>
        {/if}
      </button>
    </div>
  </div>

  {#if syncTab === 'webdav'}
    <!-- WebDAV Sync Panel -->
    <div class="webdav-sync-panel">
      <div class="sync-intro-box">
        <div class="sync-intro-icon">{@html icons.cloud}</div>
        <div class="sync-intro-text">
          <strong>去中心化 WebDAV 云盘同步</strong>
          <p>
            支持坚果云（Nutstore）、Nextcloud、InfiniCLOUD、群晖 WebDAV、Alist 等标准 WebDAV 网盘。每个设备独立上报自身批次，去中心化拉取其他节点，无单点故障与写冲突。应用密码通过操作系统原生安全凭据库加密保护。
          </p>
        </div>
      </div>

      <div class="form-grid" style="margin-top: 14px;">
        <label class="field-label">
          <span>WebDAV 服务器地址 (URL)</span>
          <input
            class="field-input"
            type="url"
            bind:value={webdavUrl}
            placeholder="例如：https://dav.jianguoyun.com/dav/TokenScope/"
          />
          <div class="provider-preset-row">
            <span class="preset-label">快捷填入：</span>
            <button
              type="button"
              class="preset-chip"
              onclick={() => applyPreset('jianguoyun')}
            >
              坚果云 (Nutstore)
            </button>
            <button
              type="button"
              class="preset-chip"
              onclick={() => applyPreset('nextcloud')}
            >
              Nextcloud
            </button>
            <button
              type="button"
              class="preset-chip"
              onclick={() => applyPreset('alist')}
            >
              Alist
            </button>
            <button
              type="button"
              class="preset-chip"
              onclick={() => applyPreset('infinicloud')}
            >
              InfiniCLOUD
            </button>
          </div>
          {#if activePresetTip}
            <div class="provider-guide-tip">
              {@html icons.info} {activePresetTip}
            </div>
          {/if}
        </label>
        <label class="field-label">
          <span>WebDAV 账号 / 邮箱</span>
          <input
            class="field-input"
            type="text"
            bind:value={webdavUsername}
            placeholder="例如：user@example.com"
          />
        </label>
        <label class="field-label">
          <span>应用授权密码 / 访问令牌</span>
          <div class="password-input-wrap">
            <input
              class="field-input"
              type={showPassword ? "text" : "password"}
              bind:value={webdavPassword}
              placeholder={webdavEnabled ? "已加密存储（如需修改请输入新密码）" : "请输入网盘生成的应用专用密码"}
            />
            {#if webdavPassword}
              <button
                type="button"
                class="password-toggle-btn"
                onclick={() => (showPassword = !showPassword)}
                title={showPassword ? "隐藏密码" : "显示密码"}
              >
                {@html showPassword ? icons.eyeOff : icons.eye}
              </button>
            {/if}
          </div>
        </label>
      </div>

      <div class="sync-actions-bar">
        <div class="sync-actions-left">
          <button
            class="btn-primary"
            disabled={webdavBusy || webdavTesting || webdavSyncing || !webdavUrl.trim() || !webdavUsername.trim()}
            onclick={() => void onSaveWebDav()}
          >
            {@html icons.shield} {webdavEnabled ? '更新并保存配置' : '保存并启用 WebDAV 同步'}
          </button>
          <button
            class="btn-secondary"
            disabled={webdavBusy || webdavTesting || !webdavUrl.trim() || !webdavUsername.trim() || !webdavPassword.trim()}
            onclick={() => void onTestWebDav()}
          >
            <span class:spin={webdavTesting}>{@html icons.refresh}</span> 测试连接
          </button>
          {#if webdavEnabled}
            <button
              class="btn-secondary"
              disabled={webdavBusy || webdavSyncing}
              onclick={() => void onSyncWebDavNow()}
            >
              <span class:spin={webdavSyncing}>{@html icons.refresh}</span> 立即同步
            </button>
            <button
              class="btn-secondary"
              disabled={webdavBusy}
              onclick={() => void onRefreshWebDavDevices()}
            >
              {@html icons.refresh} 刷新设备列表
            </button>
          {/if}
        </div>

        {#if webdavEnabled}
          <div class="sync-actions-right">
            {#if confirmingDisconnectWebDav}
              <div class="disconnect-confirm-group">
                <span class="confirm-tip">确认断开？将清除安全密码库中的凭据</span>
                <button
                  class="btn-danger"
                  disabled={webdavBusy}
                  onclick={() => {
                    confirmingDisconnectWebDav = false;
                    void onDisconnectWebDav();
                  }}
                >
                  {@html icons.unlink} 确认断开
                </button>
                <button
                  class="btn-secondary"
                  onclick={() => (confirmingDisconnectWebDav = false)}
                >
                  取消
                </button>
              </div>
            {:else}
              <button
                class="btn-danger-ghost"
                disabled={webdavBusy}
                onclick={() => (confirmingDisconnectWebDav = true)}
                title="断开 WebDAV 同步并清除本地保存的凭据"
              >
                {@html icons.unlink} 断开 WebDAV
              </button>
            {/if}
          </div>
        {/if}
      </div>

      {#if webdavLastSyncAt}
        <div class="metric-foot" style="margin-top: 12px;">
          <span>上次 WebDAV 同步时间：{relativeTime(webdavLastSyncAt)}</span>
        </div>
      {/if}

      {#if webdavError}
        <div class="session-action-notice error" role="alert" style="margin-top: 14px; margin-bottom: 0;">
          {webdavError}
        </div>
      {/if}
      {#if webdavSuccess}
        <div class="session-action-notice success" role="status" aria-live="polite" style="margin-top: 14px; margin-bottom: 0;">
          {webdavSuccess}
        </div>
      {/if}
    </div>
  {:else}
    <!-- Hub Pairing Management -->
    <div>
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

      <div class="sync-actions-bar">
        <div class="sync-actions-left">
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
          {/if}
        </div>

        {#if hubPaired}
          <div class="sync-actions-right">
            {#if confirmingDisconnectHub}
              <div class="disconnect-confirm-group">
                <span class="confirm-tip">确认断开与 Hub 的配对？</span>
                <button
                  class="btn-danger"
                  disabled={hubBusy}
                  onclick={() => {
                    confirmingDisconnectHub = false;
                    void onDisconnectHub();
                  }}
                >
                  {@html icons.unlink} 确认断开
                </button>
                <button
                  class="btn-secondary"
                  onclick={() => (confirmingDisconnectHub = false)}
                >
                  取消
                </button>
              </div>
            {:else}
              <button
                class="btn-danger-ghost"
                disabled={hubBusy}
                onclick={() => (confirmingDisconnectHub = true)}
              >
                {@html icons.unlink} 断开连接
              </button>
            {/if}
          </div>
        {/if}
      </div>

      {#if hubError}
        <div class="session-action-notice error" role="alert" style="margin-top: 14px; margin-bottom: 0;">
          {hubError}
        </div>
      {/if}
      {#if hubSuccess}
        <div class="session-action-notice success" role="status" aria-live="polite" style="margin-top: 14px; margin-bottom: 0;">
          {hubSuccess}
        </div>
      {/if}
    </div>
  {/if}
</section>
