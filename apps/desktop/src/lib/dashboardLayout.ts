export type CardId =
  | 'metrics'
  | 'integrity'
  | 'trend'
  | 'tools'
  | 'models'
  | 'sessions'
  | 'sources'
  | 'projects'
  | 'quotas'
  | 'cost'
  | 'efficiency'
  | 'devices'
  | 'activity'
  | 'resets';

export type CardWidth = '1/3' | '1/2' | '2/3' | 'full';

export interface DashboardCardConfig {
  id: CardId;
  visible: boolean;
  width: CardWidth;
  rows?: CardRows;
}

export interface CardMeta {
  id: CardId;
  title: string;
  description: string;
  defaultWidth: CardWidth;
  canResize: boolean;
  canSetRows?: boolean;
  minWidth?: CardWidth;
}

export const CARD_CATALOG: Record<CardId, CardMeta> = {
  metrics: {
    id: 'metrics',
    title: '核心用量指标',
    description: '总 Token、输入、输出、缓存及估算费用核心指标卡',
    defaultWidth: 'full',
    canResize: false,
  },
  integrity: {
    id: 'integrity',
    title: '数据校验横幅',
    description: '精确度（精确/聚合/估算）及时间戳有效性校验横幅',
    defaultWidth: 'full',
    canResize: false,
  },
  trend: {
    id: 'trend',
    title: 'Token 使用趋势',
    description: 'Token 每日消耗趋势分析，支持曲线图与热力图视图切换',
    defaultWidth: '2/3',
    canResize: true,
  },
  tools: {
    id: 'tools',
    title: '工具用量分布',
    description: 'Codex、Claude、Antigravity 等各开发工具消耗排行与占比',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  models: {
    id: 'models',
    title: '模型用量分布',
    description: '各 AI 模型（GPT-4、Claude 3.7、Gemini 等）用量排行、费用与占比',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  sessions: {
    id: 'sessions',
    title: '最近会话动态',
    description: '实时会话活动检测、任务生成状态与 Token 消耗速率',
    defaultWidth: '1/2',
    canResize: true,
    canSetRows: true,
  },
  sources: {
    id: 'sources',
    title: '采集源状态',
    description: '本机各采集工具检查点扫描状态与健康度监控',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  projects: {
    id: 'projects',
    title: '项目用量分布',
    description: '关联工作区项目 Token 消耗分布与排行榜',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  quotas: {
    id: 'quotas',
    title: '额度与限额监控',
    description: 'Codex 与 Antigravity 实时剩余额度、耗尽预警与重置倒计时',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  cost: {
    id: 'cost',
    title: '费用估算分析',
    description: '总预估费用、计价覆盖率及按模型消耗费用排行榜',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  efficiency: {
    id: 'efficiency',
    title: 'Token 构成与效率',
    description: '输入/输出/推理/缓存结构拆解、缓存命中率与单笔均耗',
    defaultWidth: '1/3',
    canResize: true,
  },
  devices: {
    id: 'devices',
    title: '多设备与同步',
    description: '本机与远端设备用量对比以及跨端同步引擎状态',
    defaultWidth: '1/3',
    canResize: true,
    canSetRows: true,
  },
  activity: {
    id: 'activity',
    title: '日均与峰值统计',
    description: '活跃日均消耗、单日峰值、连续活跃天数与近 14 日走势',
    defaultWidth: '1/3',
    canResize: true,
  },
  resets: {
    id: 'resets',
    title: '额度重置倒计时',
    description: '按时间顺序列出今天与后续即将重置的额度池倒计时',
    defaultWidth: '1/3',
    canResize: true,
  },
};

export const DEFAULT_DASHBOARD_CARDS: DashboardCardConfig[] = [
  { id: 'metrics', visible: true, width: 'full' },
  { id: 'trend', visible: true, width: '2/3' },
  { id: 'models', visible: true, width: '1/3' },
  { id: 'sessions', visible: true, width: '2/3' },
  { id: 'sources', visible: true, width: '1/3' },
  { id: 'integrity', visible: false, width: 'full' },
  { id: 'tools', visible: false, width: '1/3' },
  { id: 'projects', visible: false, width: '1/2' },
  { id: 'quotas', visible: false, width: '1/3' },
  { id: 'cost', visible: false, width: '1/3' },
  { id: 'efficiency', visible: false, width: '1/3' },
  { id: 'devices', visible: false, width: '1/3' },
  { id: 'activity', visible: false, width: '1/3' },
  { id: 'resets', visible: false, width: '1/3' },
];

export const STORAGE_KEY = 'tokenscope.dashboardLayout.v3';
const GAP_STORAGE_KEY = 'tokenscope.dashboardCardGap';
const ROWS_STORAGE_KEY = 'tokenscope.dashboardCardRows';
export const CARD_GAP_OPTIONS = [
  { value: 8, label: '紧凑' },
  { value: 14, label: '默认' },
  { value: 22, label: '宽松' },
] as const;
export type CardGap = (typeof CARD_GAP_OPTIONS)[number]['value'];
export const DEFAULT_CARD_GAP: CardGap = 14;

export const CARD_ROWS_OPTIONS = [
  { value: 3, label: '3 行' },
  { value: 5, label: '5 行' },
  { value: 8, label: '8 行' },
  { value: 10, label: '10 行' },
] as const;
export type CardRows = (typeof CARD_ROWS_OPTIONS)[number]['value'];
export const DEFAULT_CARD_ROWS: CardRows = 5;

export function loadDashboardCardGap(): CardGap {
  try {
    const stored = localStorage.getItem(GAP_STORAGE_KEY);
    return CARD_GAP_OPTIONS.find(({ value }) => String(value) === stored)?.value ?? DEFAULT_CARD_GAP;
  } catch {
    return DEFAULT_CARD_GAP;
  }
}

export function saveDashboardCardGap(gap: CardGap): void {
  try {
    localStorage.setItem(GAP_STORAGE_KEY, String(gap));
  } catch {
    // Keep the active setting when storage is unavailable.
  }
}

export function loadDashboardCardRows(): CardRows {
  try {
    const stored = localStorage.getItem(ROWS_STORAGE_KEY);
    return CARD_ROWS_OPTIONS.find(({ value }) => String(value) === stored)?.value ?? DEFAULT_CARD_ROWS;
  } catch {
    return DEFAULT_CARD_ROWS;
  }
}

export function saveDashboardCardRows(rows: CardRows): void {
  try {
    localStorage.setItem(ROWS_STORAGE_KEY, String(rows));
  } catch {
    // Keep the active setting when storage is unavailable.
  }
}

const VALID_CARD_IDS = new Set<CardId>([
  'metrics',
  'integrity',
  'trend',
  'tools',
  'models',
  'sessions',
  'sources',
  'projects',
  'quotas',
  'cost',
  'efficiency',
  'devices',
  'activity',
  'resets',
]);

const VALID_WIDTHS = new Set<CardWidth>(['1/3', '1/2', '2/3', 'full']);

export function isValidCardId(id: unknown): id is CardId {
  return typeof id === 'string' && VALID_CARD_IDS.has(id as CardId);
}

const VALID_ROWS = new Set<number>(CARD_ROWS_OPTIONS.map((o) => o.value));

export function isValidCardRows(rows: unknown): rows is CardRows {
  return typeof rows === 'number' && VALID_ROWS.has(rows);
}

export function isValidWidth(width: unknown): width is CardWidth {
  return typeof width === 'string' && VALID_WIDTHS.has(width as CardWidth);
}

export function loadDashboardLayout(): DashboardCardConfig[] {
  if (typeof localStorage === 'undefined') {
    return DEFAULT_DASHBOARD_CARDS.map((c) => ({ ...c }));
  }

  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return DEFAULT_DASHBOARD_CARDS.map((c) => ({ ...c }));
    }

    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed) || parsed.length === 0) {
      return DEFAULT_DASHBOARD_CARDS.map((c) => ({ ...c }));
    }

    const seen = new Set<CardId>();
    const loadedCards: DashboardCardConfig[] = [];

    for (const item of parsed) {
      if (item && typeof item === 'object' && 'id' in item && isValidCardId((item as { id: unknown }).id)) {
        const id = (item as { id: CardId }).id;
        if (!seen.has(id)) {
          seen.add(id);
          const meta = CARD_CATALOG[id];
          const rawWidth = 'width' in item ? (item as { width: unknown }).width : undefined;
          const width = meta.canResize && isValidWidth(rawWidth) ? rawWidth : meta.defaultWidth;
          const rawRows = 'rows' in item ? (item as { rows: unknown }).rows : undefined;
          const rows = meta.canSetRows && isValidCardRows(rawRows) ? rawRows : undefined;
          loadedCards.push({
            id,
            visible: Boolean((item as { visible?: unknown }).visible),
            width,
            ...(rows !== undefined ? { rows } : {}),
          });
        }
      }
    }

    // Merge any missing cards from catalog so new cards are available
    for (const def of DEFAULT_DASHBOARD_CARDS) {
      if (!seen.has(def.id)) {
        loadedCards.push({ ...def });
      }
    }

    return loadedCards;
  } catch {
    return DEFAULT_DASHBOARD_CARDS.map((c) => ({ ...c }));
  }
}

export function saveDashboardLayout(cards: DashboardCardConfig[]): void {
  if (typeof localStorage === 'undefined') return;
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(cards));
  } catch {
    // Ignore storage quota or security errors
  }
}

