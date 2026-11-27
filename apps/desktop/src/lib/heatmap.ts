import type { HeatmapConfig } from './heatmapConfig';
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
  /** Effective token thresholds [level2, level3, level4] used for coloring. */
  thresholds: [number, number, number];
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

function mondayOf(date: Date): Date {
  const monIdx = (date.getDay() + 6) % 7;
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() - monIdx);
}

function addDays(date: Date, days: number): Date {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate() + days);
}

export function buildHeatmapCalendar(
  dailyList: DashboardSnapshot['daily'],
  anchorIso: string,
  config?: Partial<HeatmapConfig>,
): HeatmapCalendar {
  const map = new Map<string, DashboardSnapshot['daily'][number]>();
  for (const item of dailyList) {
    map.set(item.date, item);
  }

  const todayStr = anchorIso || toIsoDate(new Date());
  const todayDate = parseIsoLocal(todayStr);
  const rangeMode = config?.range ?? 'all';

  let rangeEndDateObj = todayDate;
  let rangeEndStr = todayStr;
  let rangeStartDateObj: Date;
  let startMonday: Date;
  let numWeeks: number;

  if (rangeMode === '12w' || rangeMode === '26w' || rangeMode === '52w') {
    numWeeks = rangeMode === '12w' ? 12 : rangeMode === '26w' ? 26 : 52;
    const endMonday = mondayOf(todayDate);
    startMonday = addDays(endMonday, -(numWeeks - 1) * 7);
    rangeStartDateObj = startMonday;
  } else if (rangeMode === 'custom') {
    const rawEnd = config?.customEnd && config.customEnd <= todayStr ? config.customEnd : todayStr;
    const parsedEnd = parseIsoLocal(rawEnd);
    rangeEndDateObj = Number.isNaN(parsedEnd.getTime()) ? todayDate : parsedEnd;
    rangeEndStr = toIsoDate(rangeEndDateObj);

    const endMonday = mondayOf(rangeEndDateObj);
    const defaultStart = addDays(endMonday, -11 * 7);
    const parsedStart = config?.customStart ? parseIsoLocal(config.customStart) : defaultStart;
    rangeStartDateObj =
      Number.isNaN(parsedStart.getTime()) || parsedStart > rangeEndDateObj
        ? defaultStart
        : parsedStart;

    startMonday = mondayOf(rangeStartDateObj);
    const daySpan = Math.max(
      0,
      Math.round((endMonday.getTime() - startMonday.getTime()) / 86_400_000),
    );
    numWeeks = Math.max(1, Math.min(260, Math.floor(daySpan / 7) + 1));
  } else {
    // 'all': derive the calendar width from the retained data.
    const currentWeekMonday = mondayOf(todayDate);
    let earliest: Date | null = null;
    for (const item of dailyList) {
      if (item.date > todayStr) continue;
      const parsed = parseIsoLocal(item.date);
      if (Number.isNaN(parsed.getTime())) continue;
      if (!earliest || parsed < earliest) earliest = parsed;
    }
    const earliestMonday = earliest ? mondayOf(earliest) : addDays(currentWeekMonday, -11 * 7);
    const daySpan = Math.max(
      0,
      Math.round((currentWeekMonday.getTime() - earliestMonday.getTime()) / 86_400_000),
    );
    numWeeks = Math.max(1, Math.floor(daySpan / 7) + 1);
    startMonday = earliestMonday;
    rangeStartDateObj = earliest ?? startMonday;
  }

  const rangeStartStr = toIsoDate(rangeStartDateObj);

  let maxTokens = 0;
  let activeDays = 0;
  let peakDate = '';
  const activeTotals: number[] = [];

  for (let w = 0; w < numWeeks; w++) {
    for (let d = 0; d < 7; d++) {
      const cellDate = addDays(startMonday, w * 7 + d);
      const iso = toIsoDate(cellDate);
      if (iso > rangeEndStr) continue;
      if (rangeMode === 'custom' && iso < rangeStartStr) continue;
      const entry = map.get(iso);
      const total = entry?.total_tokens ?? 0;
      if (total > 0) {
        activeDays += 1;
        activeTotals.push(total);
        if (total > maxTokens) {
          maxTokens = total;
          peakDate = iso;
        }
      }
    }
  }

  // Compute effective thresholds [t2, t3, t4]:
  // - In custom mode: use user's absolute M-unit thresholds.
  // - In auto mode: use P90 of active days (when >= 5 active days) so single giant spike days
  //   do not wash out normal heavy days, with lower cutoffs (12% / 35% / 65%).
  let thresholds: [number, number, number];
  if (config?.thresholdMode === 'custom' && Array.isArray(config.customThresholdsM)) {
    const raw = config.customThresholdsM
      .map((m) => Math.max(1, Math.round((Number(m) || 0) * 1_000_000)))
      .sort((a, b) => a - b);
    thresholds = [raw[0] ?? 5_000_000, raw[1] ?? 20_000_000, raw[2] ?? 60_000_000];
  } else {
    let refScale = maxTokens;
    if (activeTotals.length >= 5) {
      activeTotals.sort((a, b) => a - b);
      const p90Idx = Math.floor((activeTotals.length - 1) * 0.9);
      refScale = Math.max(1, activeTotals[p90Idx] ?? maxTokens);
    }
    thresholds = [
      Math.max(1, Math.round(refScale * 0.12)),
      Math.max(1, Math.round(refScale * 0.35)),
      Math.max(1, Math.round(refScale * 0.65)),
    ];
  }

  const [t2, t3, t4] = thresholds;
  const weeks: HeatmapCell[][] = [];
  const monthHeaderCandidates: { colIndex: number; label: string }[] = [];
  let lastMonth = -1;

  for (let w = 0; w < numWeeks; w++) {
    const col: HeatmapCell[] = [];
    for (let d = 0; d < 7; d++) {
      const cellDate = addDays(startMonday, w * 7 + d);
      const iso = toIsoDate(cellDate);
      const isOutOfCustomRange = rangeMode === 'custom' && iso < rangeStartStr;
      const isFuture = iso > rangeEndStr || isOutOfCustomRange;
      const entry = map.get(iso);
      const total = isFuture ? 0 : (entry?.total_tokens ?? 0);
      const input = isFuture ? 0 : (entry?.input_tokens ?? 0);
      const output = isFuture ? 0 : (entry?.output_tokens ?? 0);
      const cache = Math.max(0, total - input - output);

      let level: 0 | 1 | 2 | 3 | 4 = 0;
      if (total > 0) {
        if (total >= t4) level = 4;
        else if (total >= t3) level = 3;
        else if (total >= t2) level = 2;
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

  const monthHeaders = monthHeaderCandidates
    .filter((header, index) => {
      const next = monthHeaderCandidates[index + 1];
      return !(index === 0 && next && next.colIndex - header.colIndex < 3);
    })
    .map((header) => ({
      ...header,
      align: header.colIndex > numWeeks - 4 ? ('end' as const) : ('start' as const),
    }));

  return {
    weeks,
    monthHeaders,
    activeDays,
    peakDate,
    peakTokens: maxTokens,
    rangeStartDate: rangeStartStr,
    rangeEndDate: rangeEndStr,
    thresholds,
  };
}
