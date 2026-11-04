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
