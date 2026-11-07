<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import CustomSelect from './CustomSelect.svelte';
  import { currencyPreference, displayCurrency, setDisplayCurrency, setDisplayExchangeRate, DEFAULT_EXCHANGE_RATE } from './displayCurrency';
  import { formatNumber } from './format';
  import { icons } from './icons';
  import ManualModelPrice from './ManualModelPrice.svelte';
  import { priceKey, rateColumns as columns, type Catalog, type PriceEntry, type PricingPreferences, type RateKey } from './modelPricing';

  export let runtimeAvailable: boolean;

  const PAGE_SIZES = [10, 25, 50, 100] as const;
  const pageSizeOptions = PAGE_SIZES.map((n) => ({
    key: String(n),
    label: `${n} 条 / 页`,
  }));

  const rateFormat = new Intl.NumberFormat('en-US', { maximumFractionDigits: 9 });
  const dateFormat = new Intl.DateTimeFormat('zh-CN', { dateStyle: 'medium', timeStyle: 'short' });

  let catalog: Catalog | null = null;
  let search = '';
  let provider = '';
  let pageSize = 25;
  let page = 1;
  let loading = false;
  let error = '';
  let pageSizeReady = false;
  let saving = false;
  let formOpen = false;
  let editing: PriceEntry | null = null;
  let manualOnly = false;

  type SortField = 'model' | 'provider' | RateKey;
  let sortBy: SortField = 'model';
  let sortDir: 'asc' | 'desc' = 'asc';

  function toggleSort(field: SortField) {
    if (sortBy === field) {
      sortDir = sortDir === 'asc' ? 'desc' : 'asc';
    } else {
      sortBy = field;
      sortDir = field === 'model' || field === 'provider' ? 'asc' : 'desc';
    }
    page = 1;
  }

  $: manual = catalog?.preferences.manual_entries ?? [];
  $: manualKeys = new Set(manual.map(priceKey));
  $: entries = [...manual, ...(catalog?.entries ?? []).filter((entry) => !manualKeys.has(priceKey(entry)))];
  $: multiplier = $displayCurrency.multiplier;
  $: shownCurrency = $displayCurrency.code;
  $: currencySymbol = shownCurrency === 'CNY' ? '¥' : '$';

  $: if (pageSizeReady) {
    try { localStorage.setItem('tokenscope.pricePageSize', String(pageSize)); } catch { /* Keep the in-memory page size. */ }
  }

  $: providerCounts = entries.reduce((acc, entry) => {
    acc.set(entry.provider_id, (acc.get(entry.provider_id) ?? 0) + 1);
    return acc;
  }, new Map<string, number>());

  $: providerOptions = [
    { key: '', label: '全部厂商', count: entries.length },
    ...Array.from(providerCounts.entries())
      .sort((a, b) => a[0].localeCompare(b[0]))
      .map(([id, count]) => ({ key: id, label: id, count })),
  ];

  $: hasFilters = Boolean(search.trim() || provider || manualOnly);
  $: terms = search.trim().toLowerCase().split(/\s+/).filter(Boolean);
  $: filtered = entries.filter((entry) => {
    if (provider && provider !== entry.provider_id) return false;
    if (manualOnly && !manualKeys.has(priceKey(entry))) return false;
    const label = (entry.provider_id + '/' + entry.model_id).toLowerCase();
    return terms.every((term) => label.includes(term));
  });

  $: sorted = [...filtered].sort((a, b) => {
    const aManual = manualKeys.has(priceKey(a)) ? 1 : 0;
    const bManual = manualKeys.has(priceKey(b)) ? 1 : 0;
    if (aManual !== bManual && sortBy === 'model' && sortDir === 'asc') {
      return bManual - aManual;
    }
    const dir = sortDir === 'asc' ? 1 : -1;
    if (sortBy === 'model') return a.model_id.localeCompare(b.model_id) * dir;
    if (sortBy === 'provider') {
      const cmp = a.provider_id.localeCompare(b.provider_id);
      return (cmp !== 0 ? cmp : a.model_id.localeCompare(b.model_id)) * dir;
    }
    const av = a[sortBy];
    const bv = b[sortBy];
    if (av === null && bv === null) return a.model_id.localeCompare(b.model_id);
    if (av === null) return 1;
    if (bv === null) return -1;
    return (av - bv) * dir;
  });

  $: pages = Math.max(1, Math.ceil(sorted.length / pageSize));
  $: currentPage = Math.min(page, pages);
  $: offset = (currentPage - 1) * pageSize;
  $: visible = sorted.slice(offset, offset + pageSize);

  function clearFilters() {
    search = '';
    provider = '';
    manualOnly = false;
    page = 1;
  }

  async function load() {
    if (!runtimeAvailable || loading) return;
    loading = true;
    error = '';
    try {
      catalog = await invoke<Catalog>('get_model_price_catalog');
      if (provider && ![...catalog.entries, ...catalog.preferences.manual_entries].some((entry) => entry.provider_id === provider)) provider = '';
      page = 1;
    } catch {
      error = '读取价格目录失败，请重试。';
    } finally {
      loading = false;
    }
  }

  async function savePreferences(preferences: PricingPreferences) {
    if (saving || !catalog) return false;
    saving = true;
    error = '';
    try {
      await invoke('save_model_pricing_preferences', { preferences });
      catalog = { ...catalog, preferences };
      return true;
    } catch (reason) {
      error = '无法保存定价设置：' + String(reason);
      return false;
    } finally {
      saving = false;
    }
  }

  async function toggleMatching(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    if (catalog && !(await savePreferences({ ...catalog.preferences, keyword_matching: input.checked }))) {
      input.checked = catalog.preferences.keyword_matching;
    }
  }

  async function saveManual(entry: PriceEntry) {
    if (!catalog) return;
    const manual_entries = [entry, ...manual.filter((item) => priceKey(item) !== priceKey(entry))];
    if (await savePreferences({ ...catalog.preferences, manual_entries })) {
      formOpen = false;
      editing = null;
      page = 1;
      search = '';
      provider = '';
    }
  }

  async function removeManual(entry: PriceEntry) {
    if (!catalog) return;
    const manual_entries = manual.filter((item) => priceKey(item) !== priceKey(entry));
    if (await savePreferences({ ...catalog.preferences, manual_entries })) formOpen = false;
  }

  function displayRate(rate: number | null, factor: number) {
    return rate === null ? '—' : rateFormat.format((rate / 1_000_000_000) * factor);
  }

  onMount(() => {
    try {
      const legacy = JSON.parse(localStorage.getItem('tokenscope.priceCurrency') ?? '{}');
      const savedPageSize = Number(localStorage.getItem('tokenscope.pricePageSize') ?? legacy?.pageSize);
      if ((PAGE_SIZES as readonly number[]).includes(savedPageSize)) pageSize = savedPageSize;
    } catch {
      /* Ignore malformed display preferences. */
    }
    pageSizeReady = true;
    void load();
  });
