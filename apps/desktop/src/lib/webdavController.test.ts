import { describe, expect, it, vi } from 'vitest';
import { createWebDavController, type WebDavControllerState } from './webdavController';
import type { RemoteDevice } from './types';

function mockDevice(id: string): RemoteDevice {
  return {
    device_id: id,
    display_name: id,
    os_name: 'windows',
    app_version: '0.1.0',
    time_zone: 'Asia/Shanghai',
    created_at_ms: 1000,
    last_seen_ms: 2000,
    revoked_at_ms: null,
  };
}

function getState(controller: ReturnType<typeof createWebDavController>): WebDavControllerState {
  let result = undefined as WebDavControllerState | undefined;
  controller.state.subscribe((value) => {
    result = value;
  })();
  return result!;
}

describe('webdav controller', () => {
  it('tests connection, saves config and wipes password from memory', async () => {
    const api = {
      testWebDavConnection: vi.fn().mockResolvedValue(true),
      saveWebDavConfig: vi.fn().mockResolvedValue(true),
      getWebDavConfig: vi.fn().mockResolvedValue({
        enabled: true,
        url: 'https://dav.jianguoyun.com/dav/TokenScope/',
        username: 'user@example.com',
      }),
      disconnectWebDav: vi.fn().mockResolvedValue(true),
      listWebDavDevices: vi.fn().mockResolvedValue({
        protocol_version: 1,
        devices: [mockDevice('node-mac')],
      }),
      syncWebDavNow: vi.fn().mockResolvedValue({
        uploaded_records: 10,
        uploaded_batches: 1,
        pulled_records: 5,
        pulled_batches: 1,
        remaining_due: 0,
      }),
    };

    const controller = createWebDavController(() => true, {
      api,
      confirm: () => true,
    });

    controller.setUrl('https://dav.jianguoyun.com/dav/TokenScope/');
    controller.setUsername('user@example.com');
    controller.setPassword('app_password_xyz');

    const testOk = await controller.testConnection();
    expect(testOk).toBe(true);
    expect(api.testWebDavConnection).toHaveBeenCalledWith(
      'https://dav.jianguoyun.com/dav/TokenScope/',
      'user@example.com',
      'app_password_xyz',
    );

    await controller.saveConfig();

    expect(api.saveWebDavConfig).toHaveBeenCalledWith(
      'https://dav.jianguoyun.com/dav/TokenScope/',
      'user@example.com',
      'app_password_xyz',
    );
    expect(api.syncWebDavNow).toHaveBeenCalled();
    expect(api.listWebDavDevices).toHaveBeenCalled();

    const current = getState(controller);
    expect(current.enabled).toBe(true);
    // Password must be purged from in-memory state
    expect(current.password).toBe('');
    expect(current.devices.length).toBe(1);
    expect(current.devices[0].device_id).toBe('node-mac');
  });

  it('disconnect clears state after confirmation', async () => {
    const api = {
      testWebDavConnection: vi.fn(),
      saveWebDavConfig: vi.fn(),
      getWebDavConfig: vi.fn(),
      disconnectWebDav: vi.fn().mockResolvedValue(true),
      listWebDavDevices: vi.fn(),
      syncWebDavNow: vi.fn(),
    };

    const controller = createWebDavController(() => true, {
      api,
      confirm: () => true,
    });

    controller.state.update((s) => ({
      ...s,
      enabled: true,
      url: 'https://dav.example.com',
      devices: [mockDevice('dev-1')],
    }));

    await controller.disconnect();
    expect(api.disconnectWebDav).toHaveBeenCalled();
    const current = getState(controller);
    expect(current.enabled).toBe(false);
    expect(current.url).toBe('');
    expect(current.devices).toEqual([]);
  });
});
