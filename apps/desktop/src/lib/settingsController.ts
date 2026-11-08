import { get, writable } from 'svelte/store';
import { getAutostartEnabled, setAutostartEnabled } from './api';
import { applyTheme, getInitialTheme, initThemeListener, type ThemeMode } from './theme';

const ROLLING_STORAGE_KEY = 'tokenscope.rollingNumbers';
const REFRESH_INTERVAL_STORAGE_KEY = 'tokenscope.refreshIntervalMs';
export const DEFAULT_REFRESH_INTERVAL_MS = 300_000;
export const REFRESH_INTERVAL_OPTIONS = [15_000, 30_000, 60_000, 300_000, 900_000, 0] as const;
export type RefreshIntervalMs = (typeof REFRESH_INTERVAL_OPTIONS)[number];

function isRefreshIntervalMs(value: number): value is RefreshIntervalMs {
  return (REFRESH_INTERVAL_OPTIONS as readonly number[]).includes(value);
}

function readRefreshIntervalMs(): RefreshIntervalMs {
  if (typeof window === 'undefined') return DEFAULT_REFRESH_INTERVAL_MS;
  try {
    const raw = window.localStorage.getItem(REFRESH_INTERVAL_STORAGE_KEY);
    const parsed = raw === null ? Number.NaN : Number(raw);
    return isRefreshIntervalMs(parsed) ? parsed : DEFAULT_REFRESH_INTERVAL_MS;
  } catch {
    return DEFAULT_REFRESH_INTERVAL_MS;
  }
}

function writeRefreshIntervalMs(intervalMs: RefreshIntervalMs): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(REFRESH_INTERVAL_STORAGE_KEY, String(intervalMs));
  } catch {
    // Ignore storage write failures in restricted environments.
  }
}

function readRollingNumbersEnabled(): boolean {
  if (typeof window === 'undefined') return true;
  try {
    const raw = window.localStorage.getItem(ROLLING_STORAGE_KEY);
    return raw === null ? true : raw !== 'false';
  } catch {
    return true;
  }
}

function writeRollingNumbersEnabled(enabled: boolean): void {
  if (typeof window === 'undefined') return;
  try {
    window.localStorage.setItem(ROLLING_STORAGE_KEY, String(enabled));
  } catch {
    // Ignore storage write failures in restricted environments.
  }
}

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
  autostartLoaded: boolean;
  autostartBusy: boolean;
  autostartError: string;
  rollingNumbersEnabled: boolean;
  refreshIntervalMs: RefreshIntervalMs;
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
    autostartLoaded: false,
    autostartBusy: false,
    autostartError: '',
    rollingNumbersEnabled: readRollingNumbersEnabled(),
    refreshIntervalMs: readRefreshIntervalMs(),
  });

  function update(patch: Partial<SettingsControllerState>) {
    state.update((current) => ({ ...current, ...patch }));
  }

  function setTheme(theme: ThemeMode) {
    applyTheme(theme);
    update({ theme });
  }

  function toggleRollingNumbers() {
    const next = !get(state).rollingNumbersEnabled;
    writeRollingNumbersEnabled(next);
    update({ rollingNumbersEnabled: next });
  }

  function setRefreshInterval(intervalMs: number) {
    if (!isRefreshIntervalMs(intervalMs)) return;
    writeRefreshIntervalMs(intervalMs);
    update({ refreshIntervalMs: intervalMs });
  }

  async function toggleAutostart() {
    const current = get(state);
    if (current.autostartBusy || !current.autostartLoaded || !isRuntimeAvailable()) return;
    update({ autostartBusy: true, autostartError: '' });
    try {
      update({ autostartEnabled: await runtime.api.setAutostartEnabled(!current.autostartEnabled) });
    } catch (error) {
      update({ autostartLoaded: false, autostartError: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ autostartBusy: false });
    }
  }

  async function refreshAutostart() {
    if (get(state).autostartBusy || !isRuntimeAvailable()) return;
    update({ autostartBusy: true, autostartError: '' });
    try {
      update({ autostartEnabled: await runtime.api.getAutostartEnabled(), autostartLoaded: true });
    } catch (error) {
      update({ autostartLoaded: false, autostartError: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ autostartBusy: false });
    }
  }

  function mount() {
    setTheme(runtime.initialTheme());
    update({
      rollingNumbersEnabled: readRollingNumbersEnabled(),
      refreshIntervalMs: readRefreshIntervalMs(),
    });
    const unlisten = runtime.initThemeListener(() => {});
    if (isRuntimeAvailable()) {
      void refreshAutostart();
    }
    return unlisten;
  }

  return { state, setTheme, toggleRollingNumbers, setRefreshInterval, toggleAutostart, refreshAutostart, mount };
}
