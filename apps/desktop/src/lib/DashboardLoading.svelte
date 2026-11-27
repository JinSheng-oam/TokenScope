<script lang="ts">
  import { onMount } from 'svelte';
  import LoadingStatus from './LoadingStatus.svelte';
  import {
    cardWidthToGridClass,
    dashboardLayoutPrefs,
    refreshDashboardLayoutPrefs,
    type CardId,
  } from './dashboardLayout';

  export let activeTab = 'overview';

  const METRIC_SKELETONS = [
    { titleW: '38%', valW: '72%', subW: '44%' },
    { titleW: '42%', valW: '62%', subW: '36%' },
    { titleW: '40%', valW: '56%', subW: '40%' },
    { titleW: '44%', valW: '68%', subW: '48%' },
  ];

  const TREND_BAR_HEIGHTS = [38, 56, 44, 74, 62, 88, 52, 68, 92, 60, 78, 84, 48, 70];
  const ACTIVITY_BAR_HEIGHTS = [42, 64, 50, 80, 58, 74, 46, 90, 62, 70, 54, 84, 66, 76];
  const RANK_WIDTHS = ['86%', '68%', '50%', '34%', '22%'];
  const ROW_TITLE_WIDTHS = ['46%', '58%', '40%', '52%', '36%'];

  function rowCountFor(cardRows: number | undefined, fallbackRows: number): number {
    return Math.min(Math.max(2, cardRows ?? fallbackRows), 5);
  }

  function isRankCard(id: CardId): boolean {
    return id === 'tools' || id === 'models' || id === 'projects' || id === 'cost' || id === 'devices';
  }

  onMount(() => {
    refreshDashboardLayoutPrefs();
  });

  $: visibleCards = $dashboardLayoutPrefs.cards.filter((card) => card.visible);
  $: cardGap = $dashboardLayoutPrefs.gap;
  $: defaultRows = $dashboardLayoutPrefs.defaultRows;
</script>

