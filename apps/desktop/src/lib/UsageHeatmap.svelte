<script lang="ts">
  import { formatCompactTokens } from "./dashboardView";
  import { formatNumber } from "./format";
  import type { HeatmapCalendar, HeatmapCell } from "./heatmap";

  export let heatmapCalendar: HeatmapCalendar;
  export let hoveredHeatmapCell: HeatmapCell | null;

  const heatmapWeekdayLabels = ['一', '', '三', '', '五', '', '日'];

  // 热力图格子尺寸跟随可用高度，保证 7 行完整落在面板内，并在格子较少时避免被拉得过大。
  let heatmapStageWidth = 0;
  let heatmapStageHeight = 0;
  const HEATMAP_LABEL_COL = 25; // 星期标签列 22px + 3px 间距
  $: heatmapWeekCount = Math.max(1, heatmapCalendar.weeks.length);
  $: heatmapCellSize = Math.max(4, Math.min(15, (heatmapStageHeight - 46) / 7));
  $: heatmapMaxWidth = HEATMAP_LABEL_COL + heatmapWeekCount * (heatmapCellSize + 3);
  $: heatmapInnerWidth = Math.max(0, Math.min(heatmapStageWidth - 4, heatmapMaxWidth));

  function heatmapTooltipLeft(colIndex: number) {
    const offset = (heatmapStageWidth - heatmapInnerWidth) / 2 + HEATMAP_LABEL_COL;
    return offset + ((colIndex + 0.5) * (heatmapInnerWidth - HEATMAP_LABEL_COL)) / heatmapWeekCount;
  }

</script>

