<script lang="ts">
  import { displayCurrency } from './displayCurrency';
  import { findActivityForSession, type TrendViewMode } from '../lib/dashboardView';
  import UsageCurve from './UsageCurve.svelte';
  import UsageHeatmap from './UsageHeatmap.svelte';
  import { formatCost, formatNumber, percentOf } from '../lib/format';
  import type { ChartPoint } from '../lib/dashboardView';
  import type { HeatmapCalendar, HeatmapCell } from '../lib/heatmap';
  import { icons } from '../lib/icons';
  import RollingNumber from './RollingNumber.svelte';
  import { type SessionRates } from './sessionRates';
  import { liveNow } from '../lib/liveClock';
  import type { DashboardSnapshot, SessionActivity, UsageBreakdown, UsageTotals } from '../lib/types';
  import {
    loadDashboardLayout,
    loadDashboardCardGap,
    saveDashboardLayout,
    saveDashboardCardGap,
    resetDashboardLayout,
    reorderCards,
    updateCardWidth,
    setCardVisibility,
    CARD_CATALOG,
    CARD_GAP_OPTIONS,
    DEFAULT_CARD_GAP,
    type CardGap,
    type CardId,
    type CardWidth,
    type DashboardCardConfig,
  } from './dashboardLayout';
  import AddCardDialog from './AddCardDialog.svelte';
  import OverviewSupportCard from './OverviewSupportCard.svelte';

  export let snapshot: DashboardSnapshot;
  export let sessionActivity: SessionActivity[] = [];
  export let sessionRates: SessionRates | null = null;
  export let local: UsageTotals | undefined;
  export let tools: UsageBreakdown[] | undefined = undefined;
  export let sortedModels: UsageBreakdown[] = [];
  export let topModelTotal: number = 1;
  export let scopedProjects: UsageBreakdown[] = [];
  export let activeAntigravityQuotas: DashboardSnapshot['antigravity_quota'] = [];
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
  export let onNavigateModels: () => void = () => {};
  export let onNavigateProjects: () => void = () => {};
  export let onNavigateQuota: () => void = () => {};
  export let rollingNumbersEnabled = true;
  export let isEditingLayout = false;
  export let onToggleEditLayout: () => void = () => {};

  let cards: DashboardCardConfig[] = loadDashboardLayout();
  let cardGap = loadDashboardCardGap();
  let showAddCardModal = false;
  let draggedIndex: number | null = null;
  let dragOverIndex: number | null = null;

  $: visibleCards = cards.filter((c) => c.visible);

  $: recentSessions = snapshot.session_usage.sessions.slice(0, 4);
  $: pendingActivity = sessionActivity.filter(
    (item) =>
      Boolean(item.display_name?.trim()) &&
      $liveNow >= item.observed_at_ms &&
      $liveNow - item.observed_at_ms <= 120_000 &&
      !recentSessions.some((session) => findActivityForSession(session, sessionActivity)?.session_id === item.session_id)
  ).slice(0, 4);
  $: visibleRecentSessions = recentSessions.slice(0, Math.max(0, 4 - pendingActivity.length));
  $: activeTools = tools ?? snapshot.by_tool;
  $: untimedRecordCount = activeTools.reduce((sum, tool) => sum + tool.untimed_records, 0);
  $: heatmapRangeLabel = `${heatmapCalendar.rangeStartDate} 至 ${heatmapCalendar.rangeEndDate}`;

  $: allModelTokens = sortedModels.reduce((sum, m) => sum + m.total_tokens, 0);
  $: visibleProjects = scopedProjects.filter((p) => p.key !== 'no-project' && p.label !== '未关联项目');
  $: allProjectTokens = visibleProjects.reduce((sum, p) => sum + p.total_tokens, 0);
  $: topProjectTotal = Math.max(1, ...visibleProjects.map((p) => p.total_tokens));

  function widthClass(width: CardWidth): string {
    switch (width) {
      case '1/3':
        return 'col-span-4';
      case '1/2':
        return 'col-span-6';
      case '2/3':
        return 'col-span-8';
      case 'full':
      default:
        return 'col-span-12';
    }
  }

  function handleDragStart(e: DragEvent, index: number) {
    draggedIndex = index;
    if (e.dataTransfer) {
      e.dataTransfer.effectAllowed = 'move';
      e.dataTransfer.setData('text/plain', String(index));
      const target = e.currentTarget as HTMLElement | null;
      const cardEl = target?.closest('.overview-card-wrapper') as HTMLElement | null;
      if (cardEl && e.dataTransfer.setDragImage) {
        e.dataTransfer.setDragImage(cardEl, 30, 20);
      }
    }
  }

  function handleDragEnter(e: DragEvent, index: number) {
    if (draggedIndex === null || draggedIndex === index) return;
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }
    dragOverIndex = index;
  }

  function handleDragOver(e: DragEvent, index: number) {
    if (draggedIndex === null || draggedIndex === index) return;
    e.preventDefault();
    if (e.dataTransfer) {
      e.dataTransfer.dropEffect = 'move';
    }
    dragOverIndex = index;
  }

  function handleDrop(e: DragEvent, targetIndex: number) {
    const fromIndex = draggedIndex;
    if (fromIndex === null) return;

    e.preventDefault();
    e.stopPropagation();
    handleDragEnd();
    if (fromIndex === targetIndex) return;

    const sourceCard = visibleCards[fromIndex];
    const targetCard = visibleCards[targetIndex];
    if (!sourceCard || !targetCard) return;

    const sourceIndexInAll = cards.findIndex((c) => c.id === sourceCard.id);
    const targetIndexInAll = cards.findIndex((c) => c.id === targetCard.id);

    cards = reorderCards(cards, sourceIndexInAll, targetIndexInAll);
    saveDashboardLayout(cards);
  }

  function handleDragEnd() {
    draggedIndex = null;
    dragOverIndex = null;
  }

  function handleMoveCard(index: number, delta: number) {
    const targetIndex = index + delta;
    if (targetIndex < 0 || targetIndex >= visibleCards.length) return;

    const sourceCard = visibleCards[index];
    const targetCard = visibleCards[targetIndex];
    if (!sourceCard || !targetCard) return;

    const sourceIndexInAll = cards.findIndex((c) => c.id === sourceCard.id);
    const targetIndexInAll = cards.findIndex((c) => c.id === targetCard.id);

    cards = reorderCards(cards, sourceIndexInAll, targetIndexInAll);
    saveDashboardLayout(cards);
  }

  function handleSetWidth(id: CardId, width: CardWidth) {
    cards = updateCardWidth(cards, id, width);
    saveDashboardLayout(cards);
  }

  function handleHideCard(id: CardId) {
    cards = setCardVisibility(cards, id, false);
    saveDashboardLayout(cards);
  }

  function handleAddCard(id: CardId) {
    cards = setCardVisibility(cards, id, true);
    saveDashboardLayout(cards);
  }

  function handleResetDefaultLayout() {
    cards = resetDashboardLayout();
    cardGap = DEFAULT_CARD_GAP;
  }

  function handleSetCardGap(gap: CardGap) {
    cardGap = gap;
    saveDashboardCardGap(gap);
  }
