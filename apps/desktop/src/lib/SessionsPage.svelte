<script lang="ts">
import { formatNumber, formatToolLabel, relativeTime } from '../lib/format';
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
                <label class="session-filter-field">
                  <span>工具</span>
                  <select bind:value={sessionToolFilter} onchange={onScheduleSessionAnalysis} aria-label="按工具筛选会话">
                    <option value="">全部工具</option>
                    {#each sessionFilterOptions.tools as option}
                      <option value={option.key}>{option.label} · {formatNumber(option.session_count)}</option>
                    {/each}
                  </select>
                </label>
                <label class="session-filter-field">
                  <span>模型</span>
                  <select bind:value={sessionModelFilter} onchange={onScheduleSessionAnalysis} aria-label="按模型筛选会话">
                    <option value="">全部模型</option>
                    {#each sessionFilterOptions.models as option}
                      <option value={option.key}>{option.label} · {formatNumber(option.session_count)}</option>
                    {/each}
                  </select>
                </label>
                <label class="session-filter-field">
                  <span>项目</span>
                  <select bind:value={sessionProjectFilter} onchange={onScheduleSessionAnalysis} aria-label="按项目筛选会话">
                    <option value="">全部项目</option>
                    {#each sessionFilterOptions.projects as option}
                      <option value={option.key}>{option.label} · {formatNumber(option.session_count)}</option>
                    {/each}
                  </select>
                </label>
                <button class="btn-secondary session-clear-btn" disabled={!hasSessionFilters} onclick={onClearSessionFilters}>清除筛选</button>
              </div>
              <div class="session-filter-meta">
                <span>多模型或多项目会话按“曾使用该项”匹配；Token 显示整段会话合计。</span>
                <span class="session-filter-count" role="status">{sessionLoading ? '正在更新结果…' : `匹配 ${formatNumber(sessionPage?.total ?? 0)} 个会话`}</span>
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
                      <th>会话</th><th>来源</th><th>模型使用</th><th>Token 使用</th><th>精度与记录</th><th>响应次数</th><th>最近事件</th>
                    </tr>
                  </thead>
                  <tbody>
                    {#each sessionPage.sessions as session}
                      <tr>
                        <td>
                          <div class="session-name" title={session.display_name ?? undefined}>{session.display_name ?? '未命名会话'}</div>
                          <code title={session.session_id}>…{session.session_id.slice(-12)}</code>
                          <div class="session-facet-list" aria-label="项目分类">
                            {#if session.project_ids.length > 0}
                              {#each session.project_ids.slice(0, 2) as projectId}
                                <span class="session-facet-chip" title={getSessionProjectLabel(projectId)}>{getSessionProjectLabel(projectId)}</span>
                              {/each}
                              {#if session.project_ids.length > 2}<span class="session-facet-chip muted">+{session.project_ids.length - 2}</span>{/if}
                            {:else}
                              <span class="session-facet-chip muted">未关联项目</span>
                            {/if}
                          </div>
                          <div class="session-row-actions">
                            {#if pendingSessionDeleteKey === sessionActionKey(session)}
                              <div class="session-action-confirm">
                                <span>仅影响本机统计</span>
                                <button
                                  class="btn-danger session-action-btn"
                                  disabled={deletingSessionKey !== ''}
                                  onclick={() => void onConfirmSessionDelete(session)}
                                >
                                  {deletingSessionKey === sessionActionKey(session) ? '删除中…' : '确认删除'}
                                </button>
                                <button
                                  class="btn-secondary session-action-btn"
                                  disabled={deletingSessionKey !== ''}
                                  onclick={onCancelSessionDelete}
                                >取消</button>
                              </div>
                            {:else}
                              <button
                                class="btn-danger session-delete-btn"
                                aria-label={`从本机统计删除会话 ${session.display_name ?? session.session_id}`}
                                title="只删除 TokenScope 本机统计，不删除原应用聊天"
                                disabled={refreshing || deletingSessionKey !== ''}
                                onclick={() => onRequestSessionDelete(session)}
                              >删除</button>
                            {/if}
                          </div>
                        </td>
                        <td>{formatToolLabel(session.tool_id)}</td>
                        <td class="session-models">
                          {#if session.models.length > 0}
                            {#each session.models.slice(0, 4) as model}
                              <div class="session-model-row">
                                <span class="session-model-name" title={model.model_id ?? '未识别模型'}>{model.model_id ?? '未识别模型'}</span>
                                <span class="tabular">{formatNumber(model.total_tokens)}</span>
                              </div>
                            {/each}
                            {#if session.models.length > 4}
                              <div class="session-precision">另有 {session.models.length - 4} 个模型</div>
                            {/if}
                          {:else}
                            <span class="text-muted">来源未提供</span>
                          {/if}
                        </td>
                        <td class="tabular"><strong>{formatNumber(session.total_tokens)}</strong></td>
                        <td>
                          <span>{formatNumber(session.usage_record_count)} 笔记录</span>
                          <div class="session-precision">
                            精确 {formatNumber(session.exact_records)} · 聚合 {formatNumber(session.aggregate_records)} · 估算 {formatNumber(session.estimated_records)}
                            {#if session.unknown_precision_records > 0} · 未知 {formatNumber(session.unknown_precision_records)}{/if}
                            {#if session.untimed_records > 0} · 无时间 {formatNumber(session.untimed_records)}{/if}
                          </div>
                        </td>
                        <td class="tabular">
                          {#if session.known_response_count !== null}
                            {formatNumber(session.known_response_count)}
                            <div class="session-precision">{session.tool_id === 'antigravity' ? '来源累计' : '当前范围'}{#if session.unknown_response_count} · 未识别 {formatNumber(session.unknown_response_count)}{/if}</div>
                          {:else}
                            <span class="text-muted">来源未提供</span>
                          {/if}
                        </td>
                        <td>
                          {session.last_event_at_ms === null ? '事件时间未知' : relativeTime(session.last_event_at_ms)}
                          {#if session.tool_id === 'antigravity' && session.activity_observed_at_ms !== null}
                            <div class="session-precision">来源检查 {relativeTime(session.activity_observed_at_ms)}</div>
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
