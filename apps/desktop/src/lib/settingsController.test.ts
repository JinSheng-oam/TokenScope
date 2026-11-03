import { describe, expect, it, vi } from 'vitest';
import { createSettingsController, type SettingsControllerState } from './settingsController';

function state(controller: ReturnType<typeof createSettingsController>): SettingsControllerState {
  let result = undefined as SettingsControllerState | undefined;
  controller.state.subscribe((value) => { result = value; })();
  return result!;
}

describe('settings controller', () => {
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
});
