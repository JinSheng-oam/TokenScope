import { describe, expect, it, vi } from 'vitest';
import { createHubController, type HubControllerState } from './hubController';
import type { RemoteDevice } from './types';

function device(id: string): RemoteDevice {
  return {
    device_id: id,
    display_name: id,
    os_name: 'windows',
    app_version: '0.1.0',
    time_zone: 'Asia/Shanghai',
    created_at_ms: 1,
    last_seen_ms: 1,
    revoked_at_ms: null,
  };
}

function state(controller: ReturnType<typeof createHubController>): HubControllerState {
  let result = undefined as HubControllerState | undefined;
  controller.state.subscribe((value) => { result = value; })();
  return result!;
}

describe('hub controller', () => {
  it('pairs once, clears the one-use code and persists only the origin', async () => {
    const values = new Map<string, string>();
    const api = {
      pairHubDevice: vi.fn().mockResolvedValue({
        hubOrigin: 'https://hub.example',
        deviceId: 'device-1',
        createdAtMs: 1,
      }),
      listHubDevices: vi.fn().mockResolvedValue({
        protocol_version: 1,
        devices: [device('device-1')],
      }),
      renameHubDevice: vi.fn(),
      rotateHubDeviceToken: vi.fn(),
      disconnectHubDevice: vi.fn(),
    };
    const storage = {
      getItem: (key: string) => values.get(key) ?? null,
      setItem: (key: string, value: string) => { values.set(key, value); },
    };
    const hub = createHubController(() => true, {
      api,
      storage,
      confirm: () => true,
    });
    hub.state.update((value) => ({
      ...value,
      hubUrl: 'https://hub.example',
      pairingCode: 'ONE-TIME-CODE',
      displayName: 'Laptop',
    }));

    await hub.pair();

    expect(api.pairHubDevice).toHaveBeenCalledWith(
      'https://hub.example',
      'ONE-TIME-CODE',
      'Laptop',
    );
    expect(state(hub)).toMatchObject({
      hubUrl: 'https://hub.example',
      pairingCode: '',
      paired: true,
      devices: [device('device-1')],
    });
    expect([...values.entries()]).toEqual([['tokenscope.hubUrl', 'https://hub.example']]);
  });

  it('requires explicit confirmation before rotating a credential', async () => {
    const rotateHubDeviceToken = vi.fn().mockResolvedValue(1);
    const hub = createHubController(() => true, {
      api: {
        pairHubDevice: vi.fn(),
        listHubDevices: vi.fn().mockResolvedValue({ protocol_version: 1, devices: [] }),
        renameHubDevice: vi.fn(),
        rotateHubDeviceToken,
        disconnectHubDevice: vi.fn(),
      },
      storage: { getItem: () => null, setItem: () => {} },
      confirm: () => false,
    });
    hub.state.update((value) => ({ ...value, hubUrl: 'https://hub.example' }));

    await hub.rotateCredential();

    expect(rotateHubDeviceToken).not.toHaveBeenCalled();
  });
});
