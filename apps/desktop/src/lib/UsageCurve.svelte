<script lang="ts">
  import { nextChartIndex } from "./chartInteraction";
  import { formatCompactTokens, type ChartPoint } from "./dashboardView";
  import { formatNumber, percentOf } from "./format";
  import type { DashboardSnapshot } from "./types";

  export let inputShareRate: string;
  export let outputShareRate: string;
  export let cacheShareRate: string;
  export let hoveredCurveIndex: number | null;
  export let chartValues: DashboardSnapshot['daily'];
  export let chartMax: number;
  export let chartPoints: ChartPoint[];
  export let hoveredCurvePoint: ChartPoint | null;

  // 趋势图按舞台实际像素尺寸绘制，避免 viewBox 等比缩放导致曲线压扁、文字过小。
  // dashboardView 输出的坐标基于 800×176 归一化空间（x: 64→772，y: 156→32）。
  const CHART_PAD = { left: 52, right: 14, top: 12, bottom: 10 };
  const CURVE_DOT_LIMIT = 60;
  let stageWidth = 0;
  let stageHeight = 0;

  $: plotLeft = CHART_PAD.left;
  $: plotRight = Math.max(plotLeft + 1, stageWidth - CHART_PAD.right);
  $: plotTop = CHART_PAD.top;
  $: plotBottom = Math.max(plotTop + 1, stageHeight - CHART_PAD.bottom);
  $: plotWidth = plotRight - plotLeft;
  $: plotHeight = plotBottom - plotTop;
  $: plotted = chartPoints.map((point) => ({
    ...point,
    px: plotLeft + ((point.x - 64) / 708) * plotWidth,
    py: plotBottom - ((156 - point.y) / 124) * plotHeight,
  }));
  $: plottedPath = plotted
    .map((point, index) => `${index === 0 ? 'M' : 'L'} ${point.px.toFixed(1)} ${point.py.toFixed(1)}`)
    .join(' ');
  $: hoveredPlotted = hoveredCurveIndex !== null ? plotted[hoveredCurveIndex] ?? null : null;
  $: showCurveDots = plotted.length <= CURVE_DOT_LIMIT;
  $: hitWidth = Math.max(4, plotWidth / Math.max(1, plotted.length));
  $: gridFractions = [1, 0.75, 0.5, 0.25, 0];

  function gridY(fraction: number) {
    return plotBottom - fraction * plotHeight;
  }

  function handleTrendPointerMove(event: PointerEvent) {
    if (plotted.length === 0) return;
    const bounds = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const localX = event.clientX - bounds.left;
    if (localX < plotLeft - 8 || localX > plotRight + 8) {
      hoveredCurveIndex = null;
      return;
    }
    const ratio = (localX - plotLeft) / plotWidth;
    hoveredCurveIndex = Math.max(
      0,
      Math.min(plotted.length - 1, Math.round(ratio * (plotted.length - 1))),
    );
  }
</script>

