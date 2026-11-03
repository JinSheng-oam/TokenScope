import type { DashboardSnapshot } from './types';

export type HeatmapCell = {
  date: string;
  weekdayLabel: string;
  total_tokens: number;
  input_tokens: number;
  output_tokens: number;
  cache_tokens: number;
  level: 0 | 1 | 2 | 3 | 4;
  isToday: boolean;
  isFuture: boolean;
  colIndex: number;
  rowIndex: number;
};

export type HeatmapCalendar = {
  weeks: HeatmapCell[][];
  monthHeaders: { colIndex: number; label: string }[];
  activeDays: number;
  peakDate: string;
  peakTokens: number;
};

const weekdayNames = ['周一', '周二', '周三', '周四', '周五', '周六', '周日'];

export function parseIsoLocal(iso: string): Date {
  const [y, m, d] = iso.split('-').map((n) => Number(n));
  if (!y || !m || !d) return new Date();
  return new Date(y, m - 1, d);
}

export function toIsoDate(date: Date): string {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, '0');
  const day = String(date.getDate()).padStart(2, '0');
  return `${year}-${month}-${day}`;
}

export function formatWeekday(iso: string): string {
  const dt = parseIsoLocal(iso);
  const monIdx = (dt.getDay() + 6) % 7;
  return weekdayNames[monIdx] ?? '';
}

export function buildHeatmapCalendar(
  dailyList: DashboardSnapshot['daily'],
  anchorIso: string,
  numWeeks = 20
): HeatmapCalendar {
  const map = new Map<string, DashboardSnapshot['daily'][number]>();
  for (const item of dailyList) {
    map.set(item.date, item);
  }

  const todayStr = anchorIso || toIsoDate(new Date());
  const todayDate = parseIsoLocal(todayStr);
  const todayMonIdx = (todayDate.getDay() + 6) % 7;
  const currentWeekMonday = new Date(
    todayDate.getFullYear(),
    todayDate.getMonth(),
    todayDate.getDate() - todayMonIdx
  );
  const startMonday = new Date(
    currentWeekMonday.getFullYear(),
    currentWeekMonday.getMonth(),
    currentWeekMonday.getDate() - (numWeeks - 1) * 7
  );

  let maxTokens = 0;
  let activeDays = 0;
  let peakDate = '';
  for (let w = 0; w < numWeeks; w++) {
    for (let d = 0; d < 7; d++) {
      const cellDate = new Date(
        startMonday.getFullYear(),
        startMonday.getMonth(),
        startMonday.getDate() + w * 7 + d
      );
      const iso = toIsoDate(cellDate);
      if (iso > todayStr) continue;
      const entry = map.get(iso);
      const total = entry?.total_tokens ?? 0;
      if (total > 0) {
        activeDays += 1;
        if (total > maxTokens) {
          maxTokens = total;
          peakDate = iso;
        }
      }
    }
  }

  const weeks: HeatmapCell[][] = [];
  const monthHeaders: { colIndex: number; label: string }[] = [];
  let lastMonth = -1;

  for (let w = 0; w < numWeeks; w++) {
    const col: HeatmapCell[] = [];
    for (let d = 0; d < 7; d++) {
      const cellDate = new Date(
        startMonday.getFullYear(),
        startMonday.getMonth(),
        startMonday.getDate() + w * 7 + d
      );
      const iso = toIsoDate(cellDate);
      const isFuture = iso > todayStr;
      const entry = map.get(iso);
      const total = isFuture ? 0 : (entry?.total_tokens ?? 0);
      const input = isFuture ? 0 : (entry?.input_tokens ?? 0);
      const output = isFuture ? 0 : (entry?.output_tokens ?? 0);
      const cache = Math.max(0, total - input - output);

      let level: 0 | 1 | 2 | 3 | 4 = 0;
      if (total > 0 && maxTokens > 0) {
        const ratio = total / maxTokens;
        if (ratio >= 0.75) level = 4;
        else if (ratio >= 0.5) level = 3;
        else if (ratio >= 0.22) level = 2;
        else level = 1;
      }

      if (d === 0) {
        const month = cellDate.getMonth() + 1;
        if (month !== lastMonth) {
          monthHeaders.push({ colIndex: w, label: `${month}月` });
          lastMonth = month;
        }
      }

      col.push({
        date: iso,
        weekdayLabel: weekdayNames[d] ?? '',
        total_tokens: total,
        input_tokens: input,
        output_tokens: output,
        cache_tokens: cache,
        level,
        isToday: iso === todayStr,
        isFuture,
        colIndex: w,
        rowIndex: d,
      });
    }
    weeks.push(col);
  }

  return {
    weeks,
    monthHeaders,
    activeDays,
    peakDate,
    peakTokens: maxTokens,
  };
}
