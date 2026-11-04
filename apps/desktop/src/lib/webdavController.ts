import { get, writable } from 'svelte/store';
import {
  disconnectWebDav,
  getWebDavConfig,
  listWebDavDevices,
  saveWebDavConfig,
  syncWebDavNow,
  testWebDavConnection,
  type WebDavPublicConfig,
  type WebDavSyncReport,
} from './api';
import type { RemoteDevice } from './types';

export type WebDavApi = {
  testWebDavConnection: typeof testWebDavConnection;
  saveWebDavConfig: typeof saveWebDavConfig;
  getWebDavConfig: typeof getWebDavConfig;
  disconnectWebDav: typeof disconnectWebDav;
  listWebDavDevices: typeof listWebDavDevices;
  syncWebDavNow: typeof syncWebDavNow;
};

export type WebDavRuntime = {
  api: WebDavApi;
  confirm: (message: string) => boolean;
  refreshDashboard?: () => Promise<void>;
};

export type WebDavControllerState = {
  url: string;
  username: string;
  password: string;
  enabled: boolean;
  devices: RemoteDevice[];
  busy: boolean;
  testing: boolean;
  syncing: boolean;
  error: string;
  success: string;
  lastSyncReport: WebDavSyncReport | null;
  lastSyncAt: number | null;
};

const INITIAL_STATE: WebDavControllerState = {
  url: '',
  username: '',
  password: '',
  enabled: false,
  devices: [],
  busy: false,
  testing: false,
  syncing: false,
  error: '',
  success: '',
  lastSyncReport: null,
  lastSyncAt: null,
};

export function createWebDavController(
  isRuntimeAvailable: () => boolean,
  runtime: WebDavRuntime = {
    api: {
      testWebDavConnection,
      saveWebDavConfig,
      getWebDavConfig,
      disconnectWebDav,
      listWebDavDevices,
      syncWebDavNow,
    },
    confirm: (message) => window.confirm(message),
  },
) {
  const state = writable<WebDavControllerState>({ ...INITIAL_STATE });

  function update(patch: Partial<WebDavControllerState>) {
    state.update((current) => ({ ...current, ...patch }));
  }

  async function loadConfig() {
    if (!isRuntimeAvailable()) return;
    try {
      const config: WebDavPublicConfig | null = await runtime.api.getWebDavConfig();
      if (config && config.enabled) {
        update({
          url: config.url,
          username: config.username,
          enabled: true,
          error: '',
        });
        await refreshDevices();
      } else {
        update({ enabled: false });
      }
    } catch (error) {
      update({ error: error instanceof Error ? error.message : String(error) });
    }
  }

  async function testConnection(): Promise<boolean> {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable()) return false;
    const url = current.url.trim();
    const username = current.username.trim();
    const password = current.password;

    if (!url) {
      update({ error: '请输入 WebDAV 服务器地址' });
      return false;
    }
    if (!username) {
      update({ error: '请输入 WebDAV 账号或邮箱' });
      return false;
    }
    if (!password) {
      update({ error: '请输入 WebDAV 应用密码或访问令牌' });
      return false;
    }

    update({ busy: true, testing: true, error: '', success: '' });
    try {
      await runtime.api.testWebDavConnection(url, username, password);
      update({ success: 'WebDAV 服务器连接成功，验证通过！' });
      return true;
    } catch (error) {
      update({ error: error instanceof Error ? error.message : String(error) });
      return false;
    } finally {
      update({ busy: false, testing: false });
    }
  }

  async function saveConfig() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable()) return;
    const url = current.url.trim();
    const username = current.username.trim();
    const password = current.password;

    if (!url || !username || !password) {
      update({ error: '请完整填写 WebDAV 服务器地址、账号及密码' });
      return;
    }

    update({ busy: true, error: '', success: '' });
    try {
      await runtime.api.saveWebDavConfig(url, username, password);
      update({
        enabled: true,
        password: '', // Never retain password in UI state once saved to Keyring
        success: 'WebDAV 同步配置已安全加密保存至系统凭据库，并已启用！',
      });
      await syncNow();
      await refreshDevices();
    } catch (error) {
      update({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ busy: false });
    }
  }

  async function syncNow() {
    const current = get(state);
    if (current.syncing || !isRuntimeAvailable()) return;
    update({ syncing: true, error: '' });
    try {
      const report = await runtime.api.syncWebDavNow();
      update({
        lastSyncReport: report,
        lastSyncAt: Date.now(),
        success: `同步完成：上报 ${report.uploaded_records} 条记录（${report.uploaded_batches} 批次），拉取 ${report.pulled_records} 条远端记录。`,
      });
      if (runtime.refreshDashboard) {
        await runtime.refreshDashboard();
      }
      await refreshDevices();
    } catch (error) {
      update({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ syncing: false });
    }
  }

  async function refreshDevices() {
    if (!isRuntimeAvailable()) return;
    try {
      const result = await runtime.api.listWebDavDevices();
      update({ devices: result.devices });
    } catch (error) {
      // Non-blocking device refresh failure
      const msg = error instanceof Error ? error.message : String(error);
      if (msg.includes('尚未配置')) {
        update({ devices: [], enabled: false });
      }
    }
  }

  async function disconnect() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable()) return;
    if (
      !runtime.confirm(
        '确定要断开 WebDAV 同步吗？\n断开后将清除本机保存的密码与同步配置，但不会删除本地或 WebDAV 云盘已有数据。',
      )
    ) {
      return;
    }
    update({ busy: true, error: '', success: '' });
    try {
      await runtime.api.disconnectWebDav();
      update({
        ...INITIAL_STATE,
        success: '已断开 WebDAV 同步并清除系统凭据。',
      });
    } catch (error) {
      update({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ busy: false });
    }
  }

  return {
    state,
    loadConfig,
    testConnection,
    saveConfig,
    syncNow,
    refreshDevices,
    disconnect,
    setUrl: (url: string) => update({ url }),
    setUsername: (username: string) => update({ username }),
    setPassword: (password: string) => update({ password }),
  };
}