{#if chartValues.length > 0}
  <div class="chart-wrap">
    <div
      class="chart-stage"
      role="region"
      aria-label="Token 趋势交互区；按方向键、Home 或 End 浏览每日数据"
      bind:clientWidth={stageWidth}
      bind:clientHeight={stageHeight}
      onmouseleave={() => (hoveredCurveIndex = null)}
      onpointermove={handleTrendPointerMove}
    >
      {#if stageWidth > 0 && stageHeight > 0}
        <svg
          class="trend-svg"
          viewBox={`0 0 ${stageWidth} ${stageHeight}`}
          width={stageWidth}
          height={stageHeight}
          role="img"
          aria-label="每日 Token 使用曲线"
        >
          <defs>
            <linearGradient id="trendGradient" x1="0" x2="0" y1="0" y2="1">
              <stop offset="0%" stop-color="var(--brand-primary)" stop-opacity="0.28" />
              <stop offset="100%" stop-color="var(--brand-primary)" stop-opacity="0.02" />
            </linearGradient>
          </defs>

          <!-- Y-Axis Labels & Horizontal Grid Lines -->
          {#each gridFractions as fraction}
            <line
              x1={plotLeft}
              x2={plotRight}
              y1={gridY(fraction)}
              y2={gridY(fraction)}
              stroke="var(--chart-grid)"
              stroke-dasharray={fraction === 0 ? undefined : '4 6'}
            />
          {/each}
          <text x={plotLeft - 8} y={gridY(1)} class="chart-grid-label" text-anchor="end" dominant-baseline="middle">{formatCompactTokens(chartMax)}</text>
          <text x={plotLeft - 8} y={gridY(0.5)} class="chart-grid-label" text-anchor="end" dominant-baseline="middle">{formatCompactTokens(Math.round(chartMax / 2))}</text>
          <text x={plotLeft - 8} y={gridY(0)} class="chart-grid-label" text-anchor="end" dominant-baseline="middle">0</text>

          {#if hoveredPlotted}
            <line
              x1={hoveredPlotted.px}
              x2={hoveredPlotted.px}
              y1={plotTop}
              y2={plotBottom}
              stroke="var(--brand-primary)"
              stroke-width="1.25"
              stroke-dasharray="3 3"
            />
          {/if}

          {#if plotted.length > 1}
            <path
              d={`${plottedPath} L ${plotted[plotted.length - 1].px} ${plotBottom} L ${plotted[0].px} ${plotBottom} Z`}
              fill="url(#trendGradient)"
            />
            <path
              d={plottedPath}
              fill="none"
              stroke="var(--brand-primary)"
              stroke-width={showCurveDots ? 2.25 : 1.75}
              stroke-linecap="round"
              stroke-linejoin="round"
            />
            {#each plotted as point, idx}
              {#if hoveredCurveIndex === idx}
                <circle cx={point.px} cy={point.py} r="9" fill="var(--brand-subtle)" />
              {/if}
              {#if showCurveDots || hoveredCurveIndex === idx}
                <circle
                  cx={point.px}
                  cy={point.py}
                  r={hoveredCurveIndex === idx ? 4.5 : 3.5}
                  fill={hoveredCurveIndex === idx ? 'var(--brand-primary)' : 'var(--bg-card)'}
                  stroke="var(--brand-primary)"
                  stroke-width="2"
                />
              {/if}
              <rect
                class="chart-hit-rect"
                tabindex="0"
                role="button"
                aria-label={`${point.date} ${point.weekdayLabel}，总计 ${formatNumber(point.total_tokens)} Token，输入 ${formatNumber(point.input_tokens)}，输出 ${formatNumber(point.output_tokens)}，缓存/其他 ${formatNumber(point.cache_tokens)}`}
                aria-pressed={hoveredCurveIndex === idx}
                x={point.px - hitWidth / 2}
                y={plotTop}
                width={hitWidth}
                height={plotHeight}
                onmouseenter={() => (hoveredCurveIndex = idx)}
                onfocus={() => (hoveredCurveIndex = idx)}
                onblur={() => (hoveredCurveIndex = null)}
                onkeydown={(event) => {
                  if (['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) {
                    event.preventDefault();
                    hoveredCurveIndex = nextChartIndex(
                      idx,
                      event.key === 'ArrowLeft'
                        ? 'prev'
                        : event.key === 'ArrowRight'
                          ? 'next'
                          : event.key === 'Home' ? 'home' : 'end',
                      plotted.length,
                    );
                  }
                }}
              />
            {/each}
          {:else if plotted.length === 1}
            <!-- Single-Day Visual Pulse ("今天" / 1-Day Sample) -->
            <line
              x1={plotLeft}
              x2={plotRight}
              y1={plotted[0].py}
              y2={plotted[0].py}
              stroke="var(--brand-border)"
              stroke-dasharray="3 4"
            />
            <rect
              x={plotted[0].px - 32}
              y={plotted[0].py}
              width="64"
              height={Math.max(6, plotBottom - plotted[0].py)}
              rx="6"
              fill="url(#trendGradient)"
              stroke="var(--brand-border)"
            />
            <circle cx={plotted[0].px} cy={plotted[0].py} r="11" fill="var(--brand-subtle)" />
            <circle
              cx={plotted[0].px}
              cy={plotted[0].py}
              r="4.5"
              fill="var(--bg-card)"
              stroke="var(--brand-primary)"
              stroke-width="2.5"
            />
            <rect
              class="chart-hit-rect"
              tabindex="0"
              role="button"
              aria-label={`${plotted[0].date} ${plotted[0].weekdayLabel}，总计 ${formatNumber(plotted[0].total_tokens)} Token，输入 ${formatNumber(plotted[0].input_tokens)}，输出 ${formatNumber(plotted[0].output_tokens)}，缓存/其他 ${formatNumber(plotted[0].cache_tokens)}`}
              aria-pressed={hoveredCurveIndex === 0}
              x={plotLeft}
              y={plotTop}
              width={plotWidth}
              height={plotHeight}
              onmouseenter={() => (hoveredCurveIndex = 0)}
              onfocus={() => (hoveredCurveIndex = 0)}
              onblur={() => (hoveredCurveIndex = null)}
            />
          {/if}
        </svg>
      {/if}

      {#if hoveredPlotted && hoveredCurvePoint}
        <div
          class="chart-hover-tooltip"
          style={`left: ${hoveredPlotted.px}px; top: ${Math.max(8, hoveredPlotted.py)}px; transform: translate(${hoveredPlotted.px > stageWidth * 0.65 ? '-106%' : hoveredPlotted.px < stageWidth * 0.28 ? '8%' : '-50%'}, ${hoveredPlotted.py < stageHeight * 0.45 ? '14px' : '-108%'});`}
        >
          <div class="tooltip-head">
            <span>{hoveredCurvePoint.date}</span>
            <span class="tooltip-weekday">{hoveredCurvePoint.weekdayLabel}</span>
          </div>
          <div class="tooltip-total-row">
            <span>当日总计</span>
            <strong>{formatNumber(hoveredCurvePoint.total_tokens)}</strong>
          </div>
          <div class="tooltip-breakdown">
            <div class="tooltip-row">
              <span class="tooltip-row-label"><span class="integrity-dot" style="background: var(--brand-primary);"></span>输入 Token</span>
              <span class="tooltip-row-val">{formatNumber(hoveredCurvePoint.input_tokens)}</span>
            </div>
            <div class="tooltip-row">
              <span class="tooltip-row-label"><span class="integrity-dot aggregate"></span>输出 Token</span>
              <span class="tooltip-row-val">{formatNumber(hoveredCurvePoint.output_tokens)}</span>
            </div>
            <div class="tooltip-row">
              <span class="tooltip-row-label"><span class="integrity-dot exact"></span>缓存/其他</span>
              <span class="tooltip-row-val">{formatNumber(hoveredCurvePoint.cache_tokens)}</span>
            </div>
          </div>
        </div>
      {/if}
    </div>
    <div class="chart-axis-labels">
      <span>{chartValues[0]?.date ?? ''}</span>
      <span class="chart-legend-inline">
        <span class="chart-legend-item"><span class="integrity-dot" style="background: var(--brand-primary);"></span>输入 {inputShareRate}%</span>
        <span class="chart-legend-item"><span class="integrity-dot aggregate"></span>输出 {outputShareRate}%</span>
        <span class="chart-legend-item"><span class="integrity-dot exact"></span>缓存 {cacheShareRate}%</span>
      </span>
      <span>{chartValues[chartValues.length - 1]?.date ?? ''}</span>
    </div>
  </div>
{:else}
  <div class="empty-box">当前时间范围暂无可绘制的用量记录</div>
{/if}

<style>
  /* 舞台尺寸由布局决定，SVG 绝对定位并按实际像素绘制，避免尺寸反馈循环 */
  :where(.chart-stage) {
    position: relative;
    width: 100%;
    height: 180px;
  }

  .trend-svg {
    position: absolute;
    inset: 0;
    overflow: visible;
    display: block;
  }

  .chart-hit-rect {
    fill: transparent;
    cursor: crosshair;
    outline: none;
  }

  .chart-hit-rect:focus-visible {
    fill: color-mix(in srgb, var(--brand-primary) 8%, transparent);
    stroke: var(--brand-primary);
    stroke-width: 1.5px;
  }

  .chart-grid-label {
    font-size: 11px;
    font-family: var(--font-mono);
    fill: var(--text-muted);
  }

  .chart-legend-inline {
    display: inline-flex;
    align-items: center;
    gap: 12px;
    font-family: var(--font-sans);
    font-size: 11px;
    color: var(--text-secondary);
  }

  .chart-legend-item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }
</style>