</script>

<div class="page-header">
  <h1>模型定价</h1>
  <p>{shownCurrency} ({currencySymbol}) / 百万 Token{catalog?.updated_at_ms ? ' · 更新于 ' + dateFormat.format(catalog.updated_at_ms) : ''}</p>
</div>

{#if !runtimeAvailable}
  <div class="empty-box">请在 TokenScope 桌面端查看价格目录。</div>
{:else}
  <section class="panel-card pricing-panel" aria-label="模型价格目录" aria-busy={loading}>
    <div class="panel-head pricing-panel-head">
      <div>
        <h2>模型费率目录</h2>
        <p>
          {#if catalog}
            共收录 <strong>{formatNumber(entries.length)}</strong> 个模型 · 覆盖 <strong>{formatNumber(providerCounts.size)}</strong> 家厂商{#if manual.length > 0} · 含 <strong>{formatNumber(manual.length)}</strong> 条手动报价{/if}
          {:else if loading}
            正在同步模型价格目录…
          {:else}
            按每百万 Token 计费
          {/if}
        </p>
      </div>

      <div class="pricing-head-controls">
        <div class="currency-segment" role="group" aria-label="显示币种">
          <button
            type="button"
            class="seg-btn"
            class:active={$currencyPreference.currency === 'USD'}
            onclick={() => setDisplayCurrency('USD')}
          >
            $ USD
          </button>
          <button
            type="button"
            class="seg-btn"
            class:active={$currencyPreference.currency === 'CNY'}
            onclick={() => setDisplayCurrency('CNY')}
          >
            ¥ CNY
          </button>
        </div>

        {#if $currencyPreference.currency === 'CNY'}
          <label class="rate-inline-box" title="1 美元折合人民币汇率">
            <span>1 USD =</span>
            <input
              type="number"
              class="exchange-rate"
              min="0.000001"
              max="1000"
              step="any"
              value={$currencyPreference.exchangeRate}
              oninput={(event) => setDisplayExchangeRate(event.currentTarget.value === '' ? undefined : event.currentTarget.valueAsNumber)}
              placeholder={String(DEFAULT_EXCHANGE_RATE)}
              aria-label="1 美元折合人民币"
            />
            <span>CNY</span>
          </label>
        {/if}

        <label class="matching-pill" title="自动匹配模型费率；已计价的历史记录保留原估算">
          <input
            type="checkbox"
            checked={catalog?.preferences.keyword_matching ?? true}
            onchange={toggleMatching}
            disabled={!catalog || loading || saving}
          />
          <span>关键词匹配</span>
        </label>

        <div class="pricing-page-size">
          <span class="page-size-label">每页</span>
          <CustomSelect
            value={String(pageSize)}
            options={pageSizeOptions}
            align="right"
            ariaLabel="设置每页显示模型数量"
            onChange={(val) => {
              pageSize = Number(val);
              page = 1;
            }}
          />
        </div>
      </div>
    </div>

    <div class="pricing-filter-bar">
      <div class="pricing-search-wrap">
        <svg class="search-icon" aria-hidden="true" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
          <circle cx="11" cy="11" r="6" /><path d="m16 16 4 4" />
        </svg>
        <input
          type="search"
          bind:value={search}
          oninput={() => (page = 1)}
          placeholder="搜索模型名称或厂商标识（支持多关键词空格分隔）…"
          aria-label="搜索模型或厂商"
        />
        {#if search}
          <button type="button" class="search-clear" onclick={() => { search = ''; page = 1; }} aria-label="清除搜索">✕</button>
        {/if}
      </div>

      <div class="pricing-provider-select">
        <CustomSelect
          value={provider}
          options={providerOptions}
          ariaLabel="按厂商筛选"
          onChange={(val) => {
            provider = val;
            page = 1;
          }}
        />
      </div>

      <button
        type="button"
        class="filter-toggle-chip"
        class:active={manualOnly}
        aria-pressed={manualOnly}
        onclick={() => {
          manualOnly = !manualOnly;
          page = 1;
        }}
      >
        <span>仅手动报价</span>
        {#if manual.length > 0}
          <span class="chip-count">{manual.length}</span>
        {/if}
      </button>

      {#if hasFilters}
        <button type="button" class="btn-secondary btn-clear-filter" onclick={clearFilters}>清除筛选</button>
      {/if}

      <div class="pricing-toolbar-actions">
        <button type="button" class="btn-secondary" disabled={loading || saving} onclick={load}>
          <span class:spin={loading}>{@html icons.refresh}</span>
          <span>重载目录</span>
        </button>
        <button
          type="button"
          class="btn-primary"
          disabled={!catalog || saving || loading}
          onclick={() => {
            editing = null;
            formOpen = !formOpen || editing !== null;
          }}
        >
          + 添加模型
        </button>
      </div>
    </div>

    {#if formOpen}
      {#key editing}
        <ManualModelPrice entry={editing} busy={saving} onSave={saveManual} onCancel={() => { formOpen = false; editing = null; }} />
      {/key}
    {/if}

    {#if error}
      <div class="notice-box" role="alert">{error}</div>
    {/if}

    {#if visible.length}
      <div class="table-wrap pricing-table-wrap" class:is-refreshing={loading}>
        {#if loading}
          <div class="session-table-progress" aria-hidden="true"></div>
        {/if}
        <table class="pricing-table" aria-label={shownCurrency + ' / 百万 Token'}>
          <thead>
            <tr>
              <th scope="col" class="col-model">
                <button type="button" class="th-sort" class:active={sortBy === 'model'} onclick={() => toggleSort('model')}>
                  模型
                  <span class="sort-arrow">{sortBy === 'model' ? (sortDir === 'asc' ? '↑' : '↓') : '↕'}</span>
                </button>
              </th>
              <th scope="col" class="col-provider">
                <button type="button" class="th-sort" class:active={sortBy === 'provider'} onclick={() => toggleSort('provider')}>
                  厂商
                  <span class="sort-arrow">{sortBy === 'provider' ? (sortDir === 'asc' ? '↑' : '↓') : '↕'}</span>
                </button>
              </th>
              {#each columns as column}
                <th scope="col" class="col-rate tabular">
                  <button type="button" class="th-sort" class:active={sortBy === column.key} onclick={() => toggleSort(column.key)}>
                    {column.label}
                    <span class="sort-arrow">{sortBy === column.key ? (sortDir === 'asc' ? '↑' : '↓') : '↕'}</span>
                  </button>
                </th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each visible as entry (priceKey(entry))}
              {@const isManual = manualKeys.has(priceKey(entry))}
              <tr class:is-manual-row={isManual}>
                <td class="col-model">
                  <div class="model-cell-inner">
                    <span class="model-name" title={entry.model_id}>{entry.model_id}</span>
                    {#if isManual}
                      <span class="manual-badge">手动</span>
                      <div class="manual-actions">
                        <button
                          type="button"
                          class="btn-row-action"
                          disabled={saving || loading}
                          onclick={() => {
                            editing = entry;
                            formOpen = true;
                          }}
                        >
                          编辑
                        </button>
                        <button
                          type="button"
                          class="btn-row-action danger"
                          disabled={saving || loading}
                          onclick={() => removeManual(entry)}
                        >
                          移除
                        </button>
                      </div>
                    {/if}
                  </div>
                </td>
                <td class="col-provider">
                  <button
                    type="button"
                    class="provider-pill"
                    title={`筛选厂商 ${entry.provider_id}`}
                    onclick={() => {
                      provider = provider === entry.provider_id ? '' : entry.provider_id;
                      page = 1;
                    }}
                  >
                    {entry.provider_id}
                  </button>
                </td>
                {#each columns as column}
                  {@const rawRate = entry[column.key]}
                  <td
                    class="col-rate tabular"
                    class:missing={rawRate === null}
                    title={rawRate === null ? '未提供费率' : `${shownCurrency} ${displayRate(rawRate, multiplier)} / 百万 Token`}
                  >
                    {#if rawRate === null}
                      <span class="rate-empty">—</span>
                    {:else}
                      <span class="rate-sym">{currencySymbol}</span><span class="rate-val">{displayRate(rawRate, multiplier)}</span>
                    {/if}
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>

      <div class="pricing-footer">
        <span class="pricing-footer-info" role="status">
          显示 <strong>{formatNumber(offset + 1)}–{formatNumber(offset + visible.length)}</strong> / 共 <strong>{formatNumber(sorted.length)}</strong> 条报价（单位：{shownCurrency} / 百万 Token）
        </span>
        <div class="pricing-pagination" role="group" aria-label="报价分页">
          <button type="button" class="btn-secondary btn-page" disabled={currentPage <= 1} onclick={() => (page = 1)} title="第一页">首页</button>
          <button type="button" class="btn-secondary btn-page" disabled={currentPage <= 1} onclick={() => (page = currentPage - 1)}>上一页</button>
          <span class="page-indicator tabular">第 <strong>{currentPage}</strong> / {pages} 页</span>
          <button type="button" class="btn-secondary btn-page" disabled={currentPage >= pages} onclick={() => (page = currentPage + 1)}>下一页</button>
          <button type="button" class="btn-secondary btn-page" disabled={currentPage >= pages} onclick={() => (page = pages)} title="最后一页">末页</button>
        </div>
      </div>
    {:else if loading}
      <div class="table-wrap is-skeleton-table" role="status" aria-live="polite">
        <div class="session-table-progress"></div>
        <table class="pricing-table" aria-hidden="true">
          <thead>
            <tr>
              <th class="col-model">模型</th>
              <th class="col-provider">厂商</th>
              {#each columns as column}
                <th class="col-rate tabular">{column.label}</th>
              {/each}
            </tr>
          </thead>
          <tbody>
            {#each ['52%', '44%', '62%', '38%', '56%', '48%', '50%', '42%'] as w, idx}
              <tr style={`--delay: ${idx * 55}ms`}>
                <td class="col-model"><span class="sk-bone" style={`width: ${w}; height: 13px; --delay: ${idx * 55}ms`}></span></td>
                <td class="col-provider"><span class="sk-bone" style={`width: 64px; height: 20px; border-radius: 6px; --delay: ${idx * 55 + 30}ms`}></span></td>
                {#each columns as _, cIdx}
                  <td class="col-rate tabular">
                    <span class="sk-bone" style={`width: ${cIdx < 2 ? '44px' : '32px'}; height: 12px; margin-left: auto; --delay: ${idx * 55 + cIdx * 20}ms`}></span>
                  </td>
                {/each}
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    {:else if !error}
      <div class="session-empty-state">
        <strong>{entries.length ? '没有匹配的模型报价' : 'Agent 尚未下载价格目录'}</strong>
        {#if entries.length && hasFilters}
          <p>请尝试调整搜索词或清除筛选条件。</p>
          <button type="button" class="btn-secondary" onclick={clearFilters}>清除筛选</button>
        {/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .pricing-panel {
    min-width: 0;
  }

  .pricing-panel-head {
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
  }

  .pricing-panel-head p strong {
    color: var(--text-primary);
    font-weight: 600;
  }

  .pricing-head-controls {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
  }

  .currency-segment {
    display: inline-flex;
    align-items: center;
    padding: 2px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    height: 34px;
    box-sizing: border-box;
  }

  .seg-btn {
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    font-weight: 600;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .seg-btn:hover {
    color: var(--text-primary);
  }

  .seg-btn.active {
    background: var(--bg-card);
    color: var(--brand-primary);
    box-shadow: var(--shadow-card);
  }

  .rate-inline-box {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 34px;
    padding: 0 8px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    font-size: 11.5px;
    color: var(--text-secondary);
    box-sizing: border-box;
  }

  .rate-inline-box .exchange-rate {
    width: 68px;
    height: 24px;
    padding: 0 6px;
    font: inherit;
    font-variant-numeric: tabular-nums;
    color: var(--text-primary);
    background: var(--bg-card);
    border: 1px solid var(--border-input);
    border-radius: 4px;
  }

  .matching-pill {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    font-size: 12px;
    color: var(--text-secondary);
    cursor: pointer;
    user-select: none;
    box-sizing: border-box;
  }

  .matching-pill input {
    margin: 0;
    accent-color: var(--brand-strong);
  }

  .pricing-page-size {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    width: 146px;
  }

  .page-size-label {
    font-size: 11.5px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .pricing-filter-bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px;
    padding: 10px 12px;
    margin-bottom: 12px;
    background: color-mix(in srgb, var(--bg-secondary) 72%, var(--bg-card));
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
  }

  .pricing-search-wrap {
    position: relative;
    flex: 1 1 240px;
    max-width: 380px;
    min-width: 180px;
  }

  .pricing-search-wrap .search-icon {
    position: absolute;
    left: 10px;
    top: 50%;
    transform: translateY(-50%);
    width: 14px;
    height: 14px;
    color: var(--text-muted);
    pointer-events: none;
  }

  .pricing-search-wrap input {
    width: 100%;
    height: 34px;
    padding: 0 28px 0 30px;
    font: inherit;
    font-size: 12px;
    color: var(--text-primary);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    transition: border-color var(--transition-fast);
  }

  .pricing-search-wrap input:focus {
    outline: none;
    border-color: var(--brand-primary);
    background: var(--bg-card);
  }

  .search-clear {
    position: absolute;
    right: 6px;
    top: 50%;
    transform: translateY(-50%);
    width: 20px;
    height: 20px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-muted);
    font-size: 11px;
    cursor: pointer;
  }

  .search-clear:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }

  .pricing-provider-select {
    width: 180px;
    flex: 0 0 180px;
  }

  .filter-toggle-chip {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 34px;
    padding: 0 11px;
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .filter-toggle-chip:hover {
    color: var(--text-primary);
    border-color: var(--border-strong);
  }

  .filter-toggle-chip.active {
    color: var(--brand-primary);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
    font-weight: 600;
  }

  .chip-count {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--bg-secondary);
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
  }

  .btn-clear-filter {
    height: 34px;
    padding: 0 10px;
    font-size: 11.5px;
  }

  .pricing-toolbar-actions {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-left: auto;
  }

  .pricing-table {
    width: 100%;
    table-layout: fixed;
    min-width: 680px;
  }

  .pricing-table .col-model {
    width: 33%;
  }

  .pricing-table .col-provider {
    width: 13%;
  }

  .pricing-table .col-rate {
    width: 10.8%;
    text-align: right;
  }

  .pricing-table th.col-rate .th-sort {
    justify-content: flex-end;
    width: 100%;
  }

  .model-cell-inner {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
    min-width: 0;
  }

  .model-name {
    color: var(--text-primary);
    font-weight: 600;
    font-size: 12.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }

  .manual-badge {
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--brand-subtle);
    border: 1px solid var(--brand-border);
    color: var(--brand-primary);
    font-size: 10.5px;
    font-weight: 600;
  }

  .manual-actions {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-left: auto;
  }

  .btn-row-action {
    height: 22px;
    padding: 0 7px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-row-action:hover {
    color: var(--text-primary);
    border-color: var(--border-strong);
  }

  .btn-row-action.danger:hover {
    color: var(--color-danger);
    border-color: var(--color-danger-border);
    background: var(--color-danger-bg);
  }

  .provider-pill {
    display: inline-flex;
    align-items: center;
    height: 22px;
    padding: 0 8px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font-family: var(--font-mono);
    font-size: 11px;
    cursor: pointer;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    transition: all var(--transition-fast);
  }

  .provider-pill:hover {
    color: var(--brand-primary);
    border-color: var(--brand-border);
    background: var(--brand-subtle);
  }

  .rate-sym {
    color: var(--text-muted);
    font-size: 11px;
    margin-right: 1px;
  }

  .rate-val {
    color: var(--text-primary);
    font-weight: 500;
  }

  .rate-empty {
    color: var(--text-muted);
    opacity: 0.55;
  }

  .is-manual-row td {
    background: color-mix(in srgb, var(--brand-subtle) 32%, transparent);
  }

  .pricing-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 10px;
    margin-top: 14px;
    font-size: 12px;
    color: var(--text-muted);
  }

  .pricing-footer-info strong,
  .page-indicator strong {
    color: var(--text-primary);
    font-weight: 600;
  }

  .pricing-pagination {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .btn-page {
    min-height: 30px;
    padding: 0 10px;
    font-size: 11.5px;
  }

  .page-indicator {
    padding: 0 6px;
    font-size: 12px;
  }
</style>
