<script lang="ts">
  import { formatCompactTokens } from '../dashboardView';
  import { formatNumber } from '../format';
  import type { UsageTotals } from '../types';

  let {
    local,
    cacheHitRate = '0',
    inputShareRate = '0',
    outputShareRate = '0',
    cacheShareRate = '0',
    reasoningShareRate = '0',
    cacheTokens = 0,
  }: {
    local?: UsageTotals;
    cacheHitRate?: string;
    inputShareRate?: string;
    outputShareRate?: string;
    cacheShareRate?: string;
    reasoningShareRate?: string;
    cacheTokens?: number;
  } = $props();

  let avgTokensPerRecord = $derived(
    (local?.record_count ?? 0) > 0
      ? Math.round((local?.total_tokens ?? 0) / (local?.record_count ?? 1))
      : 0,
  );
  let outputToInputRatio = $derived(
    (local?.input_tokens ?? 0) > 0
      ? ((local?.output_tokens ?? 0) / (local?.input_tokens ?? 1)).toFixed(2)
      : '0.00',
  );
</script>

<section class="panel-card efficiency-card">
  <div class="panel-head">
    <div>
      <h2>Token 构成与效率</h2>
    </div>
    <span class="precision-chip exact tabular">缓存率 {cacheHitRate}%</span>
  </div>

  <div class="efficiency-bar-wrap">
    <div class="efficiency-stacked-bar" aria-label="Token 构成比例">
      <div class="eff-seg input" style={`width: ${inputShareRate}%`} title={`净输入 ${inputShareRate}%`}></div>
      <div class="eff-seg output" style={`width: ${outputShareRate}%`} title={`输出 ${outputShareRate}%`}></div>
      <div class="eff-seg cache" style={`width: ${cacheShareRate}%`} title={`缓存 ${cacheShareRate}%`}></div>
    </div>
    <div class="efficiency-legend">
      <span><i class="legend-dot input"></i>输入 {inputShareRate}%</span>
      <span><i class="legend-dot output"></i>输出 {outputShareRate}%</span>
      <span title={`缓存共 ${formatNumber(cacheTokens)} Token`}><i class="legend-dot cache"></i>缓存 {cacheShareRate}%</span>
    </div>
  </div>

  <div class="mini-kpi-grid">
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">缓存读 / 写</span>
      <strong class="mini-kpi-val tabular">{formatCompactTokens(local?.cache_read_tokens ?? 0)} / {formatCompactTokens(local?.cache_write_tokens ?? 0)}</strong>
    </div>
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">输出 / 输入比</span>
      <strong class="mini-kpi-val tabular">{outputToInputRatio}x</strong>
    </div>
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">推理占输出</span>
      <strong class="mini-kpi-val tabular">{reasoningShareRate}%</strong>
    </div>
    <div class="mini-kpi-item">
      <span class="mini-kpi-label">单笔均耗</span>
      <strong class="mini-kpi-val tabular">{formatCompactTokens(avgTokensPerRecord)}</strong>
    </div>
  </div>
</section>

<style>
  .efficiency-bar-wrap {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-bottom: 12px;
  }

  .efficiency-stacked-bar {
    display: flex;
    width: 100%;
    height: 9px;
    border-radius: var(--radius-pill);
    overflow: hidden;
    background: var(--bg-hover);
    gap: 2px;
  }

  .eff-seg.input {
    background: var(--brand-primary);
  }

  .eff-seg.output {
    background: var(--accent-output);
  }

  .eff-seg.cache {
    background: var(--color-exact);
  }

  .efficiency-legend {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 14px;
    font-size: 11px;
    color: var(--text-secondary);
  }

  .efficiency-legend span {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .legend-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    display: inline-block;
  }

  .legend-dot.input {
    background: var(--brand-primary);
  }

  .legend-dot.output {
    background: var(--accent-output);
  }

  .legend-dot.cache {
    background: var(--color-exact);
  }
</style>
