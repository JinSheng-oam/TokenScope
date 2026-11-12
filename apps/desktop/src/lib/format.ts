import { USD_DISPLAY, type DisplayCurrency } from './displayCurrency';

export function formatNumber(value: number | null | undefined): string {
  return new Intl.NumberFormat('zh-CN', { maximumFractionDigits: 0 }).format(value ?? 0);
}

const toolLabels: Record<string, string> = {
  codex: 'Codex',
  claude: 'Claude',
  antigravity: 'Antigravity',
  pi: 'Pi Agent',
  'deepseek-harness': 'DeepSeek Harness',
};

export function formatToolLabel(toolId: string): string {
  return toolLabels[toolId] ?? toolId;
}

export function formatCost(microUsd: number | null | undefined, display: DisplayCurrency = USD_DISPLAY): string {
  if (microUsd === null || microUsd === undefined) return '暂无价格数据';
  const places = display.costDecimalPlaces;
  const amount = (microUsd / 1_000_000) * display.multiplier;
  const formatter = new Intl.NumberFormat(display.code === 'CNY' ? 'zh-CN' : 'en-US', {
    style: 'currency', currency: display.code, minimumFractionDigits: places, maximumFractionDigits: places,
  });
  const smallestUnit = 10 ** -places;
  return amount > 0 && amount < smallestUnit / 2
    ? `< ${formatter.format(smallestUnit)}`
    : formatter.format(amount);
}

export function formatPercent(basisPoints: number | null | undefined): string {
  if (basisPoints === null || basisPoints === undefined) return '未知';
  return `${(basisPoints / 100).toFixed(1)}%`;
}

export function relativeTime(timestamp: number | null | undefined, now = Date.now()): string {
  if (timestamp === null || timestamp === undefined) return '从未采集';
  const diff = Math.max(0, now - timestamp);
  if (diff < 60_000) return '刚刚';
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`;
  return `${Math.floor(diff / 86_400_000)} 天前`;
}

export function percentOf(value: number, total: number): number {
  if (total <= 0 || value <= 0) return 0;
  return Math.max(0, Math.min(100, (value / total) * 100));
}
