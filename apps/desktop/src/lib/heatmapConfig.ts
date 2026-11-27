import { writable } from 'svelte/store';

export type HeatmapRangeMode = 'all' | '12w' | '26w' | '52w' | 'custom';
export type HeatmapThresholdMode = 'auto' | 'custom';
export type HeatmapColorScheme = 'emerald' | 'indigo' | 'amber' | 'rose' | 'cyan' | 'custom';

export interface HeatmapConfig {
  range: HeatmapRangeMode;
  customStart: string;
  customEnd: string;
  thresholdMode: HeatmapThresholdMode;
  /** Daily token thresholds in millions (M) for level 2, level 3, level 4. */
  customThresholdsM: [number, number, number];
  colorScheme: HeatmapColorScheme;
  customColor: string;
}

export const HEATMAP_RANGE_OPTIONS: { value: HeatmapRangeMode; label: string }[] = [
  { value: 'all', label: '全部' },
  { value: '12w', label: '近3月' },
  { value: '26w', label: '近半年' },
  { value: '52w', label: '近1年' },
  { value: 'custom', label: '自定义' },
];

export const HEATMAP_COLOR_PRESETS: { id: Exclude<HeatmapColorScheme, 'custom'>; label: string; color: string }[] = [
  { id: 'emerald', label: '翠绿', color: '#10b981' },
  { id: 'indigo', label: '靛蓝', color: '#6366f1' },
  { id: 'amber', label: '琥珀', color: '#f59e0b' },
  { id: 'rose', label: '玫瑰', color: '#f43f5e' },
  { id: 'cyan', label: '青空', color: '#06b6d4' },
];

export const DEFAULT_HEATMAP_CONFIG: HeatmapConfig = {
  range: 'all',
  customStart: '',
  customEnd: '',
  thresholdMode: 'auto',
  customThresholdsM: [5, 20, 60],
  colorScheme: 'emerald',
  customColor: '#10b981',
};

const STORAGE_KEY = 'tokenscope:heatmap-config:v1';
const ISO_DATE_RE = /^\d{4}-\d{2}-\d{2}$/;
const HEX_COLOR_RE = /^#[0-9a-fA-F]{6}$/;

function sanitizeThresholdM(value: unknown, fallback: number): number {
  const num = Number(value);
  if (!Number.isFinite(num) || num <= 0) return fallback;
  return Math.min(1_000_000, Math.round(num * 100) / 100);
}

export function normalizeHeatmapConfig(raw: Partial<HeatmapConfig> | null | undefined): HeatmapConfig {
  if (!raw || typeof raw !== 'object') return { ...DEFAULT_HEATMAP_CONFIG, customThresholdsM: [...DEFAULT_HEATMAP_CONFIG.customThresholdsM] };

  const validRanges: HeatmapRangeMode[] = ['all', '12w', '26w', '52w', 'custom'];
  const validSchemes: HeatmapColorScheme[] = ['emerald', 'indigo', 'amber', 'rose', 'cyan', 'custom'];
  const range = validRanges.includes(raw.range as HeatmapRangeMode) ? (raw.range as HeatmapRangeMode) : DEFAULT_HEATMAP_CONFIG.range;
  const customStart = typeof raw.customStart === 'string' && ISO_DATE_RE.test(raw.customStart) ? raw.customStart : '';
  const customEnd = typeof raw.customEnd === 'string' && ISO_DATE_RE.test(raw.customEnd) ? raw.customEnd : '';
  const thresholdMode: HeatmapThresholdMode = raw.thresholdMode === 'custom' ? 'custom' : 'auto';

  const rawTuple = Array.isArray(raw.customThresholdsM) ? raw.customThresholdsM : DEFAULT_HEATMAP_CONFIG.customThresholdsM;
  const t2 = sanitizeThresholdM(rawTuple[0], DEFAULT_HEATMAP_CONFIG.customThresholdsM[0]);
  const t3 = sanitizeThresholdM(rawTuple[1], DEFAULT_HEATMAP_CONFIG.customThresholdsM[1]);
  const t4 = sanitizeThresholdM(rawTuple[2], DEFAULT_HEATMAP_CONFIG.customThresholdsM[2]);
  const sorted = [t2, t3, t4].sort((a, b) => a - b) as [number, number, number];

  const colorScheme = validSchemes.includes(raw.colorScheme as HeatmapColorScheme)
    ? (raw.colorScheme as HeatmapColorScheme)
    : DEFAULT_HEATMAP_CONFIG.colorScheme;
  const customColor =
    typeof raw.customColor === 'string' && HEX_COLOR_RE.test(raw.customColor)
      ? raw.customColor
      : DEFAULT_HEATMAP_CONFIG.customColor;

  return {
    range,
    customStart,
    customEnd,
    thresholdMode,
    customThresholdsM: sorted,
    colorScheme,
    customColor,
  };
}

function loadHeatmapConfig(): HeatmapConfig {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return normalizeHeatmapConfig(null);
    return normalizeHeatmapConfig(JSON.parse(raw) as Partial<HeatmapConfig>);
  } catch {
    return normalizeHeatmapConfig(null);
  }
}

function persistHeatmapConfig(config: HeatmapConfig) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  } catch {
    /* Ignore storage quota errors. */
  }
}

export const heatmapConfig = writable<HeatmapConfig>(loadHeatmapConfig());

export function updateHeatmapConfig(patch: Partial<HeatmapConfig>) {
  heatmapConfig.update((current) => {
    const next = normalizeHeatmapConfig({ ...current, ...patch });
    persistHeatmapConfig(next);
    return next;
  });
}

export function resetHeatmapConfig() {
  const next = normalizeHeatmapConfig(null);
  heatmapConfig.set(next);
  persistHeatmapConfig(next);
}

export function resolveHeatmapAccentColor(config: HeatmapConfig): string | null {
  if (config.colorScheme === 'emerald') return null;
  if (config.colorScheme === 'custom') return config.customColor;
  return HEATMAP_COLOR_PRESETS.find((preset) => preset.id === config.colorScheme)?.color ?? null;
}

export function resolveHeatmapPaletteStyle(config: HeatmapConfig): string {
  const accent = resolveHeatmapAccentColor(config);
  if (!accent) return '';
  return [
    `--heatmap-1: color-mix(in srgb, ${accent} 24%, var(--heatmap-0))`,
    `--heatmap-2: color-mix(in srgb, ${accent} 48%, var(--heatmap-0))`,
    `--heatmap-3: color-mix(in srgb, ${accent} 74%, var(--heatmap-0))`,
    `--heatmap-4: ${accent}`,
  ].join('; ');
}

export function describeHeatmapRangeMode(config: HeatmapConfig): string {
  switch (config.range) {
    case '12w':
      return '近 12 周';
    case '26w':
      return '近半年';
    case '52w':
      return '近 1 年';
    case 'custom':
      return '自定义范围';
    default:
      return '完整历史';
  }
}
