<script lang="ts">
  import CustomSelect from './CustomSelect.svelte';
  import { formatNumber, formatToolLabel, relativeTime } from '../lib/format';
  import { icons } from '../lib/icons';
  import type {
    DashboardRange,
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
  export let onSelectRange: (range: DashboardRange) => void;
  export let onLoadSessionAnalysis: () => Promise<void>;

  $: cleanProjectOptions = [
    { key: '', label: '全部项目' },
    ...sessionFilterOptions.projects
      .filter((o) => !o.label.startsWith('prj_') && o.label !== '项目名称未提供')
      .map((o) => ({ key: o.key, label: o.label, count: o.session_count })),
  ];
</script>

<div class="page-header">
  <h1>会话分析</h1>
  <p>按工具、模型和项目筛选；可搜索会话名称、ID、模型或项目标识。仅查询本机统计，不读取聊天正文。</p>
</div>

<section class="panel-card">
  <div class="panel-head">
    <div>
      <h2>会话 Token 使用</h2>
      <p>{#if sessionPage}显示 {formatNumber(sessionPageStart)}–{formatNumber(sessionPageEnd)} / {formatNumber(sessionPage.total)} 个匹配会话{:else if sessionLoading}正在查询会话…{:else}当前范围：{rangeLabel}{/if}</p>
    </div>
    <span class="panel-tag">每页 {sessionPage?.limit ?? 50}</span>
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
      <span class="session-filter-count" role="status">
        {#if sessionLoading}
          <span class="session-loading-inline">更新中…</span>
        {/if}
        匹配 {formatNumber(sessionPage?.total ?? 0)} 个会话
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
    <div class="table-wrap">
      <table class="session-table">
        <thead>
          <tr>
            <th class="th-session">会话</th>
            <th class="th-source">来源</th>
            <th class="th-models">模型使用</th>
            <th class="th-tokens tabular">Token 使用</th>
            <th class="th-precision">精度与记录</th>
            <th class="th-responses tabular">响应次数</th>
            <th class="th-time">最近事件</th>
          </tr>
        </thead>
        <tbody>
          {#each sessionPage.sessions as session}
            {@const projectLabels = session.project_ids.map((id) => getSessionProjectLabel(id)).filter((p) => p && p.trim().length > 0)}
            {@const isDeletingThis = pendingSessionDeleteKey === sessionActionKey(session)}
            <tr class:row-deleting={isDeletingThis}>
              <td class="session-info-cell">
                <div class="session-name-row">
                  <span class="session-name" title={session.display_name ?? undefined}>
                    {session.display_name ?? '未命名会话'}
                  </span>
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

              <td class="session-source-cell"><span class="session-source-badge">{formatToolLabel(session.tool_id)}</span>
              </td>

              <td class="session-models-cell">
                {#if session.models.length > 0}
                  <div class="session-models-list">
                    {#each session.models.slice(0, 4) as model}
                      <div class="session-model-row">
                        <span class="session-model-name" title={model.model_id ?? '未识别模型'}>
                          {model.model_id ?? '未识别模型'}
                        </span>
                        <span class="tabular session-model-num">{formatNumber(model.total_tokens)}</span>
                      </div>
                    {/each}
                    {#if session.models.length > 4}
                      <div class="session-precision-sub">另有 {session.models.length - 4} 个模型</div>
                    {/if}
                  </div>
                {:else}
                  <span class="text-muted">来源未提供</span>
                {/if}
              </td>

              <td class="tabular session-token-cell">
                <strong>{formatNumber(session.total_tokens)}</strong>
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
                  <div class="session-precision-sub">{session.tool_id === 'antigravity' ? '来源累计' : '当前范围'}{#if session.unknown_response_count} · 未识别 {formatNumber(session.unknown_response_count)}{/if}</div>
                {:else}
                  <span class="text-muted">来源未提供</span>
                {/if}
              </td>

              <td class="session-time-cell"><div class="session-time-main">
                  {session.last_event_at_ms === null ? '时间未知' : relativeTime(session.last_event_at_ms)}
                </div>
                {#if session.tool_id === 'antigravity' && session.activity_observed_at_ms !== null}
                  <div class="session-precision-sub">来源检查 {relativeTime(session.activity_observed_at_ms)}</div>
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
        <button class="btn-secondary" disabled={sessionOffset === 0 || refreshing} onclick={() => onSessionPage(Math.max(0, sessionOffset - (sessionPage?.limit ?? 50)))}>上一页</button>
        <button class="btn-secondary" disabled={!sessionPage || sessionOffset + sessionPage.limit >= sessionPage.total || refreshing} onclick={() => onSessionPage(sessionOffset + (sessionPage?.limit ?? 50))}>下一页</button>
      </div>
    </div>
  {:else if sessionLoading && !sessionAnalysis}
    <div class="session-empty-state" role="status">正在读取本机会话统计…</div>
  {:else if sessionPage && sessionPage.total === 0 && hasSessionFilters}
    <div class="session-empty-state">
      <strong>没有匹配的会话</strong>
      <p>调整搜索内容或分类条件，或清除筛选查看当前范围的全部会话。</p>
      <button class="btn-secondary" onclick={onClearSessionFilters}>清除筛选</button>
    </div>
  {:else if sessionPage}
    <div class="session-empty-state">
      <strong>这个时间范围内没有会话用量</strong>
      <p>切换到“全部”可查看历史记录，以及来源未提供事件时间的用量。</p>
      <button class="btn-secondary" disabled={range === 'all'} onclick={() => onSelectRange('all')}>查看全部日期</button>
    </div>
  {:else if sessionError}
    <div class="session-empty-state">
      <strong>会话数据暂时无法读取</strong>
      <p>请重试查询；本机统计数据不会因此被更改。</p>
      <button class="btn-secondary" disabled={sessionLoading} onclick={() => void onLoadSessionAnalysis()}>重试</button>
    </div>
  {:else}
    <div class="session-empty-state" role="status">选择“会话分析”后会读取当前日期范围的会话。</div>
  {/if}
</section>
