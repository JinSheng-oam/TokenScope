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
    <h2>设备列表</h2>
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
    <span>在其他设备上配置相同的 WebDAV 或 Hub。</span>
  </div>
{/if}

<!-- Single Device Drilldown -->
<section class="panel-card" style="margin-bottom: 20px;">
  <div class="panel-head">
    <div>
      <h2>远端设备用量</h2>
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
      <h4>暂无远端设备用量</h4>
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
      <h2>同步设置</h2>
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
        <div class="sync-intro-main">
          <div class="sync-intro-icon">{@html icons.cloud}</div>
          <div class="sync-intro-text">
            <div class="sync-intro-title-row">
              <strong>WebDAV 同步</strong>
              <span class="sync-security-pill">{@html icons.lock} 系统凭证库</span>
            </div>
          </div>
        </div>

        <div class="provider-preset-row">
          <span class="preset-label">快捷模板：</span>
          <button type="button" class="preset-chip" onclick={() => applyPreset('jianguoyun')}>
            坚果云
          </button>
          <button type="button" class="preset-chip" onclick={() => applyPreset('nextcloud')}>
            Nextcloud
          </button>
          <button type="button" class="preset-chip" onclick={() => applyPreset('alist')}>
            Alist
          </button>
          <button type="button" class="preset-chip" onclick={() => applyPreset('infinicloud')}>
            InfiniCLOUD
          </button>
        </div>
      </div>

      {#if activePresetTip}
        <div class="provider-guide-tip">
          {@html icons.info}
          <span>{activePresetTip}</span>
        </div>
      {/if}

      <div class="form-grid" style="margin-top: 14px;">
        <label class="field-label">
          <span>WebDAV 服务器地址 (URL)</span>
          <input
            class="field-input"
            type="url"
            bind:value={webdavUrl}
            placeholder="例如：https://dav.jianguoyun.com/dav/TokenScope/"
          />
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
              class:has-keyring-badge={webdavEnabled && !webdavPassword}
              type={showPassword ? "text" : "password"}
              bind:value={webdavPassword}
              placeholder={webdavEnabled ? "已加密存储（修改请直接输入新密码）" : "请输入网盘生成的应用专用密码"}
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
            {:else if webdavEnabled}
              <span class="keyring-stored-badge" title="密码已安全保存在操作系统凭据管理器中">
                {@html icons.lock} 已加密
              </span>
            {/if}
          </div>
        </label>
      </div>

      <div class="sync-actions-bar">
        <div class="sync-actions-left">
          {#if webdavEnabled}
            <button
              class="btn-primary"
              disabled={webdavBusy || webdavSyncing}
              onclick={() => void onSyncWebDavNow()}
            >
              <span class:spin={webdavSyncing}>{@html icons.refresh}</span> 立即同步
            </button>
            <button
              class="btn-secondary"
              disabled={webdavBusy || webdavTesting || webdavSyncing || !webdavUrl.trim() || !webdavUsername.trim()}
              onclick={() => void onSaveWebDav()}
            >
              {@html icons.shield} 保存配置
            </button>
          {:else}
            <button
              class="btn-primary"
              disabled={webdavBusy || webdavTesting || webdavSyncing || !webdavUrl.trim() || !webdavUsername.trim()}
              onclick={() => void onSaveWebDav()}
            >
              {@html icons.shield} 保存并启用 WebDAV 同步
            </button>
          {/if}

          <button
            class="btn-secondary"
            disabled={webdavBusy || webdavTesting || !webdavUrl.trim() || !webdavUsername.trim() || (!webdavPassword.trim() && !webdavEnabled)}
            onclick={() => void onTestWebDav()}
            title={!webdavPassword.trim() && webdavEnabled ? "如需测试新密码请先在密码框输入" : "测试 WebDAV 服务器连通性"}
          >
            <span class:spin={webdavTesting}>{@html icons.zap}</span> 测试连接
          </button>

          {#if webdavEnabled}
            <button
              class="btn-secondary"
              disabled={webdavBusy}
              onclick={() => void onRefreshWebDavDevices()}
            >
              {@html icons.devices} 刷新设备列表
            </button>
          {/if}
        </div>

        <div class="sync-actions-right">
          {#if webdavLastSyncAt}
            <span class="sync-last-time-pill" title="最近一次成功执行 WebDAV 同步的时间">
              <span class="sync-status-dot"></span>
              上次同步：{relativeTime(webdavLastSyncAt)}
            </span>
          {/if}

          {#if webdavEnabled}
            {#if confirmingDisconnectWebDav}
              <div class="disconnect-confirm-group">
                <span class="confirm-tip">确认断开？将清除安全密码库凭据</span>
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
          {/if}
        </div>
      </div>

      {#if webdavError}
        <div class="session-action-notice error sync-feedback-banner" role="alert">
          <span class="feedback-icon">{@html icons.alert}</span>
          <span>{webdavError}</span>
        </div>
      {/if}
      {#if webdavSuccess}
        <div class="session-action-notice success sync-feedback-banner" role="status" aria-live="polite">
          <span class="feedback-icon">{@html icons.check}</span>
          <span>{webdavSuccess}</span>
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
