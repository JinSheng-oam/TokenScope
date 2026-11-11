<script lang="ts">
  import './sessionTimeline.css';
  import { tick } from 'svelte';
  import SessionDetailsDrawer from './SessionDetailsDrawer.svelte';
  import { groupSessionTimeline, sessionCacheRate, sessionEventTime, sessionProjectLabels } from './sessionTimeline';
  import CustomSelect from './CustomSelect.svelte';
  import SessionHistoryDialog from './SessionHistoryDialog.svelte';
  import { formatNumber, formatToolLabel, percentOf, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import {
    resolveSessionActivityState,
    sortSessions,
    type SessionSortDirection,
    type SessionSortField,
  } from '../lib/dashboardView';
  import { sessionRateLabel, sessionRateTitle, type SessionRates } from './sessionRates';
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
  export let timeZone: string;
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
  export let sessionRates: SessionRates | null = null;
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

  let selectedKey: string | null = null;
  let detailsTrigger: HTMLButtonElement | null = null;
  let searchInput: HTMLInputElement;

  function openDetails(session: SessionUsage, event: MouseEvent) {
    detailsTrigger = event.currentTarget as HTMLButtonElement;
    selectedKey = sessionActionKey(session);
  }

  async function closeDetails() {
    if (pendingSessionDeleteKey && !deletingSessionKey) onCancelSessionDelete();
    selectedKey = null;
    await tick();
    if (detailsTrigger?.isConnected) detailsTrigger.focus();
    else searchInput?.focus();
    detailsTrigger = null;
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

  $: sortClock = sortBy === 'status' ? $liveNow : 0;
  $: groupClock = Math.floor($liveNow / 60_000) * 60_000;
  $: sortedSessions = sessionPage
    ? sortSessions(sessionPage.sessions, sortBy, sortDir, sessionActivity, sortClock)
    : [];
  $: groups = groupSessionTimeline(sortedSessions, sortBy === 'time', timeZone, groupClock);
  $: largestTokens = sortedSessions.reduce((max, session) => Math.max(max, session.total_tokens), 0);
  $: detailSession = selectedKey === null ? null
    : sessionPage?.sessions.find((session) => sessionActionKey(session) === selectedKey) ?? null;
  $: if (selectedKey && !detailSession && !sessionLoading) void closeDetails();
  $: cleanProjectOptions = [
    { key: '', label: '全部项目' },
    ...sessionFilterOptions.projects
      .filter((o) => !o.label.startsWith('prj_') && o.label !== '项目名称未提供')
      .map((o) => ({ key: o.key, label: o.label, count: o.session_count })),
  ];

</script>

<section class="session-workspace" aria-label="会话分析">
  <div class="page-header"><h1>会话分析</h1></div>

  <div class="session-filter-panel">
    <div class="session-filter-row">
      <label class="session-filter-field session-search-field">
        <span class="session-search-icon" aria-hidden="true">{@html icons.search}</span>
        <input
          type="search"
          bind:this={searchInput}
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
  </div>

  {#if sessionActionError && !detailSession}
    <div class="session-action-notice error" role="alert">{sessionActionError}</div>
  {:else if sessionActionMessage}
    <div class="session-action-notice success" role="status">{sessionActionMessage}</div>
  {/if}
  {#if sessionError}
    <div class="session-action-notice error" role="alert">{sessionError}</div>
  {/if}

  {#if sessionPage && sessionPage.sessions.length > 0}
    <div class="session-sort-bar" role="group" aria-label="会话排序，仅对本页生效">
      <span class="session-sort-label" title="仅对本页排序">排序：</span>
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
    </div>
    <div class="session-timeline" aria-busy={sessionLoading}>
      {#each groups as group (group.key)}
        <div class="session-day" class:dated={!!group.label}>
          {#if group.label}<h2><span>{group.label}</span></h2>{/if}
          <ul>
            {#each group.sessions as session (sessionActionKey(session))}
              {@const projects = sessionProjectLabels(session, getSessionProjectLabel)}
              {@const state = resolveSessionActivityState(session, sessionActivity, $liveNow)}
              {@const rate = sessionRateLabel(sessionRates, session, $liveNow)}
              {@const eventTime = sessionEventTime(session)}
              <li>
                <button type="button" class="session-entry" class:chosen={selectedKey === sessionActionKey(session)}
                  aria-haspopup="dialog" onclick={(event) => openDetails(session, event)}>
                  <span class="session-entry-main">
                    <span class="session-entry-title">
                      <strong title={session.display_name ?? undefined}>{session.display_name ?? '未命名会话'}</strong>
                      {#if state}<span class="session-activity">{state === 'generating' ? '进行中' : '近期活跃'}</span>{/if}
                    </span>
                    <span class="session-entry-meta">
                      {#if projects[0]}<span title={projects.join(' · ')}>{@html icons.projects}{projects[0]}</span>{/if}
                      <span>{@html icons.tools}{formatToolLabel(session.tool_id)}</span>
                      {#if session.models[0]?.model_id}
                        <span title={session.models[0].model_id}>{@html icons.models}{session.models[0].model_id}</span>
                      {/if}
                      {#if session.models.length > 1}<span>+{session.models.length - 1}</span>{/if}
                    </span>
                    <span class="session-entry-usage">
                      <span class="session-usage-track" title="相对本页最高 Token 用量" aria-hidden="true">
                        <span style:width={percentOf(session.total_tokens, largestTokens) + '%'}></span>
                      </span>
                      <span>缓存率 {sessionCacheRate(session)}</span>
                    </span>
                  </span>
                  <span class="session-entry-metric">
                    <span>Token 用量</span>
                    <span class="session-token-value">
                      <strong>{formatNumber(session.total_tokens)}</strong>
                      {#if session.usage_record_count > 0 && session.exact_records === session.usage_record_count}
                        <small class="session-entry-precision">精确</small>
                      {/if}
                    </span>
                    {#if rate}<small title={sessionRateTitle}>{rate}</small>{/if}
                  </span>
                  <span class="session-entry-metric session-entry-responses">
                    <span>{session.tool_id === 'antigravity' ? '响应 · 来源累计' : '响应次数'}</span>
                    <strong>{session.known_response_count === null ? '—' : formatNumber(session.known_response_count)}</strong>
                    {#if sortBy !== 'time'}
                      <small>{eventTime === null ? '时间未知' : relativeTime(eventTime, $liveNow)}</small>
                    {/if}
                  </span>
                  <span class="session-entry-arrow" aria-hidden="true">{@html icons.chevronRight}</span>
                </button>
              </li>
            {/each}
          </ul>
        </div>
      {/each}
    </div>
    <div class="session-pagination">
      <span role="status">{sessionLoading ? '更新中…' : formatNumber(sessionPageStart) + '–' + formatNumber(sessionPageEnd) + ' / ' + formatNumber(sessionPage.total) + ' 条'}</span>
      <div class="session-pagination-controls">
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
        <div aria-label="会话分页">
          <button class="btn-secondary" disabled={sessionOffset === 0 || refreshing || isPaginating} onclick={() => handleSessionPage(Math.max(0, sessionOffset - sessionPage.limit))}>上一页</button>
          <button class="btn-secondary" disabled={sessionOffset + sessionPage.limit >= sessionPage.total || refreshing || isPaginating} onclick={() => handleSessionPage(sessionOffset + sessionPage.limit)}>下一页</button>
        </div>
      </div>
    </div>
  {:else if sessionLoading && !sessionAnalysis}
    <div class="session-empty-state" role="status">正在读取会话统计…</div>
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

{#if detailSession}
  <SessionDetailsDrawer
    session={detailSession}
    projects={sessionProjectLabels(detailSession, getSessionProjectLabel)}
    {rangeLabel}
    historyLabel={!historyTools.has(detailSession.tool_id) ? null : readOnlyHistoryTools.has(detailSession.tool_id) ? '查看历史' : '编辑历史'}
    actionError={sessionActionError}
    confirming={pendingSessionDeleteKey === selectedKey}
    deleting={deletingSessionKey === selectedKey}
    deleteDisabled={refreshing || deletingSessionKey !== ''}
    onClose={() => void closeDetails()}
    onHistory={() => { historySession = detailSession; }}
    onRequestDelete={() => onRequestSessionDelete(detailSession)}
    onCancelDelete={onCancelSessionDelete}
    onConfirmDelete={() => void onConfirmSessionDelete(detailSession)}
  />
{/if}

{#if historySession}
  <SessionHistoryDialog
    session={historySession}
    isGenerating={resolveSessionActivityState(historySession, sessionActivity, $liveNow) === 'generating'}
    onClose={() => { historySession = null; }}
  />
{/if}