</script>

<div class="overview-layout">
  <div class="page-header overview-header">
    <div class="overview-header-left">
      <h1>使用总览</h1>
    </div>
    <div class="overview-header-right">
      <button
        type="button"
        class="overview-layout-toggle-btn"
        class:active={isEditingLayout}
        onclick={onToggleEditLayout}
        title={isEditingLayout ? '退出布局编辑' : '自定义看板卡片大小与位置'}
      >
        {@html isEditingLayout ? icons.check : icons.edit}
        <span>{isEditingLayout ? '完成编辑' : '自定义布局'}</span>
      </button>
    </div>
  </div>

  {#if isEditingLayout}
    <div class="layout-edit-banner" role="region" aria-label="看板布局自定义工具栏">
      <div class="edit-banner-info">
        <span class="edit-banner-badge">{@html icons.layout} 布局编辑模式</span>
        <span class="edit-banner-desc">拖拽卡片调整排序，点击卡片右上角尺寸按钮调整宽度，或增删数据显示模块</span>
      </div>
      <div class="edit-banner-actions">
        <div class="edit-banner-spacing">
          <span>卡片间距</span>
          <div class="card-size-group" role="group" aria-label="卡片间距">
            {#each CARD_GAP_OPTIONS as option (option.value)}
              <button
                type="button"
                class="size-pill"
                class:active={cardGap === option.value}
                aria-pressed={cardGap === option.value}
                onclick={() => handleSetCardGap(option.value)}
              >{option.label}</button>
            {/each}
          </div>
        </div>
        <button type="button" class="btn-banner-action" onclick={() => (showAddCardModal = true)}>
          {@html icons.plus} 添加卡片
        </button>
        <button type="button" class="btn-banner-action" onclick={handleResetDefaultLayout}>
          {@html icons.undo} 恢复默认
        </button>
        <button type="button" class="btn-banner-primary" onclick={onToggleEditLayout}>
          {@html icons.check} 完成编辑
        </button>
      </div>
    </div>
  {/if}

  <!-- Unified 12-Column Responsive Overview Grid -->
  <div class="overview-grid" style:--overview-card-gap={`${cardGap}px`} class:is-editing={isEditingLayout} class:is-dragging-any={draggedIndex !== null}>
    {#each visibleCards as card, idx (card.id)}
      {@const meta = CARD_CATALOG[card.id]}
      <div
        class={`overview-card-wrapper ${widthClass(card.width)}`}
        class:is-editing={isEditingLayout}
        class:is-dragging={draggedIndex === idx}
        class:is-drop-target={dragOverIndex === idx}
        ondragover={(e) => handleDragOver(e, idx)}
        ondragenter={(e) => handleDragEnter(e, idx)}
        ondrop={(e) => handleDrop(e, idx)}
        role="region"
        aria-label={meta.title}
      >
        {#if isEditingLayout}
          <div class="card-edit-bar">
            <div
              role="button"
              tabindex="0"
              class="card-drag-handle"
              draggable="true"
              ondragstart={(e) => handleDragStart(e, idx)}
              ondragend={handleDragEnd}
              onkeydown={(e) => {
                if (e.key === 'ArrowUp') handleMoveCard(idx, -1);
                if (e.key === 'ArrowDown') handleMoveCard(idx, 1);
              }}
              title="按住拖拽以重排位置，也可按键盘 ↑ / ↓ 键移动"
            >
              <span class="drag-icon">{@html icons.grip}</span>
              <span class="drag-title">{meta.title}</span>
            </div>

            {#if meta.canResize}
              <div class="card-size-group" role="group" aria-label="卡片尺寸">
                <button
                  type="button"
                  class="size-pill"
                  class:active={card.width === '1/3'}
                  onclick={() => handleSetWidth(card.id, '1/3')}
                >
                  1/3
                </button>
                <button
                  type="button"
                  class="size-pill"
                  class:active={card.width === '1/2'}
                  onclick={() => handleSetWidth(card.id, '1/2')}
                >
                  1/2
                </button>
                <button
                  type="button"
                  class="size-pill"
                  class:active={card.width === '2/3'}
                  onclick={() => handleSetWidth(card.id, '2/3')}
                >
                  2/3
                </button>
                <button
                  type="button"
                  class="size-pill"
                  class:active={card.width === 'full'}
                  onclick={() => handleSetWidth(card.id, 'full')}
                >
                  全宽
                </button>
              </div>
            {/if}

            <div class="card-control-actions">
              <button
                type="button"
                class="quick-move-btn"
                disabled={idx === 0}
                title="向前移动"
                onclick={() => handleMoveCard(idx, -1)}
              >
                {@html icons.arrowUp}
              </button>
              <button
                type="button"
                class="quick-move-btn"
                disabled={idx === visibleCards.length - 1}
                title="向后移动"
                onclick={() => handleMoveCard(idx, 1)}
              >
                {@html icons.arrowDown}
              </button>
              <button
                type="button"
                class="card-hide-btn"
                title="从看板中隐藏"
                onclick={() => handleHideCard(card.id)}
              >
                {@html icons.close}
              </button>
            </div>
          </div>
        {/if}

        <!-- Card Contents -->
        {#if card.id === 'metrics'}
          <!-- 4 Metric Cards -->
          <div class="metric-grid">
            <article class="metric-card hero-card">
              <div class="metric-head">
                <span class="metric-title">总 Token</span>
                <span class="precision-chip exact" title={`${exactRatio}% 的用量记录为精确精度`}>
                  {@html icons.shield} 精确 {exactRatio}%
                </span>
              </div>
              <div class="metric-val">
                <RollingNumber value={local?.total_tokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-split-bar" aria-label="Token 构成比例">
                <div class="metric-split-seg input" style={`width: ${inputShareRate}%`} title={`输入 ${inputShareRate}%`}></div>
                <div class="metric-split-seg output" style={`width: ${outputShareRate}%`} title={`输出 ${outputShareRate}%`}></div>
                <div class="metric-split-seg cache" style={`width: ${cacheShareRate}%`} title={`缓存 ${cacheShareRate}%`}></div>
              </div>
              <div class="metric-foot">
                <span>非缓存 <strong>{formatNumber(Math.max(0, (local?.total_tokens ?? 0) - cacheTokens))}</strong></span>
                {#if local?.estimated_cost_microusd != null}
                  <span title="按公开基础费率估算。Antigravity 未分类 Token 按输入价；缺报价时参考 Gemini 3 Flash。">基础估算 <strong>{formatCost(local.estimated_cost_microusd, $displayCurrency)}</strong></span>
                {:else}
                  <span class="metric-foot-sub">含缓存读写</span>
                {/if}
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span class="metric-title">输入 Token</span>
                <span class="panel-tag tabular">占总量 {inputShareRate}%</span>
              </div>
              <div class="metric-val">
                <RollingNumber value={local?.input_tokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-split-bar" aria-hidden="true">
                <div class="metric-split-seg input" style={`width: ${Math.min(100, Math.max(4, Number(inputShareRate) || 0))}%`}></div>
              </div>
              <div class="metric-foot">
                <span>净输入上下文</span>
                <span class="metric-foot-sub">不含缓存读写</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span class="metric-title">输出 Token</span>
                <span class="panel-tag tabular">推理 {reasoningShareRate}%</span>
              </div>
              <div class="metric-val">
                <RollingNumber value={local?.output_tokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-split-bar" aria-hidden="true">
                <div class="metric-split-seg output" style={`width: ${Math.min(100, Math.max(4, Number(reasoningShareRate) || 0))}%`}></div>
              </div>
              <div class="metric-foot">
                <span>推理 <strong>{formatNumber(local?.reasoning_tokens)}</strong></span>
                <span class="metric-foot-sub">占总量 {outputShareRate}%</span>
              </div>
            </article>

            <article class="metric-card">
              <div class="metric-head">
                <span class="metric-title">缓存 Token</span>
                <span class="precision-chip exact" title={`输入缓存命中率 ${cacheHitRate}%`}>缓存率 {cacheHitRate}%</span>
              </div>
              <div class="metric-val">
                <RollingNumber value={cacheTokens} enabled={rollingNumbersEnabled} />
              </div>
              <div class="metric-split-bar" aria-hidden="true">
                <div class="metric-split-seg cache" style={`width: ${Math.min(100, Math.max(4, Number(cacheHitRate) || 0))}%`}></div>
              </div>
              <div class="metric-foot">
                <span>读取 <strong>{formatNumber(local?.cache_read_tokens)}</strong></span>
                <span>写入 <strong>{formatNumber(local?.cache_write_tokens)}</strong></span>
              </div>
            </article>
          </div>

        {:else if card.id === 'integrity'}
          <!-- Data Precision Integrity Guardrail -->
          <div class="integrity-banner">
            <div class="integrity-left">
              <span class="integrity-item" class:is-zero={(local?.exact_records ?? 0) === 0}>
                <span class="integrity-dot exact"></span>精确记录 <strong>{formatNumber(local?.exact_records)}</strong> 笔
              </span>
              <span class="integrity-item" class:is-zero={(local?.aggregate_records ?? 0) === 0}>
                <span class="integrity-dot aggregate"></span>聚合粗粒度 <strong>{formatNumber(local?.aggregate_records)}</strong> 笔
              </span>
              <span class="integrity-item" class:is-zero={(local?.estimated_records ?? 0) === 0}>
                <span class="integrity-dot estimated"></span>显式估算 <strong>{formatNumber(local?.estimated_records)}</strong> 笔
              </span>
              {#if (local?.unknown_precision_records ?? 0) > 0}
                <span class="integrity-item">
                  <span class="integrity-dot muted"></span>精度未知 <strong>{formatNumber(local?.unknown_precision_records)}</strong> 笔
                </span>
              {/if}
              {#if (local?.priced_records ?? 0) > 0}
                <span class="integrity-item">
                  计价 <strong>{formatNumber(local?.priced_records)} / {formatNumber((local?.priced_records ?? 0) + (local?.unpriced_records ?? 0))}</strong> 笔
                </span>
              {/if}
            </div>
            <div class="integrity-right">
              {#if untimedRecordCount > 0}
                <span class="warning" title="已计入累计但不进入日趋势">
                  时间未知 {formatNumber(untimedRecordCount)} 条（不入日趋势）
                </span>
              {/if}
              {#if snapshot.unassigned.total_tokens > 0}
                <span>未归属 {formatNumber(snapshot.unassigned.total_tokens)} Token</span>
              {:else if untimedRecordCount === 0}
                <span class="integrity-clean-badge">时间戳与归属校验完整 · 共 {formatNumber(local?.record_count)} 笔有效记录</span>
              {/if}
            </div>
          </div>

        {:else if card.id === 'trend'}
          <!-- Trend Chart -->
          <section class="panel-card">
            <div class="panel-head">
              <div>
                <h2>Token 使用趋势</h2>
                <p>
                  {trendViewMode === 'heatmap'
                    ? `完整历史 · ${heatmapCalendar.weeks.length} 周 · ${heatmapRangeLabel}`
                    : '按本机时区自然日汇集'}
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
              <UsageCurve bind:hoveredCurveIndex {hoveredCurvePoint} {chartValues} {chartMax} {chartPoints} {inputShareRate} {outputShareRate} {cacheShareRate} />
            {:else}
              <UsageHeatmap {heatmapCalendar} bind:hoveredHeatmapCell />
            {/if}
          </section>

        {:else}
          <OverviewSupportCard
            cardId={card.id}
            {snapshot}
            {local}
            {activeTools}
            {topToolTotal}
            {sortedModels}
            {allModelTokens}
            {topModelTotal}
            {pendingActivity}
            {visibleRecentSessions}
            {sessionActivity}
            {sessionRates}
            {visibleProjects}
            {allProjectTokens}
            {topProjectTotal}
            {activeAntigravityQuotas}
            {onNavigateTools}
            {onNavigateModels}
            {onNavigateSessions}
            {onNavigateProjects}
            {onNavigateQuota}
          />
        {/if}
      </div>
    {/each}
  </div>
</div>

{#if showAddCardModal}
  <AddCardDialog
    {cards}
    onAddCard={handleAddCard}
    onClose={() => (showAddCardModal = false)}
  />
{/if}
