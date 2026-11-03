import { get, writable } from 'svelte/store';
import { getAutostartEnabled, setAutostartEnabled } from './api';
import { applyTheme, getInitialTheme, initThemeListener, type ThemeMode } from './theme';

export type SettingsApi = {
  getAutostartEnabled: typeof getAutostartEnabled;
  setAutostartEnabled: typeof setAutostartEnabled;
};

export type SettingsRuntime = {
  api: SettingsApi;
  initialTheme: () => ThemeMode;
  initThemeListener: typeof initThemeListener;
};

export type SettingsControllerState = {
  theme: ThemeMode;
  autostartEnabled: boolean;
  autostartBusy: boolean;
  autostartError: string;
};

export function createSettingsController(
  isRuntimeAvailable: () => boolean,
  runtime: SettingsRuntime = {
    api: { getAutostartEnabled, setAutostartEnabled },
    initialTheme: getInitialTheme,
    initThemeListener,
  },
) {
  const state = writable<SettingsControllerState>({
    theme: 'dark',
    autostartEnabled: false,
    autostartBusy: false,
    autostartError: '',
  });

  function update(patch: Partial<SettingsControllerState>) {
    state.update((current) => ({ ...current, ...patch }));
  }

  function setTheme(theme: ThemeMode) {
    applyTheme(theme);
    update({ theme });
  }

  async function toggleAutostart() {
    const current = get(state);
    if (current.autostartBusy || !isRuntimeAvailable()) return;
    update({ autostartBusy: true, autostartError: '' });
    try {
      update({ autostartEnabled: await runtime.api.setAutostartEnabled(!current.autostartEnabled) });
    } catch (error) {
      update({ autostartError: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ autostartBusy: false });
    }
  }

  function mount() {
    setTheme(runtime.initialTheme());
    const unlisten = runtime.initThemeListener(() => {});
    if (isRuntimeAvailable()) {
      void runtime.api.getAutostartEnabled()
        .then((enabled) => update({ autostartEnabled: enabled }))
        .catch(() => {});
    }
    return unlisten;
  }

  return { state, setTheme, toggleAutostart, mount };
}