<!-- GitHub-Style Contribution Heatmap -->
<div class="chart-wrap">
  <div
    class="heatmap-stage"
    role="region"
    aria-label="GitHub 风格每日 Token 热力图"
    bind:clientWidth={heatmapStageWidth}
    bind:clientHeight={heatmapStageHeight}
    onmouseleave={() => (hoveredHeatmapCell = null)}
  >
    <div class="heatmap-scroll">
      <div class="heatmap-inner" style={`max-width: ${heatmapMaxWidth}px;`}>
        <div
          class="heatmap-months"
          style={`grid-template-columns: repeat(${heatmapCalendar.weeks.length}, minmax(0, 1fr));`}
        >
          {#each heatmapCalendar.weeks as _, colIdx}
            {@const header = heatmapCalendar.monthHeaders.find((m) => m.colIndex === colIdx)}
            <span
              class="heatmap-month-label"
              class:edge-end={header?.align === 'end'}
            >{header ? header.label : ''}</span>
          {/each}
        </div>

        <div
          class="heatmap-grid"
          style={`grid-template-columns: 22px repeat(${heatmapCalendar.weeks.length}, minmax(4px, 1fr));`}
        >
          {#each heatmapWeekdayLabels as weekdayLabel, rowIndex}
            <span class="heatmap-weekday" aria-hidden="true">{weekdayLabel}</span>
            {#each heatmapCalendar.weeks as weekCol}
              {@const cell = weekCol[rowIndex]}
              {#if cell}
                <div
                  class={`heatmap-cell level-${cell.level}`}
                  class:is-today={cell.isToday}
                  class:is-future={cell.isFuture}
                  role="img"
                  aria-label={`${cell.date} ${cell.weekdayLabel}：${formatNumber(cell.total_tokens)} Token`}
                  onmouseenter={() => {
                    if (!cell.isFuture) hoveredHeatmapCell = cell;
                  }}
                ></div>
              {/if}
            {/each}
          {/each}
        </div>
      </div>
    </div>

    {#if hoveredHeatmapCell}
      <div
        class="chart-hover-tooltip"
        style={`left: ${heatmapTooltipLeft(hoveredHeatmapCell.colIndex)}px; top: ${((hoveredHeatmapCell.rowIndex + 1) / 7) * 100}%; transform: translate(${hoveredHeatmapCell.colIndex > heatmapCalendar.weeks.length - 6 ? '-105%' : hoveredHeatmapCell.colIndex < 4 ? '6%' : '-50%'}, ${hoveredHeatmapCell.rowIndex <= 2 ? '14px' : '-115%'});`}
      >
        <div class="tooltip-head">
          <span>{hoveredHeatmapCell.date}{hoveredHeatmapCell.isToday ? ' (今天)' : ''}</span>
          <span class="tooltip-weekday">{hoveredHeatmapCell.weekdayLabel}</span>
        </div>
        <div class="tooltip-total-row">
          <span>当日总计</span>
          <strong>{formatNumber(hoveredHeatmapCell.total_tokens)}</strong>
        </div>
        {#if hoveredHeatmapCell.total_tokens > 0}
          <div class="tooltip-breakdown">
            <div class="tooltip-row">
              <span class="tooltip-row-label"><span class="integrity-dot" style="background: var(--brand-primary);"></span>输入 Token</span>
              <span class="tooltip-row-val">{formatNumber(hoveredHeatmapCell.input_tokens)}</span>
            </div>
            <div class="tooltip-row">
              <span class="tooltip-row-label"><span class="integrity-dot aggregate"></span>输出 Token</span>
              <span class="tooltip-row-val">{formatNumber(hoveredHeatmapCell.output_tokens)}</span>
            </div>
            <div class="tooltip-row">
              <span class="tooltip-row-label"><span class="integrity-dot exact"></span>缓存/其他</span>
              <span class="tooltip-row-val">{formatNumber(hoveredHeatmapCell.cache_tokens)}</span>
            </div>
          </div>
        {:else}
          <div style="font-size: 11px; color: var(--text-muted);">当日无 Token 消耗记录</div>
        {/if}
      </div>
    {/if}
  </div>

  <div class="chart-axis-labels">
    <span>
      {#if heatmapCalendar.peakDate}
        单日峰值：{heatmapCalendar.peakDate} ({formatCompactTokens(heatmapCalendar.peakTokens)})
      {:else}
        当前范围暂无消耗峰值
      {/if}
    </span>
    <span class="heatmap-legend-scale" aria-label="热力图颜色强度图例">
      <span>少</span>
      <span class="heatmap-legend-swatch level-0"></span>
      <span class="heatmap-legend-swatch level-1"></span>
      <span class="heatmap-legend-swatch level-2"></span>
      <span class="heatmap-legend-swatch level-3"></span>
      <span class="heatmap-legend-swatch level-4"></span>
      <span>多</span>
    </span>
  </div>
</div>

<style>
  /* GitHub-Style Contribution Heatmap */
  :where(.heatmap-stage) {
    position: relative;
    width: 100%;
    height: 180px;
  }

  .heatmap-scroll {
    position: absolute;
    inset: 2px 0 4px;
    display: flex;
    align-items: center;
    overflow-x: auto;
    overflow-y: hidden;
    padding: 2px 3px;
  }

  .heatmap-inner {
    width: 100%;
    margin-inline: auto;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .heatmap-months {
    display: grid;
    padding-left: 28px;
    font-size: 11px;
    font-family: var(--font-mono);
    color: var(--text-muted);
    line-height: 1.2;
    min-height: 14px;
  }

  .heatmap-month-label {
    pointer-events: none;
    white-space: nowrap;
    overflow: visible;
  }

  .heatmap-month-label.edge-end {
    align-self: start;
    justify-self: start;
    transform: translateX(-50%);
  }

  .heatmap-grid {
    display: grid;
    width: 100%;
    gap: 3px;
    align-items: center;
  }

  .heatmap-weekday {
    padding-right: 2px;
    color: var(--text-muted);
    font-size: 11px;
    line-height: 1;
    text-align: right;
  }

  .heatmap-cell {
    width: 100%;
    aspect-ratio: 1 / 1;
    min-width: 0;
    min-height: 4px;
    border-radius: 2px;
    background-color: var(--heatmap-0);
    border: 1px solid rgba(128, 128, 128, 0.08);
    cursor: pointer;
    transition: transform 100ms ease, border-color 100ms ease;
  }

  .heatmap-cell.level-0 { background-color: var(--heatmap-0); }
  .heatmap-cell.level-1 { background-color: var(--heatmap-1); }
  .heatmap-cell.level-2 { background-color: var(--heatmap-2); }
  .heatmap-cell.level-3 { background-color: var(--heatmap-3); }
  .heatmap-cell.level-4 { background-color: var(--heatmap-4); }

  .heatmap-cell.is-future {
    opacity: 0.25;
    pointer-events: none;
  }

  .heatmap-cell.is-today {
    outline: 1.5px solid var(--brand-primary);
    outline-offset: 1px;
  }

  .heatmap-cell:hover {
    transform: scale(1.12);
    border-color: var(--text-primary);
    z-index: 2;
  }

  .heatmap-legend-scale {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-family: var(--font-sans);
    font-size: 11px;
    color: var(--text-muted);
  }

  .heatmap-legend-swatch {
    width: 10px;
    height: 10px;
    border-radius: 2px;
    border: 1px solid rgba(128, 128, 128, 0.12);
  }

  .heatmap-legend-swatch.level-0 { background-color: var(--heatmap-0); }
  .heatmap-legend-swatch.level-1 { background-color: var(--heatmap-1); }
  .heatmap-legend-swatch.level-2 { background-color: var(--heatmap-2); }
  .heatmap-legend-swatch.level-3 { background-color: var(--heatmap-3); }
  .heatmap-legend-swatch.level-4 { background-color: var(--heatmap-4); }
</style>
