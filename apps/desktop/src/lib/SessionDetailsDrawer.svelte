<script lang="ts">
  import { onMount } from 'svelte';
  import { formatNumber, formatToolLabel, percentOf, relativeTime } from './format';
  import { icons } from './icons';
  import { sessionCacheRate } from './sessionTimeline';
  import type { SessionUsage } from './types';

  export let session: SessionUsage;
  export let projects: string[];
  export let rangeLabel: string;
  export let historyLabel: string | null;
  export let actionError: string;
  export let confirming: boolean;
  export let deleting: boolean;
  export let deleteDisabled: boolean;
  export let onClose: () => void;
  export let onHistory: () => void;
  export let onRequestDelete: () => void;
  export let onCancelDelete: () => void;
  export let onConfirmDelete: () => void;

  let dialog: HTMLDialogElement;
  let closeButton: HTMLButtonElement;
  let deleteButton: HTMLButtonElement;
  let backdropPressed = false;
  onMount(() => {
    const element = dialog;
    element.showModal();
    closeButton.focus();
    return () => { if (element.open) element.close(); };
  });

  $: precision = [
    { kind: 'exact', label: '精确', count: session.exact_records },
    { kind: 'aggregate', label: '聚合', count: session.aggregate_records },
    { kind: 'estimated', label: '估算', count: session.estimated_records },
    { kind: 'unknown', label: '未知', count: session.unknown_precision_records },
  ].filter(({ count }) => count > 0);
</script>

<dialog
  class="session-drawer"
  bind:this={dialog}
  aria-labelledby="session-details-heading"
  oncancel={(event) => { event.preventDefault(); onClose(); }}
  onpointerdown={(event) => { backdropPressed = event.target === dialog; }}
  onclick={(event) => { if (backdropPressed && event.target === dialog) onClose(); }}
>
  <div class="session-drawer-inner">
    <header class="session-drawer-header">
      <div>
        <h2 id="session-details-heading">{session.display_name ?? '未命名会话'}</h2>
        <p>{[...projects, formatToolLabel(session.tool_id)].join(' · ')}</p>
        <code title={session.session_id}>…{session.session_id.slice(-8)}</code>
      </div>
      <button type="button" class="session-icon-button" bind:this={closeButton} onclick={onClose} aria-label="关闭会话详情">
        {@html icons.close}
      </button>
    </header>

    <div class="session-drawer-body">
      {#if actionError}<p class="session-action-notice error" role="alert">{actionError}</p>{/if}
      <div class="session-drawer-total">
        <span>Token 用量 · {rangeLabel}</span>
        <strong>{formatNumber(session.total_tokens)}</strong>
      </div>
      <dl class="session-drawer-metrics">
        <div>
          <dt>响应次数{session.tool_id === 'antigravity' ? ' · 来源累计' : ''}</dt>
          <dd>{session.known_response_count === null ? '—' : formatNumber(session.known_response_count)}</dd>
          {#if session.unknown_response_count}<small>另有 {formatNumber(session.unknown_response_count)} 次未识别</small>{/if}
        </div>
        <div><dt>用量记录</dt><dd>{formatNumber(session.usage_record_count)} 笔</dd></div>
        <div><dt>缓存率</dt><dd>{sessionCacheRate(session)}</dd></div>
        <div>
          <dt>记录精度</dt>
          <dd class="session-precision-list">
            {#each precision as item (item.kind)}
              <span class="session-precision {item.kind}">{item.label} {formatNumber(item.count)}</span>
            {:else}—{/each}
          </dd>
        </div>
      </dl>

      <section class="session-drawer-models" aria-label="模型用量">
        <h3>模型用量</h3>
        {#each session.models as model (model.model_id)}
          {@const share = percentOf(model.total_tokens, session.total_tokens)}
          <div class="session-model-detail">
            <span>{model.model_id ?? '来源未提供模型'}</span>
            <strong>{formatNumber(model.total_tokens)}</strong>
            <small>{session.total_tokens > 0 ? share.toFixed(1) + '%' : '—'}</small>
            <div class="session-usage-track" aria-hidden="true"><span style:width={share + '%'}></span></div>
          </div>
        {:else}
          <p>来源未提供模型用量。</p>
        {/each}
      </section>

      <dl class="session-drawer-events">
        <div><dt>最近事件</dt><dd>{session.last_event_at_ms === null ? '时间未知' : relativeTime(session.last_event_at_ms)}</dd></div>
        {#if session.tool_id === 'antigravity' && session.activity_observed_at_ms !== null}
          <div><dt>最近检查</dt><dd>{relativeTime(session.activity_observed_at_ms)}</dd></div>
        {/if}
        {#if session.untimed_records > 0}
          <div><dt>时间未知的记录</dt><dd>{formatNumber(session.untimed_records)} 笔</dd></div>
        {/if}
      </dl>
    </div>

    <footer class="session-drawer-footer">
      {#if confirming}
        <span>仅从本机统计移除？</span>
        <button type="button" class="btn-secondary" disabled={deleting} onclick={() => { onCancelDelete(); requestAnimationFrame(() => deleteButton?.focus()); }}>取消</button>
        <button type="button" class="btn-danger" disabled={deleting} onclick={onConfirmDelete}>{deleting ? '移除中…' : '确认移除'}</button>
      {:else}
        <button type="button" class="btn-secondary session-remove-button" bind:this={deleteButton} disabled={deleteDisabled} onclick={() => { onRequestDelete(); closeButton.focus(); }}>从统计移除</button>
        {#if historyLabel}
          <button type="button" class="btn-secondary session-history-action" onclick={onHistory}>
            {@html icons.quota} {historyLabel}
          </button>
        {/if}
      {/if}
    </footer>
  </div>
</dialog>
