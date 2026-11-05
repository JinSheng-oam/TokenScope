<script lang="ts">
  import { onMount } from 'svelte';

  export let startDate: string = '';
  export let endDate: string = '';
  export let maxDate: string = '';
  export let onApply: (range: { start: string; end: string }) => void;
  export let onClose: () => void;

  let containerRef: HTMLDivElement | null = null;

  // Working dates inside the picker (draft state before apply)
  let draftStart: string = startDate || maxDate || toIsoDate(new Date());
  let draftEnd: string = endDate || maxDate || toIsoDate(new Date());

  // Current calendar view (year and month 0-indexed)
  const initialDate = parseIsoDate(draftEnd || maxDate || toIsoDate(new Date()));
  let viewYear = initialDate.getFullYear();
  let viewMonth = initialDate.getMonth();

  function toIsoDate(d: Date): string {
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    return `${y}-${m}-${day}`;
  }

  function parseIsoDate(str: string): Date {
    const parts = str.split('-').map(Number);
    if (parts.length === 3 && !isNaN(parts[0]) && !isNaN(parts[1]) && !isNaN(parts[2])) {
      return new Date(parts[0], parts[1] - 1, parts[2]);
    }
    return new Date();
  }

  function prevMonth() {
    if (viewMonth === 0) {
      viewYear -= 1;
      viewMonth = 11;
    } else {
      viewMonth -= 1;
    }
  }

  function nextMonth() {
    if (viewMonth === 11) {
      viewYear += 1;
      viewMonth = 0;
    } else {
      viewMonth += 1;
    }
  }

  // Presets definition
  type Preset = { label: string; getRange: () => { start: string; end: string } };
  const presets: Preset[] = [
    {
      label: '今天',
      getRange: () => {
        const today = maxDate || toIsoDate(new Date());
        return { start: today, end: today };
      },
    },
    {
      label: '最近 3 天',
      getRange: () => {
        const end = maxDate || toIsoDate(new Date());
        const d = parseIsoDate(end);
        d.setDate(d.getDate() - 2);
        return { start: toIsoDate(d), end };
      },
    },
    {
      label: '最近 7 天',
      getRange: () => {
        const end = maxDate || toIsoDate(new Date());
        const d = parseIsoDate(end);
        d.setDate(d.getDate() - 6);
        return { start: toIsoDate(d), end };
      },
    },
    {
      label: '最近 14 天',
      getRange: () => {
        const end = maxDate || toIsoDate(new Date());
        const d = parseIsoDate(end);
        d.setDate(d.getDate() - 13);
        return { start: toIsoDate(d), end };
      },
    },
    {
      label: '最近 30 天',
      getRange: () => {
        const end = maxDate || toIsoDate(new Date());
        const d = parseIsoDate(end);
        d.setDate(d.getDate() - 29);
        return { start: toIsoDate(d), end };
      },
    },
    {
      label: '本月至今',
      getRange: () => {
        const end = maxDate || toIsoDate(new Date());
        const d = parseIsoDate(end);
        const start = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-01`;
        return { start, end };
      },
    },
    {
      label: '上个月',
      getRange: () => {
        const end = maxDate || toIsoDate(new Date());
        const d = parseIsoDate(end);
        const prevMonthDate = new Date(d.getFullYear(), d.getMonth() - 1, 1);
        const lastDayOfPrevMonth = new Date(d.getFullYear(), d.getMonth(), 0);
        return {
          start: toIsoDate(prevMonthDate),
          end: toIsoDate(lastDayOfPrevMonth),
        };
      },
    },
  ];

  function applyPreset(p: Preset) {
    const range = p.getRange();
    draftStart = range.start;
    draftEnd = range.end;
    isPickingEnd = false;
    const endDateObj = parseIsoDate(range.end);
    viewYear = endDateObj.getFullYear();
    viewMonth = endDateObj.getMonth();
  }

  function isPresetMatched(p: Preset, s: string, e: string): boolean {
    const r = p.getRange();
    return r.start === s && r.end === e;
  }

  // Day cell calculation
  type DayCell = {
    dateStr: string;
    dayNum: number;
    isCurrentMonth: boolean;
    isToday: boolean;
    isDisabled: boolean;
    isStart: boolean;
    isEnd: boolean;
    isInRange: boolean;
  };

  $: calendarDays = (() => {
    const days: DayCell[] = [];
    const todayStr = toIsoDate(new Date());

    // First day of current view month
    const firstDayOfMonth = new Date(viewYear, viewMonth, 1);
    // 0 = Sunday, 1 = Monday, ... 6 = Saturday.
    // Convert to Monday = 0, Sunday = 6
    let startDayOfWeek = firstDayOfMonth.getDay() - 1;
    if (startDayOfWeek === -1) startDayOfWeek = 6;

    // Days in previous month
    const prevMonthLastDate = new Date(viewYear, viewMonth, 0).getDate();
    for (let i = startDayOfWeek - 1; i >= 0; i--) {
      const d = prevMonthLastDate - i;
      const prevDate = new Date(viewYear, viewMonth - 1, d);
      const str = toIsoDate(prevDate);
      days.push({
        dateStr: str,
        dayNum: d,
        isCurrentMonth: false,
        isToday: str === todayStr,
        isDisabled: maxDate ? str > maxDate : false,
        isStart: str === draftStart,
        isEnd: str === draftEnd,
        isInRange: str > draftStart && str < draftEnd,
      });
    }

    // Days in current month
    const currentMonthLastDate = new Date(viewYear, viewMonth + 1, 0).getDate();
    for (let d = 1; d <= currentMonthLastDate; d++) {
      const curDate = new Date(viewYear, viewMonth, d);
      const str = toIsoDate(curDate);
      days.push({
        dateStr: str,
        dayNum: d,
        isCurrentMonth: true,
        isToday: str === todayStr,
        isDisabled: maxDate ? str > maxDate : false,
        isStart: str === draftStart,
        isEnd: str === draftEnd,
        isInRange: str > draftStart && str < draftEnd,
      });
    }

    // Days in next month to complete the 35 or 42 grid
    const remainingSlots = 42 - days.length >= 7 ? 35 - days.length : 42 - days.length;
    for (let d = 1; d <= remainingSlots; d++) {
      const nextDate = new Date(viewYear, viewMonth + 1, d);
      const str = toIsoDate(nextDate);
      days.push({
        dateStr: str,
        dayNum: d,
        isCurrentMonth: false,
        isToday: str === todayStr,
        isDisabled: maxDate ? str > maxDate : false,
        isStart: str === draftStart,
        isEnd: str === draftEnd,
        isInRange: str > draftStart && str < draftEnd,
      });
    }

    return days;
  })();

  let isPickingEnd = false;

  function handleDateClick(day: DayCell) {
    if (day.isDisabled) return;

    if (!isPickingEnd) {
      draftStart = day.dateStr;
      draftEnd = day.dateStr;
      isPickingEnd = true;
    } else {
      if (day.dateStr < draftStart) {
        draftStart = day.dateStr;
        draftEnd = day.dateStr;
      } else {
        draftEnd = day.dateStr;
        isPickingEnd = false;
      }
    }
  }

  function handleConfirm() {
    if (!draftStart || !draftEnd) return;
    const finalStart = draftStart <= draftEnd ? draftStart : draftEnd;
    const finalEnd = draftStart <= draftEnd ? draftEnd : draftStart;
    onApply({ start: finalStart, end: finalEnd });
  }

  function handleClickOutside(event: MouseEvent) {
    if (containerRef && !containerRef.contains(event.target as Node)) {
      onClose();
    }
  }

  onMount(() => {
    document.addEventListener('pointerdown', handleClickOutside);
    return () => {
      document.removeEventListener('pointerdown', handleClickOutside);
    };
  });
</script>

<div class="date-picker-popover" bind:this={containerRef} role="dialog" aria-modal="true" aria-label="选择自定义日期范围">
  <div class="picker-inner">
    <!-- Presets Sidebar -->
    <div class="picker-presets">
      <span class="presets-title">快捷预设</span>
      <div class="presets-list">
        {#each presets as preset}
          <button
            type="button"
            class="preset-btn"
            class:active={isPresetMatched(preset, draftStart, draftEnd)}
            onclick={() => applyPreset(preset)}
          >
            {preset.label}
          </button>
        {/each}
      </div>
    </div>

    <!-- Calendar Panel -->
    <div class="picker-calendar">
      <!-- Month Navigation Header -->
      <div class="calendar-header">
        <button type="button" class="nav-btn" onclick={prevMonth} aria-label="上一月">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="15 18 9 12 15 6" />
          </svg>
        </button>
        <span class="month-title">
          {viewYear}年 {viewMonth + 1}月
        </span>
        <button type="button" class="nav-btn" onclick={nextMonth} aria-label="下一月">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="9 18 15 12 9 6" />
          </svg>
        </button>
      </div>

      <!-- Weekdays -->
      <div class="weekdays-row">
        <span>一</span><span>二</span><span>三</span><span>四</span><span>五</span><span>六</span><span>日</span>
      </div>

      <!-- Grid -->
      <div class="days-grid">
        {#each calendarDays as day}
          <button
            type="button"
            class="day-cell"
            class:not-current={!day.isCurrentMonth}
            class:is-today={day.isToday}
            class:is-disabled={day.isDisabled}
            class:is-start={day.isStart}
            class:is-end={day.isEnd}
            class:in-range={day.isInRange}
            disabled={day.isDisabled}
            onclick={() => handleDateClick(day)}
          >
            <span class="day-num">{day.dayNum}</span>
          </button>
        {/each}
      </div>

      <!-- Inputs & Summary -->
      <div class="picker-footer">
        <div class="range-display">
          <input
            type="text"
            class="date-input"
            bind:value={draftStart}
            placeholder="YYYY-MM-DD"
            maxlength="10"
            aria-label="开始日期"
          />
          <span class="range-arrow">至</span>
          <input
            type="text"
            class="date-input"
            bind:value={draftEnd}
            placeholder="YYYY-MM-DD"
            maxlength="10"
            aria-label="结束日期"
          />
        </div>

        <div class="footer-actions">
          <button type="button" class="btn-ghost" onclick={onClose}>取消</button>
          <button type="button" class="btn-apply" onclick={handleConfirm}>应用范围</button>
        </div>
      </div>
    </div>
  </div>
</div>

<style>
  .date-picker-popover {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 1000;
    background-color: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-lg, 0 14px 34px -4px rgba(0, 0, 0, 0.2), 0 8px 14px -6px rgba(0, 0, 0, 0.12));
    animation: pickerSlide 0.16s cubic-bezier(0.16, 1, 0.3, 1);
    user-select: none;
    overflow: hidden;
  }

  @keyframes pickerSlide {
    from {
      opacity: 0;
      transform: translateY(-6px) scale(0.97);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  .picker-inner {
    display: flex;
    width: 448px;
  }

  /* Presets Sidebar */
  .picker-presets {
    width: 114px;
    flex-shrink: 0;
    padding: 12px 8px;
    background-color: var(--bg-secondary);
    border-right: 1px solid var(--border-subtle);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .presets-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    padding: 0 6px;
    letter-spacing: 0.02em;
  }

  .presets-list {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .preset-btn {
    width: 100%;
    padding: 6px 8px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    font: inherit;
    font-size: 11.5px;
    color: var(--text-secondary);
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .preset-btn:hover {
    background-color: var(--bg-card);
    color: var(--text-primary);
  }

  .preset-btn.active {
    background-color: var(--brand-subtle);
    border-color: var(--brand-border);
    color: var(--brand-primary);
    font-weight: 600;
  }

  /* Calendar Panel */
  .picker-calendar {
    flex: 1;
    min-width: 0;
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .calendar-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 4px;
  }

  .month-title {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .nav-btn {
    width: 26px;
    height: 26px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .nav-btn:hover {
    border-color: var(--border-strong);
    background-color: var(--bg-secondary);
    color: var(--text-primary);
  }

  .nav-btn svg {
    width: 14px;
    height: 14px;
  }

  .weekdays-row {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    text-align: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--text-muted);
    padding: 4px 0;
  }

  .days-grid {
    display: grid;
    grid-template-columns: repeat(7, 1fr);
    row-gap: 3px;
  }

  .day-cell {
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    background: transparent;
    border: none;
    cursor: pointer;
    font: inherit;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--text-primary);
    position: relative;
    padding: 0;
    transition: background-color var(--transition-fast);
  }

  .day-num {
    width: 30px;
    height: 30px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border-radius: 50%;
    transition: all var(--transition-fast);
  }

  .day-cell:hover:not(.is-disabled):not(.is-start):not(.is-end) .day-num {
    background-color: var(--bg-secondary);
    color: var(--brand-primary);
    font-weight: 600;
  }

  .day-cell.not-current .day-num {
    color: var(--text-muted);
    opacity: 0.45;
  }

  .day-cell.is-today:not(.is-start):not(.is-end) .day-num {
    border: 1px solid var(--brand-primary);
    color: var(--brand-primary);
    font-weight: 600;
  }

  .day-cell.is-disabled .day-num {
    opacity: 0.25;
  }

  .day-cell.is-disabled {
    cursor: not-allowed;
  }

  /* Range continuous highlight */
  .day-cell.in-range {
    background-color: var(--brand-subtle);
  }

  .day-cell.in-range:nth-child(7n + 1) {
    border-top-left-radius: 999px;
    border-bottom-left-radius: 999px;
  }

  .day-cell.in-range:nth-child(7n) {
    border-top-right-radius: 999px;
    border-bottom-right-radius: 999px;
  }

  .day-cell.is-start {
    background: linear-gradient(to right, transparent 50%, var(--brand-subtle) 50%);
  }

  .day-cell.is-start:nth-child(7n) {
    background: transparent;
  }

  .day-cell.is-end {
    background: linear-gradient(to left, transparent 50%, var(--brand-subtle) 50%);
  }

  .day-cell.is-end:nth-child(7n + 1) {
    background: transparent;
  }

  .day-cell.is-start.is-end {
    background: transparent;
  }

  .day-cell.is-start .day-num,
  .day-cell.is-end .day-num {
    background-color: var(--brand-strong);
    color: #fff;
    font-weight: 600;
    opacity: 1;
    box-shadow: 0 2px 5px rgba(0, 0, 0, 0.18);
  }

  /* Footer Actions */
  .picker-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 10px;
    margin-top: 2px;
    border-top: 1px solid var(--border-subtle);
  }

  .range-display {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .date-input {
    width: 82px;
    height: 28px;
    padding: 0 4px;
    font: inherit;
    font-family: var(--font-mono);
    font-size: 11px;
    text-align: center;
    background-color: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-xs);
    color: var(--text-primary);
  }

  .date-input:focus {
    outline: none;
    border-color: var(--brand-primary);
  }

  .range-arrow {
    font-size: 11px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .footer-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .btn-ghost {
    height: 28px;
    padding: 0 9px;
    background: transparent;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    white-space: nowrap;
    flex-shrink: 0;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-ghost:hover {
    color: var(--text-primary);
    background-color: var(--bg-secondary);
    border-color: var(--border-strong);
  }

  .btn-apply {
    height: 28px;
    padding: 0 11px;
    background-color: var(--brand-strong);
    border: none;
    border-radius: var(--radius-xs);
    color: #fff;
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .btn-apply:hover {
    background-color: var(--brand-strong-hover);
  }
</style>
