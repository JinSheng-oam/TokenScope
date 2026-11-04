<script lang="ts">
import { formatCompactTokens, type TrendViewMode } from '../lib/dashboardView';
  import { nextChartIndex } from '../lib/chartInteraction';
  import { formatCost, formatNumber, formatToolLabel, percentOf, relativeTime } from '../lib/format';
  import type { ChartPoint } from '../lib/dashboardView';
  import type { HeatmapCalendar, HeatmapCell } from '../lib/heatmap';
  import { icons } from '../lib/icons';
  import RollingNumber from './RollingNumber.svelte';
  import type { DashboardSnapshot, UsageTotals } from '../lib/types';

  export let snapshot: DashboardSnapshot;
  export let local: UsageTotals | undefined;
  export let exactRatio: string;
  export let cacheHitRate: string;
  export let inputShareRate: string;
  export let outputShareRate: string;
  export let cacheShareRate: string;
  export let reasoningShareRate: string;
  export let cacheTokens: number;
  export let trendViewMode: TrendViewMode;
  export let hoveredCurveIndex: number | null;
  export let hoveredHeatmapCell: HeatmapCell | null;
  export let chartValues: DashboardSnapshot['daily'];
  export let chartMax: number;
  export let chartPoints: ChartPoint[];
  export let hoveredCurvePoint: ChartPoint | null;
  export let heatmapCalendar: HeatmapCalendar;
  export let topToolTotal: number;
  export let onSwitchTrendView: (mode: TrendViewMode) => void;
  export let onNavigateTools: () => void;
  export let onNavigateSessions: () => void;
  export let rollingNumbersEnabled = true;

  $: recentSessions = snapshot.session_usage.sessions.slice(0, 4);
  $: untimedRecordCount = snapshot.by_tool.reduce((sum, tool) => sum + tool.untimed_records, 0);
  $: heatmapRangeLabel = `${heatmapCalendar.rangeStartDate} 至 ${heatmapCalendar.rangeEndDate}`;
  const heatmapWeekdayLabels = ['一', '', '三', '', '五', '', '日'];

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

