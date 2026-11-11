<script lang="ts">
  import { CARD_CATALOG, type CardId, type DashboardCardConfig } from './dashboardLayout';
  import { icons } from './icons';

  export let cards: DashboardCardConfig[];
  export let onAddCard: (id: CardId) => void;
  export let onClose: () => void;

  const catalogList = Object.values(CARD_CATALOG);

  $: visibleSet = new Set(cards.filter((c) => c.visible).map((c) => c.id));

  function cardIcon(id: CardId): string {
    switch (id) {
      case 'metrics':
        return icons.dashboard;
      case 'integrity':
        return icons.shield;
      case 'trend':
        return icons.sparkle;
      case 'tools':
        return icons.tools;
      case 'models':
        return icons.models;
      case 'sessions':
        return icons.sessions;
      case 'sources':
        return icons.database;
      case 'projects':
        return icons.projects;
      case 'quotas':
        return icons.quota;
      default:
        return icons.layout;
    }
  }
</script>

<svelte:window
  onkeydown={(e) => {
    if (e.key === 'Escape') onClose();
  }}
/>

<div class="card-dialog-backdrop" onclick={onClose} role="presentation">
  <div
    class="card-dialog"
    role="dialog"
    aria-modal="true"
    aria-labelledby="card-dialog-title"
    tabindex="-1"
    onclick={(e) => e.stopPropagation()}
    onkeydown={(e) => {
      if (e.key === 'Escape') onClose();
    }}
  >
    <div class="card-dialog-head">
      <div class="card-dialog-title-group">
        <span class="card-dialog-icon">{@html icons.layout}</span>
        <div>
          <h2 id="card-dialog-title">添加卡片到看板</h2>
          <p>选择要放置在「使用总览」的数据模块</p>
        </div>
      </div>
      <button class="icon-btn" aria-label="关闭弹窗" onclick={onClose}>
        {@html icons.close}
      </button>
    </div>

    <div class="card-dialog-body">
      <div class="card-catalog-grid">
        {#each catalogList as item}
          {@const isVisible = visibleSet.has(item.id)}
          <div class="card-catalog-card" class:is-visible={isVisible}>
            <div class="card-catalog-header">
              <span class="card-catalog-lead-icon">{@html cardIcon(item.id)}</span>
              <div class="card-catalog-info">
                <h3>{item.title}</h3>
                <p>{item.description}</p>
              </div>
            </div>
            <div class="card-catalog-action">
              {#if isVisible}
                <span class="card-added-chip">
                  {@html icons.check} 已在看板中
                </span>
              {:else}
                <button
                  type="button"
                  class="btn-add-card"
                  onclick={() => onAddCard(item.id)}
                >
                  {@html icons.plus} 添加到看板
                </button>
              {/if}
            </div>
          </div>
        {/each}
      </div>
    </div>

    <div class="card-dialog-foot">
      <button type="button" class="btn-done" onclick={onClose}>
        完成
      </button>
    </div>
  </div>
</div>
