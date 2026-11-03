import { get, writable } from 'svelte/store';
import {
  disconnectHubDevice,
  listHubDevices,
  pairHubDevice,
  renameHubDevice,
  rotateHubDeviceToken,
} from './api';
import type { RemoteDevice } from './types';

export type HubApi = {
  listHubDevices: typeof listHubDevices;
  pairHubDevice: typeof pairHubDevice;
  renameHubDevice: typeof renameHubDevice;
  rotateHubDeviceToken: typeof rotateHubDeviceToken;
  disconnectHubDevice: typeof disconnectHubDevice;
};

export type HubRuntime = {
  api: HubApi;
  storage: Pick<Storage, 'getItem' | 'setItem'>;
  confirm: (message: string) => boolean;
};

const HUB_ORIGIN_KEY = 'tokenscope.hubUrl';

export type HubControllerState = {
  hubUrl: string;
  pairingCode: string;
  displayName: string;
  devices: RemoteDevice[];
  busy: boolean;
  error: string;
  success: string;
  paired: boolean;
};

const INITIAL_STATE: HubControllerState = {
  hubUrl: '',
  pairingCode: '',
  displayName: '',
  devices: [],
  busy: false,
  error: '',
  success: '',
  paired: false,
};

export function createHubController(
  isRuntimeAvailable: () => boolean,
  runtime: HubRuntime = {
    api: {
      listHubDevices,
      pairHubDevice,
      renameHubDevice,
      rotateHubDeviceToken,
      disconnectHubDevice,
    },
    storage: window.localStorage,
    confirm: (message) => window.confirm(message),
  },
) {
  const state = writable<HubControllerState>({ ...INITIAL_STATE });

  function update(patch: Partial<HubControllerState>) {
    state.update((current) => ({ ...current, ...patch }));
  }

  function restoreOrigin() {
    update({ hubUrl: runtime.storage.getItem(HUB_ORIGIN_KEY) ?? '' });
  }

  async function runAction(action: (current: HubControllerState) => Promise<void>) {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable()) return;
    update({ busy: true, error: '', success: '' });
    try {
      await action(current);
    } catch (error) {
      update({ error: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ busy: false });
    }
  }

  async function refreshDevices() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable() || !current.hubUrl.trim()) return;
    await runAction(async (current) => {
      const hubUrl = current.hubUrl.trim();
      const result = await runtime.api.listHubDevices(hubUrl);
      update({ devices: result.devices, paired: true });
      runtime.storage.setItem(HUB_ORIGIN_KEY, hubUrl);
    });
    const error = get(state).error;
    if (error.includes('尚未与该 Hub 配对') || error.includes('尚未保存')) {
      update({ paired: false, devices: [] });
    }
  }

  async function pair() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable()) return;
    await runAction(async (current) => {
      const result = await runtime.api.pairHubDevice(
        current.hubUrl.trim(),
        current.pairingCode.trim(),
        current.displayName.trim(),
      );
      update({
        hubUrl: result.hubOrigin,
        pairingCode: '',
        paired: true,
        success: `设备 ${result.deviceId} 配对成功，凭证已保存至操作系统安全密码库。`,
      });
      runtime.storage.setItem(HUB_ORIGIN_KEY, result.hubOrigin);
      const list = await runtime.api.listHubDevices(result.hubOrigin);
      update({ devices: list.devices });
    });
    const error = get(state).error;
    if (error.includes('配对请求连接中断') || error.includes('配对码无效')) {
      update({ pairingCode: '' });
    }
  }

  async function renameDevice() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable() || !current.displayName.trim()) return;
    await runAction(async (current) => {
      const hubUrl = current.hubUrl.trim();
      const name = await runtime.api.renameHubDevice(hubUrl, current.displayName.trim());
      update({ success: `本机在 Hub 中的名称已更新为“${name}”。` });
      const list = await runtime.api.listHubDevices(hubUrl);
      update({ devices: list.devices });
    });
  }

  async function rotateCredential() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable() || !runtime.confirm('轮换后旧设备凭证立即失效。确定继续吗？')) return;
    await runAction(async (latest) => {
      const hubUrl = latest.hubUrl.trim();
      await runtime.api.rotateHubDeviceToken(hubUrl);
      update({ success: '设备凭证已轮换，并已更新到系统安全密码库。' });
      const list = await runtime.api.listHubDevices(hubUrl);
      update({ devices: list.devices });
    });
  }

  async function disconnect() {
    const current = get(state);
    if (current.busy || !isRuntimeAvailable() || !runtime.confirm('这只会删除本机保存的 Hub 凭证。确定断开吗？')) return;
    await runAction(async (latest) => {
      await runtime.api.disconnectHubDevice(latest.hubUrl.trim());
      update({
        devices: [],
        paired: false,
        success: '本机 Hub 凭证已从系统安全密码库彻底删除。',
      });
    });
  }

  function setDisplayName(displayName: string) {
    update({ displayName });
  }

  return {
    state,
    restoreOrigin,
    refreshDevices,
    pair,
    renameDevice,
    rotateCredential,
    disconnect,
    setDisplayName,
  };
}
