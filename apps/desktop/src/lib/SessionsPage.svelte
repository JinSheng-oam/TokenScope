<script lang="ts">
  import "./sessionTable.css";
  import CustomSelect from './CustomSelect.svelte';
  import SessionHistoryDialog from './SessionHistoryDialog.svelte';
  import { formatNumber, formatToolLabel, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import {
    resolveSessionActivityState,
    sortSessions,
    type SessionSortDirection,
    type SessionSortField,
  } from '../lib/dashboardView';
  import { liveNow } from '../lib/liveClock';
  import { SESSION_PAGE_SIZES } from '../lib/sessionController';
  import type {
    DashboardRange,
    SessionActivity,
    SessionAnalysisPage,
    SessionFilterOptions,
    SessionUsage,
    SessionUsagePage,
  } from '../lib/types';

  export let sessionAnalysis: SessionAnalysisPage | null;
  export let sessionPage: SessionUsagePage | undefined;
  export let sessionPageStart: number;
  export let sessionPageEnd: number;
  export let range: DashboardRange;
  export let rangeLabel: string;
  export let sessionSearch: string;
  export let sessionToolFilter: string;
  export let sessionModelFilter: string;
  export let sessionProjectFilter: string;
  export let sessionLoading: boolean;
  export let sessionError: string;
  export let sessionOffset: number;
  export let sessionPageSize: number = 50;
  export let onSetSessionPageSize: (size: number) => void = () => {};

  const pageSizeOptions = SESSION_PAGE_SIZES.map((n) => ({
    key: String(n),
    label: `${n} 条 / 页`,
  }));
  export let pendingSessionDeleteKey: string;
  export let deletingSessionKey: string;
  export let sessionActionMessage: string;
  export let sessionActionError: string;
  export let sessionFilterOptions: SessionFilterOptions;
  export let hasSessionFilters: boolean;
  export let refreshing: boolean;
  export let sessionActionKey: (session: Pick<SessionUsage, 'tool_id' | 'session_id'>) => string;
  export let onScheduleSessionAnalysis: () => void;
  export let onClearSessionFilters: () => void;
  export let onSessionPage: (nextOffset: number) => void;
  export let getSessionProjectLabel: (projectId: string) => string;
  export let onRequestSessionDelete: (session: SessionUsage) => void;
  export let onCancelSessionDelete: () => void;
  export let onConfirmSessionDelete: (session: SessionUsage) => Promise<void>;
  export let onShowAllDates: () => void;
  export let onLoadSessionAnalysis: () => Promise<void>;

  export let sessionActivity: SessionActivity[] = [];
  let historySession: SessionUsage | null = null;
  const historyTools = new Set(['codex', 'claude', 'deepseek-harness', 'antigravity', 'pi']);
  const readOnlyHistoryTools = new Set(['antigravity', 'pi']);

  let sortBy: SessionSortField = 'time';
  let sortDir: SessionSortDirection = 'desc';

  let isPaginating = false;

  $: if (!sessionLoading) {
    isPaginating = false;
  }

  function handleSessionPage(nextOffset: number) {
    isPaginating = true;
    onSessionPage(nextOffset);
  }

  const MODEL_COLLAPSE_THRESHOLD = 2;
  let expandedModelKeys = new Set<string>();

  function toggleModels(key: string) {
    const next = new Set(expandedModelKeys);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    expandedModelKeys = next;
  }

  const sortOptions: { key: SessionSortField; label: string }[] = [
    { key: 'time', label: '最近事件' },
    { key: 'status', label: '当前状态' },
    { key: 'tokens', label: 'Token' },
    { key: 'responses', label: '响应次数' },
    { key: 'records', label: '记录数' },
  ];

  function selectSort(field: SessionSortField) {
    if (sortBy === field) {
      sortDir = sortDir === 'desc' ? 'asc' : 'desc';
    } else {
      sortBy = field;
      sortDir = 'desc';
    }
  }

  function ariaSort(field: SessionSortField): 'ascending' | 'descending' | 'none' {
    return sortBy === field ? (sortDir === 'asc' ? 'ascending' : 'descending') : 'none';
  }

  $: sortedSessions = sessionPage
    ? sortSessions(sessionPage.sessions, sortBy, sortDir, sessionActivity, $liveNow)
    : [];
  $: activeCount = sortedSessions.filter(
    (s) => resolveSessionActivityState(s, sessionActivity, $liveNow) !== null,
  ).length;

  $: cleanProjectOptions = [
    { key: '', label: '全部项目' },
    ...sessionFilterOptions.projects
      .filter((o) => !o.label.startsWith('prj_') && o.label !== '项目名称未提供')
      .map((o) => ({ key: o.key, label: o.label, count: o.session_count })),
  ];

  const skeletonRowTemplates = [
    { nameW: '66%', chipW: '68px', sourceW: '78px', modelW: '64%', model2W: '52%', tokW: '72px', subW: '56px', recW: '36px', tagW: '58px', respW: '34px', timeW: '52px' },
    { nameW: '52%', chipW: '56px', sourceW: '68px', modelW: '70%', model2W: '', tokW: '64px', subW: '52px', recW: '32px', tagW: '54px', respW: '30px', timeW: '48px' },
    { nameW: '74%', chipW: '74px', sourceW: '82px', modelW: '58%', model2W: '46%', tokW: '76px', subW: '58px', recW: '40px', tagW: '62px', respW: '38px', timeW: '56px' },
    { nameW: '48%', chipW: '62px', sourceW: '72px', modelW: '62%', model2W: '', tokW: '60px', subW: '50px', recW: '30px', tagW: '54px', respW: '28px', timeW: '46px' },
    { nameW: '60%', chipW: '66px', sourceW: '78px', modelW: '68%', model2W: '48%', tokW: '68px', subW: '54px', recW: '34px', tagW: '58px', respW: '32px', timeW: '50px' },
    { nameW: '56%', chipW: '58px', sourceW: '70px', modelW: '54%', model2W: '', tokW: '62px', subW: '52px', recW: '32px', tagW: '54px', respW: '30px', timeW: '48px' },
  ];

  $: visibleSkeletonRows = skeletonRowTemplates.slice(0, Math.min(Math.max(sessionPageSize, 2), 3));
</script>

<div class="page-header">
  <h1>会话分析</h1>
</div>

<section class="panel-card">
  <div class="panel-head">
    <div>
      <h2>会话 Token 使用</h2>
      <p>
        {#if sessionPage}
          显示 {formatNumber(sessionPageStart)}–{formatNumber(sessionPageEnd)} / {formatNumber(sessionPage.total)} 个匹配会话
          {#if sessionLoading}
            <span class="session-head-sync">
              <span class="session-head-pulse" aria-hidden="true"></span>正在同步…
            </span>
          {/if}
        {:else if sessionLoading}
          <span class="session-head-status">
            <span class="session-head-pulse" aria-hidden="true"></span>正在查询会话…
          </span>
        {:else}
          当前范围：{rangeLabel}
        {/if}
      </p>
    </div>
    <div class="session-page-size">
      <span class="session-page-size-label">每页</span>
      <CustomSelect
        value={String(sessionPageSize)}
        options={pageSizeOptions}
        align="right"
        ariaLabel="设置每页会话数量"
        onChange={(val) => onSetSessionPageSize(Number(val))}
      />
    </div>
  </div>

  <div class="session-filter-panel">
    <div class="session-filter-row">
      <label class="session-filter-field session-search-field">
        <span>搜索会话</span>
        <input
          type="search"
          bind:value={sessionSearch}
          oninput={onScheduleSessionAnalysis}
          placeholder="名称、会话 ID、模型或项目"
          aria-label="搜索会话名称、会话 ID、模型或项目"
        />
      </label>
      <div class="session-filter-field">
        <span>工具</span>
        <CustomSelect
          value={sessionToolFilter}
          options={[
            { key: '', label: '全部工具' },
            ...sessionFilterOptions.tools.map((o) => ({ key: o.key, label: o.label, count: o.session_count })),
          ]}
          ariaLabel="按工具筛选会话"
          onChange={(val) => { sessionToolFilter = val; onScheduleSessionAnalysis(); }}
        />
      </div>
      <div class="session-filter-field">
        <span>模型</span>
        <CustomSelect
          value={sessionModelFilter}
          options={[
            { key: '', label: '全部模型' },
            ...sessionFilterOptions.models.map((o) => ({ key: o.key, label: o.label, count: o.session_count })),
          ]}
          ariaLabel="按模型筛选会话"
          onChange={(val) => { sessionModelFilter = val; onScheduleSessionAnalysis(); }}
        />
      </div>
      <div class="session-filter-field">
        <span>项目</span>
        <CustomSelect
          value={sessionProjectFilter}
          options={cleanProjectOptions}
          ariaLabel="按项目筛选会话"
          onChange={(val) => { sessionProjectFilter = val; onScheduleSessionAnalysis(); }}
        />
      </div>
      <button class="btn-secondary session-clear-btn" disabled={!hasSessionFilters} onclick={onClearSessionFilters}>清除筛选</button>
    </div>
    <div class="session-filter-meta">
      <span>多模型或多项目会话按“曾使用该项”匹配；Token 显示整段会话合计。</span>
      <span class="session-filter-count" role="status" aria-live="polite">
        {#if sessionLoading && !sessionPage}
          <span class="session-loading-inline">
            <span class="session-inline-spinner" aria-hidden="true"></span>
            <span>正在检索匹配的会话…</span>
          </span>
        {:else}
          {#if sessionLoading && !isPaginating}
            <span class="session-loading-inline">
              <span class="session-inline-spinner" aria-hidden="true"></span>
              <span>更新中…</span>
            </span>
          {/if}
          <span>匹配 {formatNumber(sessionPage?.total ?? 0)} 个会话</span>
        {/if}
      </span>
    </div>
  </div>

  {#if sessionActionError}
    <div class="session-action-notice error" role="alert">{sessionActionError}</div>
  {:else if sessionActionMessage}
    <div class="session-action-notice success" role="status">{sessionActionMessage}</div>
  {/if}
  {#if sessionError}
    <div class="session-action-notice error" role="alert">{sessionError}</div>
  {/if}

  {#if sessionPage && sessionPage.sessions.length > 0}
    <div class="session-sort-bar" role="group" aria-label="会话排序">
      <span class="session-sort-label">排序</span>
      {#each sortOptions as opt}
        <button
          type="button"
          class="session-sort-chip"
          class:selected={sortBy === opt.key}
          aria-pressed={sortBy === opt.key}
          onclick={() => selectSort(opt.key)}
        >
          {opt.label}
          {#if sortBy === opt.key}<span class="sort-arrow" aria-hidden="true">{sortDir === 'desc' ? '↓' : '↑'}</span>{/if}
        </button>
      {/each}
      <span class="session-sort-hint">
        {#if activeCount > 0}<span class="session-sort-live"><i></i>{activeCount} 个活跃</span>{/if}
        仅对本页 {sortedSessions.length} 条排序
      </span>
    </div>
    <div class="table-wrap" class:is-refreshing={sessionLoading} aria-busy={sessionLoading}>
      {#if sessionLoading}
        <div class="session-table-progress" aria-hidden="true"></div>
      {/if}
      <table class="session-table">
        <thead>
          <tr>
            <th class="th-session">会话</th>
            <th class="th-source">来源</th>
            <th class="th-models">模型使用</th>
            <th class="th-tokens tabular" aria-sort={ariaSort('tokens')}>
              <button type="button" class="th-sort" class:active={sortBy === 'tokens'} onclick={() => selectSort('tokens')}>Token 使用<span class="sort-arrow">{sortBy === 'tokens' ? (sortDir === 'desc' ? '↓' : '↑') : '↕'}</span></button>
            </th>
            <th class="th-precision" aria-sort={ariaSort('records')}>
              <button type="button" class="th-sort" class:active={sortBy === 'records'} onclick={() => selectSort('records')}>精度与记录<span class="sort-arrow">{sortBy === 'records' ? (sortDir === 'desc' ? '↓' : '↑') : '↕'}</span></button>
            </th>
            <th class="th-responses tabular" aria-sort={ariaSort('responses')}>
              <button type="button" class="th-sort" class:active={sortBy === 'responses'} onclick={() => selectSort('responses')}>响应次数<span class="sort-arrow">{sortBy === 'responses' ? (sortDir === 'desc' ? '↓' : '↑') : '↕'}</span></button>
            </th>
            <th class="th-time tabular" aria-sort={ariaSort('time')}>
              <button type="button" class="th-sort" class:active={sortBy === 'time'} onclick={() => selectSort('time')}>最近事件<span class="sort-arrow">{sortBy === 'time' ? (sortDir === 'desc' ? '↓' : '↑') : '↕'}</span></button>
            </th>
          </tr>
        </thead>
        <tbody>
          {#each sortedSessions as session (`${session.tool_id}:${session.session_id}`)}
            {@const actState = resolveSessionActivityState(session, sessionActivity, $liveNow)}
            {@const projectLabels = session.project_ids.map((id) => getSessionProjectLabel(id)).filter((p) => p && p.trim().length > 0 && !p.startsWith('prj_') && p !== '项目名称未提供')}
            {@const isDeletingThis = pendingSessionDeleteKey === sessionActionKey(session)}
            {@const promptTokens = (session.input_uncached_tokens ?? 0) + (session.cache_read_tokens ?? 0) + (session.cache_write_tokens ?? 0)}
            <tr class:row-deleting={isDeletingThis}>
              <td class="session-info-cell">
                <div class="session-name-row">
                  <span class="session-name" title={session.display_name ?? undefined}>
                    {session.display_name ?? '未命名会话'}
                  </span>
                  {#if actState === 'generating'}
                    <span class="recent-session-status generating session-state-pill" title="任务日志仍在更新，等待结束事件">
                      <span class="recent-session-spinner" aria-hidden="true"></span>进行中
                    </span>
                  {:else if actState === 'recent'}
                    <span class="recent-session-status recent session-state-pill" title="最近检测到会话活动">
                      <span class="recent-session-live-dot" aria-hidden="true"></span>近期活跃
                    </span>
                  {/if}
                  <button
                    class="session-inline-delete-btn"
                    aria-label={`从本机统计删除会话 ${session.display_name ?? session.session_id}`}
                    title="从本机统计移除此会话记录"
                    disabled={refreshing || deletingSessionKey !== ''}
                    onclick={() => onRequestSessionDelete(session)}
                  >
                    {@html icons.trash}
                  </button>
                </div>

                <div class="session-meta-subline">
                  <code class="session-id-pill" title={session.session_id}>…{session.session_id.slice(-8)}</code>
                  {#if historyTools.has(session.tool_id)}
                    <button
                      type="button"
                      class="session-history-button"
                      title={readOnlyHistoryTools.has(session.tool_id) ? '查看本地会话记录' : '查看或编辑本地源会话记录'}
                      onclick={() => { historySession = session; }}
                    >
                      {@html icons.edit}
                      <span>{readOnlyHistoryTools.has(session.tool_id) ? '查看历史' : '编辑历史'}</span>
                    </button>
                  {/if}
                  {#if projectLabels.length > 0}
                    {#each projectLabels.slice(0, 2) as pLabel}
                      <span class="session-facet-chip" title={pLabel}>{pLabel}</span>
                    {/each}
                    {#if projectLabels.length > 2}
                      <span class="session-facet-chip muted">+{projectLabels.length - 2}</span>
                    {/if}
                  {/if}
                </div>

                {#if isDeletingThis}
                  <div class="session-inline-confirm-box">
                    <span class="confirm-tip">仅从统计移除？</span>
                    <button
                      class="btn-danger btn-compact"
                      disabled={deletingSessionKey !== ''}
                      onclick={() => void onConfirmSessionDelete(session)}
                    >
                      {deletingSessionKey === sessionActionKey(session) ? '删除中…' : '确认'}
                    </button>
                    <button
                      class="btn-secondary btn-compact"
                      disabled={deletingSessionKey !== ''}
                      onclick={onCancelSessionDelete}
                    >
                      取消
                    </button>
                  </div>
                {/if}
              </td>

              <td class="session-source-cell"><span class="session-source-badge" title={formatToolLabel(session.tool_id)}>{formatToolLabel(session.tool_id)}</span>
              </td>

              <td class="session-models-cell">
                {#if session.models.length > 0}
                  {@const rowKey = sessionActionKey(session)}
                  {@const collapsible = session.models.length > MODEL_COLLAPSE_THRESHOLD}
                  {@const expanded = expandedModelKeys.has(rowKey)}
                  {@const visibleModels = collapsible && !expanded ? session.models.slice(0, 1) : session.models}
                  <div class="session-models-list">
                    {#each visibleModels as model}
                      <div class="session-model-row">
                        <span
                          class="session-model-name"
                          title={model.model_id ?? '来源没有提供模型标识；Token 已计入会话总用量'}
                        >
                          {model.model_id ?? '来源未提供模型'}
                        </span>
                        <span class="tabular session-model-num">{formatNumber(model.total_tokens)}</span>
                      </div>
                    {/each}
                    {#if collapsible}
                      <button
                        type="button"
                        class="session-models-toggle"
                        class:open={expanded}
                        aria-expanded={expanded}
                        onclick={() => toggleModels(rowKey)}
                      >
                        <span class="toggle-chevron" aria-hidden="true">▾</span>
                        {expanded ? '收起' : `另 ${session.models.length - 1} 个模型`}
                      </button>
                    {/if}
                  </div>
                {:else}
                  <span class="text-muted">来源未提供</span>
                {/if}
              </td>

              <td class="tabular session-token-cell">
                <div class="session-token-main">
                  <strong>{formatNumber(session.total_tokens)}</strong>
                </div>
                {#if promptTokens > 0}
                  <div
                    class="session-precision-sub session-cache-rate"
                    title={`输入 Token: ${formatNumber(promptTokens)}（读取缓存 ${formatNumber(session.cache_read_tokens ?? 0)}，写入缓存 ${formatNumber(session.cache_write_tokens ?? 0)}）`}
                  >
                    缓存率 {(((session.cache_read_tokens ?? 0) / promptTokens) * 100).toFixed(1)}%
                  </div>
                {:else}
                  <div class="session-precision-sub text-muted" title="无输入 Token 或未提供缓存记录">
                    缓存率 —
                  </div>
                {/if}
              </td>

              <td class="session-precision-cell">
                <div class="session-record-main">
                  <strong>{formatNumber(session.usage_record_count)}</strong>
                  <span class="text-muted">笔</span>
                </div>
                <div class="session-precision-tags">
                  {#if session.usage_record_count > 0 && session.exact_records === session.usage_record_count}
                    <span class="precision-tag exact">100% 精确</span>
                  {:else if session.usage_record_count > 0 && session.aggregate_records === session.usage_record_count}
                    <span class="precision-tag aggregate">全部聚合</span>
                  {:else}
                    {#if session.exact_records > 0}
                      <span class="precision-tag exact">精确 {formatNumber(session.exact_records)}</span>
                    {/if}
                    {#if session.aggregate_records > 0}
                      <span class="precision-tag aggregate">聚合 {formatNumber(session.aggregate_records)}</span>
                    {/if}
                    {#if session.estimated_records > 0}
                      <span class="precision-tag estimated">估算 {formatNumber(session.estimated_records)}</span>
                    {/if}
                  {/if}
                </div>
              </td>

              <td class="tabular session-response-cell">{#if session.known_response_count !== null}
                  <strong>{formatNumber(session.known_response_count)}</strong>
                  <div class="session-precision-sub" title={session.unknown_response_count ? `未识别响应 ${formatNumber(session.unknown_response_count)} 次` : undefined}>{session.tool_id === 'antigravity' ? '来源累计' : '当前范围'}{#if session.unknown_response_count} · +{formatNumber(session.unknown_response_count)}{/if}</div>
                {:else}
                  <span class="text-muted">来源未提供</span>
                {/if}
              </td>

              <td class="session-time-cell"><div class="session-time-main">
                  {session.last_event_at_ms === null ? '时间未知' : relativeTime(session.last_event_at_ms)}
                </div>
                {#if session.tool_id === 'antigravity' && session.activity_observed_at_ms !== null}
                  <div class="session-precision-sub" title="Antigravity 活动最近检查时间">检查 {relativeTime(session.activity_observed_at_ms)}</div>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
    <div class="session-pagination">
      <span>Antigravity 响应次数为来源累计值；Token 用量按当前日期范围汇总。</span>
      <div aria-label="会话分页">
        <button class="btn-secondary" disabled={sessionOffset === 0 || refreshing} onclick={() => handleSessionPage(Math.max(0, sessionOffset - (sessionPage?.limit ?? 50)))}>上一页</button>
        <button class="btn-secondary" disabled={!sessionPage || sessionOffset + sessionPage.limit >= sessionPage.total || refreshing} onclick={() => handleSessionPage(sessionOffset + (sessionPage?.limit ?? 50))}>下一页</button>
      </div>
    </div>
  {:else if sessionLoading && !sessionAnalysis}
    <div class="session-loading-skeleton" role="status" aria-live="polite">
      <div class="session-loading-banner">
        <div class="session-loading-banner-main">
          <span class="loading-spinner-ring" aria-hidden="true">
            <span class="loading-spinner-core"></span>
          </span>
          <div class="session-loading-banner-text">
            <strong>正在读取本机会话统计…</strong>
            <span>聚合多模型 Token、缓存命中率与响应事件</span>
          </div>
        </div>
        <span class="loading-stage-pill" aria-hidden="true">SQLite WAL 索引扫描</span>
      </div>

      <div class="session-sort-bar is-skeleton" aria-hidden="true">
        <span class="session-sort-label">排序</span>
        {#each sortOptions as opt, idx}
          <span class="session-sort-chip" class:selected={idx === 0}>
            {opt.label}{#if idx === 0}<span class="sort-arrow">↓</span>{/if}
          </span>
        {/each}
        <span class="session-sort-hint">
          <span class="sk-bone" style="width: 112px; height: 11px;"></span>
        </span>
      </div>

      <div class="table-wrap is-skeleton-table" aria-hidden="true">
        <div class="session-table-progress"></div>
        <table class="session-table">
          <thead>
            <tr>
              <th class="th-session">会话</th>
              <th class="th-source">来源</th>
              <th class="th-models">模型使用</th>
              <th class="th-tokens tabular">Token 使用</th>
              <th class="th-precision">精度与记录</th>
              <th class="th-responses tabular">响应次数</th>
              <th class="th-time tabular">最近事件</th>
            </tr>
          </thead>
          <tbody>
            {#each visibleSkeletonRows as sk, idx}
              <tr class="session-skeleton-row" style={`--delay: ${idx * 65}ms`}>
                <td class="session-info-cell">
                  <div class="session-name-row">
                    <span class="sk-bone sk-session-title" style={`width: ${sk.nameW}; --delay: ${idx * 65}ms`}></span>
                  </div>
                  <div class="session-meta-subline">
                    <span class="sk-bone sk-session-id" style={`width: 58px; --delay: ${idx * 65 + 30}ms`}></span>
                    <span class="sk-bone sk-session-chip" style={`width: ${sk.chipW}; --delay: ${idx * 65 + 50}ms`}></span>
                  </div>
                </td>
                <td class="session-source-cell">
                  <span class="sk-bone sk-source-pill" style={`width: ${sk.sourceW}; --delay: ${idx * 65 + 40}ms`}></span>
                </td>
                <td class="session-models-cell">
                  <div class="session-models-list">
                    <div class="session-model-row">
                      <span class="sk-bone sk-model-name" style={`width: ${sk.modelW}; --delay: ${idx * 65 + 60}ms`}></span>
                      <span class="sk-bone sk-model-num" style={`width: 48px; --delay: ${idx * 65 + 80}ms`}></span>
                    </div>
                    {#if sk.model2W}
                      <div class="session-model-row">
                        <span class="sk-bone sk-model-name sub" style={`width: ${sk.model2W}; --delay: ${idx * 65 + 90}ms`}></span>
                        <span class="sk-bone sk-model-num sub" style={`width: 38px; --delay: ${idx * 65 + 110}ms`}></span>
                      </div>
                    {/if}
                  </div>
                </td>
                <td class="tabular session-token-cell">
                  <div class="session-sk-right">
                    <span class="sk-bone sk-token-num" style={`width: ${sk.tokW}; --delay: ${idx * 65 + 70}ms`}></span>
                    <span class="sk-bone sk-sub-note" style={`width: ${sk.subW}; --delay: ${idx * 65 + 95}ms`}></span>
                  </div>
                </td>
                <td class="session-precision-cell">
                  <div class="session-sk-left">
                    <span class="sk-bone sk-rec-num" style={`width: ${sk.recW}; --delay: ${idx * 65 + 85}ms`}></span>
                    <span class="sk-bone sk-precision-pill" style={`width: ${sk.tagW}; --delay: ${idx * 65 + 105}ms`}></span>
                  </div>
                </td>
                <td class="tabular session-response-cell">
                  <div class="session-sk-right">
                    <span class="sk-bone sk-resp-num" style={`width: ${sk.respW}; --delay: ${idx * 65 + 100}ms`}></span>
                    <span class="sk-bone sk-sub-note" style={`width: 48px; --delay: ${idx * 65 + 120}ms`}></span>
                  </div>
                </td>
                <td class="session-time-cell">
                  <div class="session-sk-right">
                    <span class="sk-bone sk-time-val" style={`width: ${sk.timeW}; --delay: ${idx * 65 + 115}ms`}></span>
                  </div>
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      </div>
    </div>
  {:else if sessionPage && sessionPage.total === 0 && hasSessionFilters}
    <div class="session-empty-state">
      <strong>没有匹配的会话</strong>
      <p>调整或清除筛选条件。</p>
      <button class="btn-secondary" onclick={onClearSessionFilters}>清除筛选</button>
    </div>
  {:else if sessionPage}
    <div class="session-empty-state">
      <strong>这个时间范围内没有会话用量</strong>
      <p>切换到“全部”查看历史会话。</p>
      <button class="btn-secondary" disabled={range === 'all'} onclick={onShowAllDates}>查看全部日期</button>
    </div>
  {:else if sessionError}
    <div class="session-empty-state">
      <strong>会话数据暂时无法读取</strong>
      <button class="btn-secondary" disabled={sessionLoading} onclick={() => void onLoadSessionAnalysis()}>重试</button>
    </div>
  {:else}
    <div class="session-empty-state" role="status">选择“会话分析”后会读取当前日期范围的会话。</div>
  {/if}
</section>

{#if historySession}
  <SessionHistoryDialog
    session={historySession}
    isGenerating={resolveSessionActivityState(historySession, sessionActivity, $liveNow) === 'generating'}
    onClose={() => { historySession = null; }}
  />
{/if}
