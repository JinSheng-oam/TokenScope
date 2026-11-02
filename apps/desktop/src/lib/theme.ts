export type ThemeMode = 'dark' | 'light' | 'system';

const STORAGE_KEY = 'tokenscope.theme';

export function getInitialTheme(): ThemeMode {
  if (typeof window === 'undefined') return 'dark';
  const saved = window.localStorage.getItem(STORAGE_KEY) as ThemeMode | null;
  if (saved === 'dark' || saved === 'light' || saved === 'system') return saved;
  return 'dark';
}

export function applyTheme(mode: ThemeMode): void {
  if (typeof document === 'undefined') return;
  const isDark =
    mode === 'dark' ||
    (mode === 'system' && window.matchMedia('(prefers-color-scheme: dark)').matches);

  document.documentElement.setAttribute('data-theme', isDark ? 'dark' : 'light');
  window.localStorage.setItem(STORAGE_KEY, mode);
}

export function initThemeListener(onChange: (isDark: boolean) => void): () => void {
  if (typeof window === 'undefined') return () => {};
  const media = window.matchMedia('(prefers-color-scheme: dark)');
  const listener = (e: MediaQueryListEvent) => {
    const current = window.localStorage.getItem(STORAGE_KEY) as ThemeMode | null;
    if (current === 'system') {
      document.documentElement.setAttribute('data-theme', e.matches ? 'dark' : 'light');
      onChange(e.matches);
    }
  };
  media.addEventListener('change', listener);
  return () => media.removeEventListener('change', listener);
}
