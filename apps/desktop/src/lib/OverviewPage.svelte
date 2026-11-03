<script lang="ts">
import { formatCompactTokens, type TrendViewMode } from '../lib/dashboardView';
  import { nextChartIndex } from '../lib/chartInteraction';
  import { formatCost, formatNumber, percentOf, relativeTime } from '../lib/format';
  import type { ChartPoint } from '../lib/dashboardView';
  import type { HeatmapCalendar, HeatmapCell } from '../lib/heatmap';
  import { icons } from '../lib/icons';
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
  export let chartPath: string;
  export let hoveredCurvePoint: ChartPoint | null;
  export let heatmapCalendar: HeatmapCalendar;
  export let topToolTotal: number;
  export let onSwitchTrendView: (mode: TrendViewMode) => void;
  export let onNavigateTools: () => void;
</script>

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
                <span>总 Token 消耗</span>
                <span class="precision-chip exact">
                  {@html icons.shield} {exactRatio}% 精确
                </span>
              </div>
              <div class="metric-val">
                {formatNumber(local?.total_tokens)}
              </div>
              <div class="metric-foot">
                <span>估算成本：<strong>{formatCost(local?.estimated_cost_microusd)}</strong></span>
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
                {formatNumber(local?.input_tokens)}
              </div>
              <div class="metric-foot">
                <span>不含缓存读取与写入</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>输出 Token</span>
                <span class="panel-tag tabular">推理占 {reasoningShareRate}%</span>
              </div>
              <div class="metric-val">
                {formatNumber(local?.output_tokens)}
              </div>
              <div class="metric-foot">
                <span>含推理 <strong>{formatNumber(local?.reasoning_tokens)}</strong> (输出子集)</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span>缓存读写 Token</span>
                <span class="precision-chip exact">读取占输入 {cacheHitRate}%</span>
              </div>
              <div class="metric-val">
                {formatNumber(cacheTokens)}
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
              隔离保护：未归属用量 {formatNumber(snapshot.unassigned.total_tokens)} Token，避免跨设备重复累加
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
                      ? '近 20 周每日 Token 活跃度热力图（鼠标悬浮方格可查看当日明细）'
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
                      onmouseleave={() => (hoveredCurveIndex = null)}
                    >
                      <svg class="trend-svg" viewBox="0 0 800 176" role="img" aria-label="每日 Token 使用曲线">
                        <defs>
                          <linearGradient id="trendGradient" x1="0" x2="0" y1="0" y2="1">
                            <stop offset="0%" stop-color="var(--brand-primary)" stop-opacity="0.32" />
                            <stop offset="100%" stop-color="var(--brand-primary)" stop-opacity="0.02" />
                          </linearGradient>
                        </defs>

                        <!-- Y-Axis Labels & Horizontal Grid Lines -->
                        <text x="6" y="36" class="chart-grid-label">{formatCompactTokens(chartMax)}</text>
                        <text x="6" y="98" class="chart-grid-label">{formatCompactTokens(Math.round(chartMax / 2))}</text>
                        <text x="6" y="159" class="chart-grid-label">0</text>

                        {#each [32, 73, 114, 156] as y}
                          <line x1="56" x2="784" y1={y} y2={y} stroke="var(--chart-grid)" stroke-dasharray="4 6" />
                        {/each}

                        {#if hoveredCurvePoint}
                          <line
                            x1={hoveredCurvePoint.x}
                            x2={hoveredCurvePoint.x}
                            y1="24"
                            y2="156"
                            stroke="var(--brand-primary)"
                            stroke-width="1.25"
                            stroke-dasharray="3 3"
                          />
                        {/if}

                        {#if chartPoints.length > 1}
                          <path
                            d={`${chartPath} L ${chartPoints[chartPoints.length - 1].x} 156 L ${chartPoints[0].x} 156 Z`}
                            fill="url(#trendGradient)"
                          />
                          <path
                            d={chartPath}
                            fill="none"
                            stroke="var(--brand-primary)"
                            stroke-width="2.25"
                            stroke-linecap="round"
                            stroke-linejoin="round"
                          />
                          {#each chartPoints as point, idx}
                            {#if hoveredCurveIndex === idx}
                              <circle cx={point.x} cy={point.y} r="9" fill="var(--brand-subtle)" />
                            {/if}
                            <circle
                              cx={point.x}
                              cy={point.y}
                              r={hoveredCurveIndex === idx ? 4.5 : 3.5}
                              fill={hoveredCurveIndex === idx ? 'var(--brand-primary)' : 'var(--bg-card)'}
                              stroke="var(--brand-primary)"
                              stroke-width="2"
                            />
                            <rect
                              class="chart-hit-rect"
                              tabindex="0"
                              role="button"
                              aria-label={`${point.date} ${point.weekdayLabel}，总计 ${formatNumber(point.total_tokens)} Token，输入 ${formatNumber(point.input_tokens)}，输出 ${formatNumber(point.output_tokens)}，缓存/其他 ${formatNumber(point.cache_tokens)}`}
                              aria-pressed={hoveredCurveIndex === idx}
                              x={point.x - Math.max(12, 360 / chartPoints.length)}
                              y="16"
                              width={Math.max(24, 720 / chartPoints.length)}
                              height="146"
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
                                    chartPoints.length,
                                  );
                                }
                              }}
                            />
                          {/each}
                        {:else if chartPoints.length === 1}
                          <!-- Single-Day Visual Pulse ("今天" / 1-Day Sample) -->
                          <line
                            x1="56"
                            x2="784"
                            y1={chartPoints[0].y}
                            y2={chartPoints[0].y}
                            stroke="var(--brand-border)"
                            stroke-dasharray="3 4"
                          />
                          <rect
                            x={chartPoints[0].x - 32}
                            y={chartPoints[0].y}
                            width="64"
                            height={Math.max(6, 156 - chartPoints[0].y)}
                            rx="6"
                            fill="url(#trendGradient)"
                            stroke="var(--brand-border)"
                          />
                          <circle
                            cx={chartPoints[0].x}
                            cy={chartPoints[0].y}
                            r="11"
                            fill="var(--brand-subtle)"
                          />
                          <circle
                            cx={chartPoints[0].x}
                            cy={chartPoints[0].y}
                            r="4.5"
                            fill="var(--bg-card)"
                            stroke="var(--brand-primary)"
                            stroke-width="2.5"
                          />
                          <rect
                            class="chart-hit-rect"
                            tabindex="0"
                            role="button"
                            aria-label={`${chartPoints[0].date} ${chartPoints[0].weekdayLabel}，总计 ${formatNumber(chartPoints[0].total_tokens)} Token，输入 ${formatNumber(chartPoints[0].input_tokens)}，输出 ${formatNumber(chartPoints[0].output_tokens)}，缓存/其他 ${formatNumber(chartPoints[0].cache_tokens)}`}
                            aria-pressed={hoveredCurveIndex === 0}
                            x="56"
                            y="16"
                            width="728"
                            height="146"
                            onmouseenter={() => (hoveredCurveIndex = 0)}
                            onfocus={() => (hoveredCurveIndex = 0)}
                            onblur={() => (hoveredCurveIndex = null)}
                          />
                        {/if}
                      </svg>

                      {#if hoveredCurvePoint}
                        <div
                          class="chart-hover-tooltip"
                          style={`left: ${(hoveredCurvePoint.x / 800) * 100}%; top: ${Math.max(10, (hoveredCurvePoint.y / 176) * 100)}%; transform: translate(${hoveredCurvePoint.x > 520 ? '-106%' : hoveredCurvePoint.x < 220 ? '8%' : '-50%'}, ${hoveredCurvePoint.y < 68 ? '14px' : '-108%'});`}
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
                    onmouseleave={() => (hoveredHeatmapCell = null)}
                  >
                    <div class="heatmap-scroll">
                      <div class="heatmap-inner">
                        <div
                          class="heatmap-months"
                          style={`grid-template-columns: repeat(${heatmapCalendar.weeks.length}, minmax(0, 1fr));`}
                        >
                          {#each heatmapCalendar.weeks as _, colIdx}
                            {@const header = heatmapCalendar.monthHeaders.find((m) => m.colIndex === colIdx)}
                            <span class="heatmap-month-label">{header ? header.label : ''}</span>
                          {/each}
                        </div>

                        <div class="heatmap-body">
                          <div class="heatmap-weekdays" aria-hidden="true">
                            <span>一</span>
                            <span></span>
                            <span>三</span>
                            <span></span>
                            <span>五</span>
                            <span></span>
                            <span>日</span>
                          </div>

                          <div
                            class="heatmap-grid"
                            style={`grid-template-columns: repeat(${heatmapCalendar.weeks.length}, minmax(0, 1fr));`}
                          >
                            {#each heatmapCalendar.weeks as weekCol}
                              <div class="heatmap-week-col">
                                {#each weekCol as cell}
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
                                {/each}
                              </div>
                            {/each}
                          </div>
                        </div>
                      </div>
                    </div>

                    {#if hoveredHeatmapCell}
                      <div
                        class="chart-hover-tooltip"
                        style={`left: ${((hoveredHeatmapCell.colIndex + 0.5) / heatmapCalendar.weeks.length) * 100}%; top: ${((hoveredHeatmapCell.rowIndex + 1) / 7) * 100}%; transform: translate(${hoveredHeatmapCell.colIndex > heatmapCalendar.weeks.length - 6 ? '-105%' : hoveredHeatmapCell.colIndex < 4 ? '6%' : '-50%'}, -115%);`}
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
                        近 20 周暂无消耗峰值
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
