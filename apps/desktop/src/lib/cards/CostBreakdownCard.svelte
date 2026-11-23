<script lang="ts">
  import { displayCurrency } from '../displayCurrency';
  import { formatCost, formatNumber, percentOf } from '../format';
  import type { UsageBreakdown, UsageTotals } from '../types';

  let {
    cardRows = 5,
    local,
    sortedModels = [],
    onNavigatePricing = () => {},
  }: {
    cardRows?: number;
    local?: UsageTotals;
    sortedModels?: UsageBreakdown[];
    onNavigatePricing?: () => void;
  } = $props();

  function compactModelLabel(label: string): string {
    const [provider, model, ...rest] = label.split('/');
    return provider && model && rest.length === 0 && model.toLowerCase().startsWith(`${provider.toLowerCase()}-`)
      ? model
      : label;
  }

  let costRankedModels = $derived(
    [...sortedModels]
      .filter((m) => (m.estimated_cost_microusd ?? 0) > 0)
      .sort((a, b) => (b.estimated_cost_microusd ?? 0) - (a.estimated_cost_microusd ?? 0)),
  );
  let totalCostMicrousd = $derived(
    local?.estimated_cost_microusd ?? costRankedModels.reduce((s, m) => s + (m.estimated_cost_microusd ?? 0), 0),
  );
  let topModelCostMicrousd = $derived(
    Math.max(1, ...costRankedModels.map((m) => m.estimated_cost_microusd ?? 0)),
  );
  let pricedRecords = $derived(local?.priced_records ?? 0);
  let totalPriceableRecords = $derived(pricedRecords + (local?.unpriced_records ?? 0));
  let pricingCoveragePct = $derived(
    totalPriceableRecords > 0 ? Math.round((pricedRecords / totalPriceableRecords) * 100) : 0,
  );
</script>

<section class="panel-card cost-breakdown-card">
  <div class="panel-head">
    <div>
      <h2>费用估算分析</h2>
    </div>
    <button type="button" class="panel-tag" onclick={onNavigatePricing}>模型定价 →</button>
  </div>

  <div class="cost-hero-row">
    <div class="cost-hero-main">
      <span class="cost-hero-label">当前范围预估</span>
      <strong class="cost-hero-value tabular">
        {local?.estimated_cost_microusd != null ? formatCost(local.estimated_cost_microusd, $displayCurrency) : '暂无报价'}
      </strong>
    </div>
    {#if totalPriceableRecords > 0}
      <span class="panel-tag tabular" title={`已计价 ${formatNumber(pricedRecords)} / ${formatNumber(totalPriceableRecords)} 笔`}>
        计价覆盖 {pricingCoveragePct}%
      </span>
    {/if}
  </div>

  {#if costRankedModels.length > 0}
    <div class="rank-list">
      {#each costRankedModels.slice(0, cardRows) as model, idx}
        {@const modelCost = model.estimated_cost_microusd ?? 0}
        <div class="rank-item">
          <div class="rank-row">
            <span class="rank-title">
              <span class="rank-num tabular">{String(idx + 1).padStart(2, '0')}</span>
              <strong title={model.label}>{compactModelLabel(model.label)}</strong>
            </span>
            <span class="rank-meta">
              <span class="rank-value tabular">{formatCost(modelCost, $displayCurrency)}</span>
              <span class="rank-share tabular">{percentOf(modelCost, Math.max(1, totalCostMicrousd)).toFixed(1)}%</span>
            </span>
          </div>
          <div class="rank-track">
            <div class="rank-fill cost-fill" style={`width: ${percentOf(modelCost, topModelCostMicrousd)}%`}></div>
          </div>
        </div>
      {/each}
    </div>
  {:else}
    <div class="empty-box">暂无可计价的模型消耗记录</div>
  {/if}
</section>

<style>
  .cost-hero-row {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    margin-bottom: 12px;
    padding-bottom: 10px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .cost-hero-main {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .cost-hero-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .cost-hero-value {
    font-size: 22px;
    font-weight: 750;
    letter-spacing: -0.02em;
    color: var(--text-primary);
    line-height: 1.2;
  }

  .rank-fill.cost-fill {
    background: linear-gradient(90deg, var(--accent-cost-start) 0%, var(--accent-cost-end) 100%);
  }
</style>
