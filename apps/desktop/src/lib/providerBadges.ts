import type { Provider } from './agentCustomizationEntries';

export type ProviderBadgeInfo = {
  id: Provider;
  label: string;
  shortLabel: string;
  color: string;
  bg: string;
  icon: string;
};

export const ALL_PROVIDERS: ProviderBadgeInfo[] = [
  {
    id: 'claude',
    label: 'Claude Code',
    shortLabel: 'Claude',
    color: '#d97757',
    bg: 'rgba(217, 119, 87, 0.14)',
    icon: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"><path d="M8 2v12M2 8h12M3.8 3.8l8.4 8.4M12.2 3.8l-8.4 8.4"/></svg>`,
  },
  {
    id: 'codex',
    label: 'Codex',
    shortLabel: 'Codex',
    color: '#10a37f',
    bg: 'rgba(16, 163, 127, 0.14)',
    icon: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="8" cy="8" r="5.5"/><path d="M8 4.8v6.4M5.2 6.4l5.6 3.2M10.8 6.4l-5.6 3.2"/></svg>`,
  },
  {
    id: 'antigravity',
    label: 'Antigravity',
    shortLabel: 'AGY',
    color: '#4f5bd5',
    bg: 'rgba(79, 91, 213, 0.14)',
    icon: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M8 2.2 9.6 6.4 13.8 8 9.6 9.6 8 13.8 6.4 9.6 2.2 8l4.2-1.6Z"/></svg>`,
  },
  {
    id: 'pi',
    label: 'Pi Agent',
    shortLabel: 'Pi',
    color: '#475569',
    bg: 'rgba(71, 85, 105, 0.14)',
    icon: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"><path d="M3 4.5h10M6 4.5v7.5M10.5 4.5v5.8c0 1 .6 1.7 1.5 1.7"/></svg>`,
  },
  {
    id: 'deepseek',
    label: 'DeepSeek Harness',
    shortLabel: 'DS',
    color: '#2563eb',
    bg: 'rgba(37, 99, 235, 0.14)',
    icon: `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><path d="M2.5 9.5c1.5-4 5.5-5.5 9.5-3.5 1.2.6 1.8 1.8 1.5 3.1-.5 2-2.8 3.4-5.7 3.4-2.3 0-4.2-.9-5.3-3Z"/><circle cx="9.8" cy="7.8" r="0.8" fill="currentColor" stroke="none"/></svg>`,
  },
];

export const PROVIDER_BADGE_MAP: Record<Provider, ProviderBadgeInfo> = Object.fromEntries(
  ALL_PROVIDERS.map((p) => [p.id, p])
) as Record<Provider, ProviderBadgeInfo>;
