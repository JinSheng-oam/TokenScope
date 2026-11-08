import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  createSettingsController,
  DEFAULT_REFRESH_INTERVAL_MS,
  type SettingsControllerState,
} from './settingsController';

function state(controller: ReturnType<typeof createSettingsController>): SettingsControllerState {
  let result = undefined as SettingsControllerState | undefined;
  controller.state.subscribe((value) => { result = value; })();
  return result!;
}

describe('settings controller', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('loads autostart without blocking and persists the toggled value', async () => {
    const api = {
      getAutostartEnabled: vi.fn().mockResolvedValue(true),
      setAutostartEnabled: vi.fn().mockResolvedValue(false),
    };
    const settings = createSettingsController(() => true, {
      api,
      initialTheme: () => 'system',
      initThemeListener: () => () => {},
    });
    settings.mount();
    await vi.waitFor(() => expect(state(settings).autostartEnabled).toBe(true));

    await settings.toggleAutostart();

    expect(api.setAutostartEnabled).toHaveBeenCalledWith(false);
    expect(state(settings)).toMatchObject({
      theme: 'system',
      autostartEnabled: false,
      autostartBusy: false,
      autostartError: '',
    });
  });

  it('waits for the actual startup state before allowing a change', async () => {
    let resolveRead!: (enabled: boolean) => void;
    const api = {
      getAutostartEnabled: vi.fn(() => new Promise<boolean>((resolve) => { resolveRead = resolve; })),
      setAutostartEnabled: vi.fn().mockResolvedValue(false),
    };
    const settings = createSettingsController(() => true, {
      api, initialTheme: () => 'system', initThemeListener: () => () => {},
    });
    settings.mount();
    expect(state(settings)).toMatchObject({ autostartLoaded: false, autostartBusy: true });
    await settings.toggleAutostart();
    expect(api.setAutostartEnabled).not.toHaveBeenCalled();

    resolveRead(true);
    await vi.waitFor(() => expect(state(settings)).toMatchObject({ autostartLoaded: true, autostartEnabled: true, autostartBusy: false }));
    await settings.toggleAutostart();
    expect(api.setAutostartEnabled).toHaveBeenCalledWith(false);
  });

  it('reports an unknown state on failure and can reload the system setting', async () => {
    const api = {
      getAutostartEnabled: vi.fn().mockRejectedValueOnce(new Error('读取失败')).mockResolvedValue(true),
      setAutostartEnabled: vi.fn().mockRejectedValue(new Error('写入失败')),
    };
    const settings = createSettingsController(() => true, {
      api, initialTheme: () => 'dark', initThemeListener: () => () => {},
    });
    settings.mount();
    await vi.waitFor(() => expect(state(settings)).toMatchObject({ autostartLoaded: false, autostartError: '读取失败' }));
    await settings.toggleAutostart();
    expect(api.setAutostartEnabled).not.toHaveBeenCalled();
    await settings.refreshAutostart();
    expect(state(settings)).toMatchObject({ autostartLoaded: true, autostartEnabled: true, autostartError: '' });
    await settings.toggleAutostart();
    expect(state(settings)).toMatchObject({ autostartLoaded: false, autostartError: '写入失败' });
    await settings.refreshAutostart();
    expect(state(settings)).toMatchObject({ autostartLoaded: true, autostartEnabled: true, autostartError: '' });
  });

  it('persists a supported refresh interval and ignores unknown values', () => {
    const values = new Map<string, string>();
    vi.stubGlobal('window', {
      localStorage: {
        getItem: (key: string) => values.get(key) ?? null,
        setItem: (key: string, value: string) => values.set(key, value),
      },
    });
    const settings = createSettingsController(() => false, {
      api: {
        getAutostartEnabled: vi.fn().mockResolvedValue(false),
        setAutostartEnabled: vi.fn().mockResolvedValue(false),
      },
      initialTheme: () => 'dark',
      initThemeListener: () => () => {},
    });

    expect(state(settings).refreshIntervalMs).toBe(DEFAULT_REFRESH_INTERVAL_MS);
    settings.setRefreshInterval(15_000);
    expect(state(settings).refreshIntervalMs).toBe(15_000);
    expect(values.get('tokenscope.refreshIntervalMs')).toBe('15000');

    settings.setRefreshInterval(12_345);
    expect(state(settings).refreshIntervalMs).toBe(15_000);
  });
});
