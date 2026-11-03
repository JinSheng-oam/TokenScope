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
  monthHeaders: { colIndex: number; label: string; align: 'start' | 'end' }[];
  activeDays: number;
  peakDate: string;
  peakTokens: number;
  rangeStartDate: string;
  rangeEndDate: string;
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
  anchorIso: string
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

  // The overview is an all-history surface. Derive the calendar width from
  // the retained data instead of silently hiding older months behind a fixed
  // 20-week window.
  let earliest: Date | null = null;
  for (const item of dailyList) {
    if (item.date > todayStr) continue;
    const parsed = parseIsoLocal(item.date);
    if (Number.isNaN(parsed.getTime())) continue;
    if (!earliest || parsed < earliest) earliest = parsed;
  }
  const earliestMonIdx = earliest ? (earliest.getDay() + 6) % 7 : 0;
  const earliestMonday = earliest
    ? new Date(earliest.getFullYear(), earliest.getMonth(), earliest.getDate() - earliestMonIdx)
    : new Date(
        currentWeekMonday.getFullYear(),
        currentWeekMonday.getMonth(),
        currentWeekMonday.getDate() - 11 * 7
      );
  const daySpan = Math.max(
    0,
    Math.round((currentWeekMonday.getTime() - earliestMonday.getTime()) / 86_400_000)
  );
  const numWeeks = Math.max(1, Math.floor(daySpan / 7) + 1);
  const startMonday = new Date(
    earliestMonday.getFullYear(),
    earliestMonday.getMonth(),
    earliestMonday.getDate()
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
  const monthHeaderCandidates: { colIndex: number; label: string }[] = [];
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
          monthHeaderCandidates.push({ colIndex: w, label: `${month}月` });
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

  // Suppress a partial first month when its successor starts one or two
  // columns later; the exact retained range is already printed above the
  // calendar. The final label aligns to the left edge so it cannot be clipped.
  const monthHeaders = monthHeaderCandidates
    .filter((header, index) => {
      const next = monthHeaderCandidates[index + 1];
      return !(index === 0 && next && next.colIndex - header.colIndex < 3);
    })
    .map((header) => ({
      ...header,
      align: header.colIndex > numWeeks - 4 ? 'end' as const : 'start' as const,
    }));

  return {
    weeks,
    monthHeaders,
    activeDays,
    peakDate,
    peakTokens: maxTokens,
    rangeStartDate: earliest ? toIsoDate(earliest) : toIsoDate(startMonday),
    rangeEndDate: todayStr,
  };
}