<div class="overview-layout">
<div class="page-header">
            <h1>使用总览</h1>
            <p>
              统计时区 {snapshot.time_zone} · 生成于 {relativeTime(snapshot.generated_at_ms)} · 仅汇集物理单机有效事实
            </p>
          </div>

          <!-- 4 Metric Cards -->
          <div class="metric-grid">
            <article class="metric-card hero-card">
              <div class="metric-head">
                <span class="metric-label-stack">
                  <span>总 Token</span>
                  <small>累计用量 · 含缓存读写</small>
                </span>
                <span class="precision-chip exact" title={`${exactRatio}% 的用量记录为精确精度`}>
                  {@html icons.shield} {exactRatio}%
                </span>
              </div>
              <div class="metric-val">
                <RollingNumber value={local?.total_tokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-foot">
                <span>非缓存 <strong>{formatNumber((local?.total_tokens ?? 0) - cacheTokens)}</strong></span>
                {#if local?.estimated_cost_microusd != null}
                  <span>成本 <strong>{formatCost(local.estimated_cost_microusd)}</strong></span>
                {:else}
                  <span title="暂无计价记录">未计价</span>
                {/if}
              </div>
              <div class="metric-split-bar" aria-label="Token 构成比例">
                <div class="metric-split-seg input" style={`width: ${inputShareRate}%`} title={`输入 ${inputShareRate}%`}></div>
                <div class="metric-split-seg output" style={`width: ${outputShareRate}%`} title={`输出 ${outputShareRate}%`}></div>
                <div class="metric-split-seg cache" style={`width: ${cacheShareRate}%`} title={`缓存 ${cacheShareRate}%`}></div>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>输入 Token</span>
                <span class="panel-tag tabular">占总量 {inputShareRate}%</span>
              </div>
              <div class="metric-val">
                <RollingNumber value={local?.input_tokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-foot">
                <span>不含缓存读取与写入</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>输出 Token</span>
                <span class="panel-tag tabular">推理 {reasoningShareRate}%</span>
              </div>
              <div class="metric-val">
                <RollingNumber value={local?.output_tokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-foot">
                <span>含推理 <strong>{formatNumber(local?.reasoning_tokens)}</strong> (输出子集)</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>缓存 Token</span>
                <span class="precision-chip exact" title={`输入缓存命中率 ${cacheHitRate}%`}>缓存率 {cacheHitRate}%</span>
              </div>
              <div class="metric-val">
                <RollingNumber value={cacheTokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-foot">
                <span>读 <strong>{formatNumber(local?.cache_read_tokens)}</strong> · 写 <strong>{formatNumber(local?.cache_write_tokens)}</strong></span>
              </div>
            </article>
          </div>

          <!-- Data Precision Integrity Guardrail -->
          <div class="integrity-banner">
            <div class="integrity-left">
              <span class="integrity-item"><span class="integrity-dot exact"></span>精确记录：<strong>{formatNumber(local?.exact_records)}</strong> 笔</span>
              <span class="integrity-item"><span class="integrity-dot aggregate"></span>聚合粗粒度：<strong>{formatNumber(local?.aggregate_records)}</strong> 笔</span>
              <span class="integrity-item"><span class="integrity-dot estimated"></span>显式估算：<strong>{formatNumber(local?.estimated_records)}</strong> 笔</span>
              <span class="integrity-item"><span class="integrity-dot muted"></span>精度未知：<strong>{formatNumber(local?.unknown_precision_records)}</strong> 笔</span>
              <span class="integrity-item">已计价：<strong>{formatNumber(local?.priced_records)}</strong> 笔</span>
            </div>
            <div class="integrity-right">
              <span class:warning={untimedRecordCount > 0} title="已计入累计但不进入日趋势">
                时间未知：{formatNumber(untimedRecordCount)} 条（不入日趋势）
              </span>
              <span>隔离保护：未归属 {formatNumber(snapshot.unassigned.total_tokens)} Token</span>
            </div>
          </div>

          <!-- Trend Chart & Tool Rankings (Two Column Grid) -->
          <div class="two-col-grid">
            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>Token 使用趋势</h2>
                  <p>
                    {trendViewMode === 'heatmap'
                      ? `完整历史 · ${heatmapCalendar.weeks.length} 周 · ${heatmapRangeLabel}`
                      : '按本机时区自然日汇集（悬浮或用方向键查看当日明细）'}
                  </p>
                </div>
                <div class="panel-head-actions">
                  <span class="panel-tag">
                    {trendViewMode === 'heatmap'
                      ? `活跃 ${heatmapCalendar.activeDays} 天`
                      : `${snapshot.daily.length} 天样本`}
                  </span>
                  <div class="trend-mode-switch" role="group" aria-label="趋势图显示模式">
                    <button
                      class="trend-mode-btn"
                      class:selected={trendViewMode === 'curve'}
                      aria-pressed={trendViewMode === 'curve'}
                      onclick={() => onSwitchTrendView('curve')}
                    >
                      曲线图
                    </button>
                    <button
                      class="trend-mode-btn"
                      class:selected={trendViewMode === 'heatmap'}
                      aria-pressed={trendViewMode === 'heatmap'}
                      onclick={() => onSwitchTrendView('heatmap')}
                    >
                      热力图
                    </button>
                  </div>
                </div>
              </div>

              {#if trendViewMode === 'curve'}
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
              {:else}
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
              {/if}
            </section>

            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>工具用量分布</h2>
                  <p>按工具归属汇总本机真实用量</p>
                </div>
                <button class="panel-tag" onclick={() => (onNavigateTools())}>查看明细 →</button>
              </div>

              {#if snapshot.by_tool.length > 0}
                <div class="rank-list">
                  {#each snapshot.by_tool.slice(0, 5) as tool, idx}
                    <div class="rank-item">
                      <div class="rank-row">
                        <span class="rank-title">
                          <span class="rank-num">0{idx + 1}</span>
                          {tool.label}
                        </span>
                        <span class="rank-meta">
                          <span class="rank-value tabular">{formatNumber(tool.total_tokens)}</span>
                          <span class="rank-share tabular">{percentOf(tool.total_tokens, local?.total_tokens ?? topToolTotal).toFixed(1)}%</span>
                        </span>
                      </div>
                      <div class="rank-track">
                        <div class="rank-fill" style={`width: ${percentOf(tool.total_tokens, topToolTotal)}%`}></div>
                      </div>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">暂无工具消耗记录</div>
              {/if}
            </section>
          </div>

          <div class="overview-support-grid">
            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>最近会话</h2>
                  <p>按最近事件排序；显示整段会话的累计 Token 用量</p>
                </div>
                <button class="panel-tag" onclick={onNavigateSessions}>查看全部 →</button>
              </div>

              {#if recentSessions.length > 0}
                <div class="recent-session-list">
                  {#each recentSessions as session, index}
                    <div class="recent-session-row">
                      <span class="recent-session-rank tabular">{String(index + 1).padStart(2, '0')}</span>
                      <span class="recent-session-main">
                        <strong title={session.display_name ?? session.session_id}>
                          {session.display_name ?? '未命名会话'}
                        </strong>
                        <small>
                          {formatToolLabel(session.tool_id)}
                          {#if session.models[0]?.model_id} · {session.models[0].model_id}{/if}
                          · {relativeTime(session.last_event_at_ms ?? session.activity_observed_at_ms)}
                        </small>
                      </span>
                      <span class="recent-session-value tabular">{formatNumber(session.total_tokens)}</span>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">当前范围暂无会话记录</div>
              {/if}
            </section>

            <section class="panel-card">
              <div class="panel-head">
                <div>
                  <h2>采集源状态</h2>
                  <p>最近一次检查点与来源覆盖情况</p>
                </div>
                <button class="panel-tag" onclick={onNavigateTools}>运行明细 →</button>
              </div>

              {#if snapshot.source_health.length > 0}
                <div class="source-health-list">
                  {#each snapshot.source_health as source}
                    <div class="source-health-row">
                      <span
                        class="source-status-dot"
                        class:complete={source.status === 'complete'}
                        class:partial={source.status === 'partial'}
                        class:degraded={source.status === 'degraded'}
                      ></span>
                      <span class="source-health-name">{formatToolLabel(source.tool_id)}</span>
                      <span class="source-health-meta">
                        {relativeTime(source.last_scan_ms)} · {source.source_count} 个来源
                      </span>
                      <span
                        class="source-health-status"
                        class:complete={source.status === 'complete'}
                        class:partial={source.status === 'partial'}
                        class:degraded={source.status === 'degraded'}
                      >
                        {source.status === 'complete' ? '正常' : source.status === 'partial' ? '部分' : source.status === 'degraded' ? '异常' : '未知'}
                      </span>
                    </div>
                  {/each}
                </div>
              {:else}
                <div class="empty-box">暂无采集来源检查点</div>
              {/if}
            </section>
          </div>
</div>