<div class="loading-state" role="status" aria-live="polite" data-loading-tab={activeTab}>
  {#if activeTab === 'overview'}
    <LoadingStatus message="正在加载用量…" stage="WAL 并发只读通道" />

    {#if visibleCards.length > 0}
      <div
        class="overview-grid loading-overview-grid"
        style:--overview-card-gap={`${cardGap}px`}
        aria-hidden="true"
      >
        {#each visibleCards as card, idx (card.id)}
          {@const rows = rowCountFor(card.rows, defaultRows)}
          <div
            class={`overview-card-wrapper ${cardWidthToGridClass(card.width)}`}
            data-card-id={card.id}
            data-card-width={card.width}
          >
            {#if card.id === 'metrics'}
              <div class="metric-grid">
                {#each METRIC_SKELETONS as item, i}
                  <div class="loading-skeleton-card" style={`--delay: ${i * 75}ms`}>
                    <div class="sk-row-between">
                      <span class="sk-bone sk-title" style={`width: ${item.titleW}`}></span>
                      <span class="sk-bone sk-pill"></span>
                    </div>
                    <span class="sk-bone sk-value" style={`width: ${item.valW}`}></span>
                    <span class="sk-bone sk-bar"></span>
                    <div class="sk-row-between">
                      <span class="sk-bone sk-foot" style={`width: ${item.subW}`}></span>
                      <span class="sk-bone sk-foot" style="width: 24%"></span>
                    </div>
                  </div>
                {/each}
              </div>
            {:else if card.id === 'integrity'}
              <div class="loading-integrity-skeleton" style={`--delay: ${idx * 60}ms`}>
                <div class="sk-inline-group">
                  <span class="sk-bone sk-tag" style="width: 96px"></span>
                  <span class="sk-bone sk-tag" style="width: 104px"></span>
                  <span class="sk-bone sk-tag" style="width: 84px"></span>
                </div>
                <span class="sk-bone sk-tag" style="width: 156px"></span>
              </div>
            {:else if card.id === 'trend'}
              <div class="loading-skeleton-panel sk-trend-panel" style={`--delay: ${idx * 60}ms`}>
                <div class="sk-panel-head">
                  <div class="sk-stack">
                    <span class="sk-bone sk-h2" style="width: 124px"></span>
                    <span class="sk-bone sk-sub" style="width: 210px"></span>
                  </div>
                  <span class="sk-bone sk-switch"></span>
                </div>
                <div class="sk-chart-bars">
                  {#each TREND_BAR_HEIGHTS as h, barIdx}
                    <span class="sk-chart-col" style={`height: ${h}%; --bar-delay: ${barIdx * 40}ms`}></span>
                  {/each}
                </div>
              </div>
            {:else if card.id === 'activity'}
              <div class="loading-skeleton-panel sk-compact-panel" style={`--delay: ${idx * 60}ms`}>
                <div class="sk-panel-head">
                  <div class="sk-stack">
                    <span class="sk-bone sk-h2" style="width: 112px"></span>
                    <span class="sk-bone sk-sub" style="width: 168px"></span>
                  </div>
                  <span class="sk-bone sk-pill"></span>
                </div>
                <div class="sk-mini-grid">
                  {#each [1, 2, 3, 4] as _}
                    <div class="sk-mini-cell">
                      <span class="sk-bone sk-foot" style="width: 56%"></span>
                      <span class="sk-bone sk-h2" style="width: 74%"></span>
                    </div>
                  {/each}
                </div>
                <div class="sk-chart-bars sk-mini-bars">
                  {#each ACTIVITY_BAR_HEIGHTS as h, barIdx}
                    <span class="sk-chart-col" style={`height: ${h}%; --bar-delay: ${barIdx * 35}ms`}></span>
                  {/each}
                </div>
              </div>
            {:else if card.id === 'efficiency'}
              <div class="loading-skeleton-panel sk-compact-panel" style={`--delay: ${idx * 60}ms`}>
                <div class="sk-panel-head">
                  <div class="sk-stack">
                    <span class="sk-bone sk-h2" style="width: 118px"></span>
                    <span class="sk-bone sk-sub" style="width: 164px"></span>
                  </div>
                  <span class="sk-bone sk-pill"></span>
                </div>
                <span class="sk-bone sk-stack-bar"></span>
                <div class="sk-mini-grid">
                  {#each [1, 2, 3, 4] as _}
                    <div class="sk-mini-cell">
                      <span class="sk-bone sk-foot" style="width: 58%"></span>
                      <span class="sk-bone sk-h2" style="width: 72%"></span>
                    </div>
                  {/each}
                </div>
              </div>
            {:else if isRankCard(card.id)}
              <div class="loading-skeleton-panel sk-compact-panel" style={`--delay: ${idx * 60}ms`}>
                <div class="sk-panel-head">
                  <div class="sk-stack">
                    <span class="sk-bone sk-h2" style="width: 108px"></span>
                    <span class="sk-bone sk-sub" style="width: 152px"></span>
                  </div>
                  <span class="sk-bone sk-pill"></span>
                </div>
                <div class="sk-rank-list">
                  {#each RANK_WIDTHS.slice(0, rows) as w}
                    <div class="sk-rank-item">
                      <div class="sk-row-between">
                        <span class="sk-bone sk-title" style="width: 36%"></span>
                        <span class="sk-bone sk-title" style="width: 24%"></span>
                      </div>
                      <span class="sk-bone sk-bar" style={`width: ${w}`}></span>
                    </div>
                  {/each}
                </div>
              </div>
            {:else}
              <div class="loading-skeleton-panel sk-compact-panel" style={`--delay: ${idx * 60}ms`}>
                <div class="sk-panel-head">
                  <div class="sk-stack">
                    <span class="sk-bone sk-h2" style="width: 104px"></span>
                    <span class="sk-bone sk-sub" style="width: 168px"></span>
                  </div>
                  <span class="sk-bone sk-pill"></span>
                </div>
                <div class="sk-session-list">
                  {#each ROW_TITLE_WIDTHS.slice(0, rows) as titleW}
                    <div class="sk-session-row">
                      <div class="sk-stack" style="flex: 1; min-width: 0;">
                        <span class="sk-bone sk-title" style={`width: ${titleW}`}></span>
                        <span class="sk-bone sk-foot" style="width: 32%"></span>
                      </div>
                      <span class="sk-bone sk-title" style="width: 68px"></span>
                    </div>
                  {/each}
                </div>
              </div>
            {/if}
          </div>
        {/each}
      </div>
    {:else}
      <div class="loading-skeleton-panel sk-empty-layout" aria-hidden="true">
        <div class="sk-panel-head">
          <div class="sk-stack">
            <span class="sk-bone sk-h2" style="width: 140px"></span>
            <span class="sk-bone sk-sub" style="width: 240px"></span>
          </div>
        </div>
      </div>
    {/if}
  {:else}
    <div class="loading-page-head" aria-hidden="true">
      <span class="sk-bone sk-page-title"></span>
      <LoadingStatus message="正在加载用量…" stage="WAL 并发只读通道" />
    </div>

    {#if activeTab === 'quota'}
      <div class="loading-quota-grid" aria-hidden="true">
        <div class="sk-quota-stack">
          <div class="loading-skeleton-panel sk-compact-panel">
            <div class="sk-panel-head">
              <span class="sk-bone sk-h2" style="width: 120px"></span>
              <span class="sk-bone sk-pill"></span>
            </div>
            <div class="sk-session-list">
              {#each [1, 2] as _}
                <div class="sk-session-row">
                  <span class="sk-bone sk-title" style="width: 24%"></span>
                  <span class="sk-bone sk-bar" style="width: 42%"></span>
                  <span class="sk-bone sk-title" style="width: 18%"></span>
                </div>
              {/each}
            </div>
          </div>
          <div class="loading-skeleton-panel sk-compact-panel">
            <div class="sk-panel-head">
              <span class="sk-bone sk-h2" style="width: 148px"></span>
              <span class="sk-bone sk-pill"></span>
            </div>
            <div class="sk-session-list">
              {#each [1, 2, 3, 4] as _}
                <div class="sk-session-row">
                  <span class="sk-bone sk-title" style="width: 28%"></span>
                  <span class="sk-bone sk-bar" style="width: 40%"></span>
                  <span class="sk-bone sk-title" style="width: 18%"></span>
                </div>
              {/each}
            </div>
          </div>
        </div>
        <div class="loading-skeleton-panel sk-compact-panel">
          <div class="sk-panel-head">
            <span class="sk-bone sk-h2" style="width: 92px"></span>
            <span class="sk-bone sk-pill"></span>
          </div>
          <div class="sk-session-list">
            {#each ROW_TITLE_WIDTHS as titleW}
              <div class="sk-session-row">
                <span class="sk-bone sk-title" style={`width: ${titleW}`}></span>
                <span class="sk-bone sk-foot" style="width: 26%"></span>
              </div>
            {/each}
          </div>
        </div>
      </div>
    {:else if activeTab === 'devices'}
      <div class="sk-three-grid" aria-hidden="true">
        {#each METRIC_SKELETONS.slice(0, 3) as item}
          <div class="loading-skeleton-card">
            <span class="sk-bone sk-title" style={`width: ${item.titleW}`}></span>
            <span class="sk-bone sk-value" style={`width: ${item.valW}`}></span>
            <span class="sk-bone sk-foot" style={`width: ${item.subW}`}></span>
          </div>
        {/each}
      </div>
      <div class="loading-skeleton-panel sk-compact-panel" aria-hidden="true">
        <div class="sk-panel-head">
          <span class="sk-bone sk-h2" style="width: 132px"></span>
          <span class="sk-bone sk-pill"></span>
        </div>
        <div class="sk-session-list">
          {#each ROW_TITLE_WIDTHS.slice(0, 4) as titleW}
            <div class="sk-session-row">
              <span class="sk-bone sk-title" style={`width: ${titleW}`}></span>
              <span class="sk-bone sk-title" style="width: 22%"></span>
            </div>
          {/each}
        </div>
      </div>
    {:else}
      <div class="loading-skeleton-panel sk-compact-panel" aria-hidden="true">
        <div class="sk-panel-head">
          <div class="sk-stack">
            <span class="sk-bone sk-h2" style="width: 136px"></span>
            <span class="sk-bone sk-sub" style="width: 210px"></span>
          </div>
          <span class="sk-bone sk-pill"></span>
        </div>
        <div class="sk-session-list">
          {#each ROW_TITLE_WIDTHS as titleW}
            <div class="sk-session-row">
              <div class="sk-stack" style="flex: 1; min-width: 0;">
                <span class="sk-bone sk-title" style={`width: ${titleW}`}></span>
                <span class="sk-bone sk-foot" style="width: 28%"></span>
              </div>
              <span class="sk-bone sk-title" style="width: 84px"></span>
            </div>
          {/each}
        </div>
      </div>
    {/if}
  {/if}
</div>