export function resetDashboardLayout(): DashboardCardConfig[] {
  if (typeof localStorage !== 'undefined') {
    try {
      localStorage.removeItem(STORAGE_KEY);
      localStorage.removeItem(GAP_STORAGE_KEY);
      localStorage.removeItem(ROWS_STORAGE_KEY);
    } catch {
      // Ignore
    }
  }
  return DEFAULT_DASHBOARD_CARDS.map((c) => ({ ...c }));
}

export function reorderCards(
  cards: DashboardCardConfig[],
  fromIndex: number,
  toIndex: number
): DashboardCardConfig[] {
  if (
    fromIndex < 0 ||
    fromIndex >= cards.length ||
    toIndex < 0 ||
    toIndex >= cards.length ||
    fromIndex === toIndex
  ) {
    return cards;
  }

  const next = [...cards];
  const [removed] = next.splice(fromIndex, 1);
  next.splice(toIndex, 0, removed);
  return next;
}

export function updateCardWidth(
  cards: DashboardCardConfig[],
  id: CardId,
  width: CardWidth
): DashboardCardConfig[] {
  const meta = CARD_CATALOG[id];
  if (!meta || !meta.canResize || !isValidWidth(width)) {
    return cards;
  }

  return cards.map((c) => (c.id === id ? { ...c, width } : c));
}

export function updateCardRows(
  cards: DashboardCardConfig[],
  id: CardId,
  rows: CardRows
): DashboardCardConfig[] {
  const meta = CARD_CATALOG[id];
  if (!meta || !meta.canSetRows || !isValidCardRows(rows)) {
    return cards;
  }

  return cards.map((c) => (c.id === id ? { ...c, rows } : c));
}

export function setCardVisibility(
  cards: DashboardCardConfig[],
  id: CardId,
  visible: boolean
): DashboardCardConfig[] {
  return cards.map((c) => (c.id === id ? { ...c, visible } : c));
}
