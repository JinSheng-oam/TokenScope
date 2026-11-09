<script lang="ts">
  import { invoke } from '@tauri-apps/api/core';
  import { onMount, tick } from 'svelte';
  import { formatToolLabel } from './format';
  import { icons } from './icons';
  import type { SessionUsage } from './types';

  type Message = { line: number; pointer: string; role: 'user' | 'assistant'; text: string };
  type History = {
    revision: string;
    messages: Message[];
    total: number;
    writable: boolean;
    partCount: number;
    partIndex: number;
  };

  export let session: SessionUsage;
  export let onClose: () => void;
  export let isGenerating: boolean;

  const pageSize = 30;
  let dialog: HTMLDialogElement;
  let viewport: HTMLDivElement | undefined;
  let history: History | null = null;
  let selectedKey: string | null = null;
  let removingKey: string | 'session' | null = null;
  let backupPath = '';
  let draft = '';
  let offset = 0;
  let pageInput = '1';
  let part: number | undefined;
  let loading = true;
  let saving = false;
  let error = '';
  let notice = '';

  $: args = { toolId: session.tool_id, sessionId: session.session_id };
  $: sourceReadOnly = ['antigravity', 'pi'].includes(session.tool_id);
  $: readOnly = sourceReadOnly || isGenerating || (history !== null && !history.writable);
  $: busy = loading || saving;
  $: hasActiveAction = selectedKey !== null || removingKey !== null;
  $: totalPages = Math.max(1, Math.ceil((history?.total ?? 0) / pageSize));
  $: currentPage = Math.floor(offset / pageSize) + 1;

  function msgKey(m: Message): string {
    return `${m.line}:${m.pointer}`;
  }

  function lastPageOffset(total: number): number {
    return Math.floor(Math.max(0, total - 1) / pageSize) * pageSize;
  }

  onMount(() => {
    dialog.showModal();
    void load('last');
  });

  async function load(target: number | 'last', nextPart = part) {
    loading = true;
    error = '';
    notice = '';
    try {
      const initialOffset = target === 'last' ? 0 : target;
      let nextHistory = await invoke<History>('get_session_history', {
        ...args,
        offset: initialOffset,
        part: nextPart,
      });
      let resolvedOffset = initialOffset;

      if (target === 'last' && nextHistory.total > pageSize) {
        resolvedOffset = lastPageOffset(nextHistory.total);
        if (resolvedOffset !== 0) {
          nextHistory = await invoke<History>('get_session_history', {
            ...args,
            offset: resolvedOffset,
            part: nextHistory.partIndex,
          });
        }
      }

      history = nextHistory;
      part = nextHistory.partIndex;
      offset = resolvedOffset;
      pageInput = String(Math.floor(resolvedOffset / pageSize) + 1);
      selectedKey = null;
      removingKey = null;

      await tick();
      if (viewport) {
        viewport.scrollTop = target === 'last' ? viewport.scrollHeight : 0;
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  function commitPageJump() {
    if (!history || busy || hasActiveAction) return;
    const parsed = Number.parseInt(String(pageInput).trim(), 10);
    if (!Number.isFinite(parsed)) {
      pageInput = String(currentPage);
      return;
    }
    const clampedPage = Math.min(totalPages, Math.max(1, parsed));
    pageInput = String(clampedPage);
    const nextOffset = (clampedPage - 1) * pageSize;
    if (nextOffset !== offset) {
      void load(nextOffset);
    }
  }

  async function save(message: Message, text: string | null) {
    if (!history?.writable || (text !== null && !text.trim()) || saving || isGenerating) return;
    saving = true;
    error = '';
    notice = '';
    try {
      history = await invoke<History>('save_session_message', {
        ...args,
        offset,
        part,
        expectedRevision: history.revision,
        edit: { line: message.line, pointer: message.pointer, text, delete: text === null },
      });
      offset = Math.min(offset, lastPageOffset(history.total));
      pageInput = String(Math.floor(offset / pageSize) + 1);
      selectedKey = null;
      removingKey = null;
      notice = text === null ? '已删除该条消息正文，源文件已自动备份（.bak）。' : '已写回本地会话文件，源文件已自动备份（.bak）。';
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function removeSession() {
    if (!history?.writable || history.partCount !== 1 || removingKey !== 'session' || saving || isGenerating) return;
    saving = true;
    error = '';
    notice = '';
    try {
      backupPath = await invoke<string>('remove_source_session', {
        ...args,
        expectedRevision: history.revision,
      });
      history = null;
      removingKey = null;
      notice = '已移走整个会话的本地源文件。统计数据将在下次重扫时更新；原 Agent 若仍显示旧条目，重启后即可生效。';
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  function handleDialogCancel(event: Event) {
    if (saving) {
      event.preventDefault();
      return;
    }
    if (selectedKey !== null || removingKey !== null) {
      event.preventDefault();
      selectedKey = null;
      removingKey = null;
      error = '';
    }
  }

  function handleTextareaKeydown(event: KeyboardEvent, message: Message) {
    if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') {
      event.preventDefault();
      if (!saving && !isGenerating && draft.trim() && draft !== message.text) {
        void save(message, draft);
      }
    } else if (event.key === 'Escape' && !saving) {
      event.preventDefault();
      event.stopPropagation();
      selectedKey = null;
      error = '';
    }
  }
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="session-history-title"
  onclose={onClose}
  oncancel={handleDialogCancel}
>
  <header class="dialog-header">
    <div class="header-meta">
      <div class="header-title-row">
        <h2 id="session-history-title">{sourceReadOnly ? '查看历史会话' : '编辑历史会话'}</h2>
        <span class="session-source-badge">{formatToolLabel(session.tool_id)}</span>
        {#if readOnly}
          <span class="status-pill readonly">{@html icons.lock}只读模式</span>
        {/if}
      </div>
      <p class="header-subtitle" title={session.display_name ?? session.session_id}>
        {session.display_name ?? '未命名会话'}
        <code class="session-id-code">…{session.session_id.slice(-8)}</code>
      </p>
    </div>
    <button type="button" class="btn-secondary btn-compact" disabled={saving} onclick={() => dialog.close()}>
      关闭
    </button>
  </header>

  <div class="dialog-banners">
    <div class="banner banner-info">
      <span class="banner-icon" aria-hidden="true">{@html icons.info}</span>
      {#if sourceReadOnly}
        <span>这里只读取本地日志里的用户消息和助手回复。编辑或删除请在原 Agent 中操作。</span>
      {:else}
        <span>请先在原 Agent 中关闭此会话。修改将写回本地记录并在同目录保留 <code>.bak</code> 备份；已入库的 Token 用量不会重算。</span>
      {/if}
    </div>

    {#if readOnly && !sourceReadOnly}
      <div class="banner banner-warning">
        <span class="banner-icon" aria-hidden="true">{@html icons.lock}</span>
        <span>此会话当前为只读状态。请在原 Agent 中关闭会话，等待约 30 秒后点击“重新读取”。</span>
      </div>
    {/if}

    {#if notice}
      <div class="banner banner-success" role="status">
        <span class="banner-icon" aria-hidden="true">{@html icons.check}</span>
        <span>{notice}</span>
      </div>
    {/if}

    {#if error}
      <div class="banner banner-error" role="alert">
        <span class="banner-icon" aria-hidden="true">{@html icons.alert}</span>
        <span>{error}</span>
      </div>
    {/if}

    {#if history && history.partCount > 1}
      <div class="part-toolbar">
        <label class="part-select-label">
          <span>会话分段</span>
          <select
            class="field-select part-select"
            value={part}
            disabled={busy || hasActiveAction}
            onchange={async (event) => {
              const select = event.currentTarget;
              await load('last', Number(select.value));
              select.value = String(part);
            }}
          >
            {#each Array.from({ length: history.partCount }, (_, i) => i) as i (i)}
              <option value={i}>第 {i + 1} 段 / 共 {history.partCount} 段</option>
            {/each}
          </select>
        </label>
        <span class="part-hint">{sourceReadOnly ? '同一会话在不同本地来源可能各有一份日志。' : '编辑与删除仅作用于当前分段；多段会话不支持整段移除。'}</span>
      </div>
    {/if}

    {#if removingKey === 'session'}
      <div class="confirm-banner">
        <span class="confirm-text">确认移除整个会话源文件？原文件将重命名为 <code>.bak</code> 供随时恢复。</span>
        <div class="confirm-actions">
          <button type="button" class="btn-secondary btn-compact" disabled={saving} onclick={() => { removingKey = null; }}>
            取消
          </button>
          <button
            type="button"
            class="btn-danger btn-compact"
            disabled={saving || isGenerating}
            onclick={() => void removeSession()}
          >
            {saving ? '正在删除…' : '确认移除整个会话'}
          </button>
        </div>
      </div>
    {/if}
  </div>

  <div class="messages-viewport" bind:this={viewport} aria-busy={loading}>
    {#if backupPath}
      <div class="backup-result-card">
        <p class="backup-title">原会话文件已备份至：</p>
        <code class="backup-code">{backupPath}</code>
      </div>
    {:else if loading}
      <div class="loading-viewport" role="status">
        <div class="loading-pill-row">
          <span class="loading-spinner-ring" aria-hidden="true"><span class="loading-spinner-core"></span></span>
          <span>正在读取本地会话记录…</span>
        </div>
        {#each [0, 1, 2, 3] as idx (idx)}
          <div class="skeleton-message-card" style={`--delay: ${idx * 90}ms`}>
            <div class="skeleton-header">
              <span class="sk-bone sk-role"></span>
              <span class="sk-bone sk-line"></span>
            </div>
            <span class="sk-bone sk-text-long"></span>
            <span class="sk-bone sk-text-mid"></span>
          </div>
        {/each}
      </div>
    {:else if history && history.messages.length === 0}
      <div class="empty-state">此页没有可显示的对话正文。</div>
    {:else if history}
      {#each history.messages as message (msgKey(message))}
        {@const key = msgKey(message)}
        {@const isEditing = selectedKey === key}
        {@const isRemoving = removingKey === key}
        <article class="message-card" class:editing={isEditing} class:removing={isRemoving}>
          <div class="message-header">
            <div class="message-role-group">
              <span class="role-badge" class:user={message.role === 'user'} class:assistant={message.role === 'assistant'}>
                {message.role === 'user' ? '用户' : '助手'}
              </span>
              <span class="line-meta tabular">行 #{message.line}</span>
            </div>

            {#if history.writable && !isGenerating && !isEditing}
              <div class="message-actions">
                <button
                  type="button"
                  class="btn-secondary btn-compact"
                  disabled={hasActiveAction}
                  onclick={() => {
                    selectedKey = key;
                    draft = message.text;
                    error = '';
                    notice = '';
                  }}
                >
                  {@html icons.edit}编辑正文
                </button>
                <button
                  type="button"
                  class="btn-danger-ghost btn-compact"
                  disabled={hasActiveAction}
                  onclick={() => {
                    removingKey = key;
                    error = '';
                    notice = '';
                  }}
                >
                  {@html icons.trash}删除正文
                </button>
              </div>
            {/if}
          </div>

          {#if isEditing}
            <div class="editor-box">
              <textarea
                aria-label="消息正文"
                rows="6"
                bind:value={draft}
                disabled={saving}
                onkeydown={(e) => handleTextareaKeydown(e, message)}
              ></textarea>
              <div class="editor-footer">
                <span class="editor-shortcut-hint">Ctrl + Enter 保存 · Esc 取消</span>
                <div class="confirm-actions">
                  <button
                    type="button"
                    class="btn-secondary btn-compact"
                    disabled={saving}
                    onclick={() => {
                      selectedKey = null;
                      error = '';
                    }}
                  >
                    取消
                  </button>
                  <button
                    type="button"
                    class="btn-primary btn-compact"
                    disabled={saving || isGenerating || !draft.trim() || draft === message.text}
                    onclick={() => void save(message, draft)}
                  >
                    {saving ? '正在保存…' : '保存到原 Agent'}
                  </button>
                </div>
              </div>
            </div>
          {:else}
            <p class="message-text">{message.text}</p>
            {#if isRemoving}
              <div class="confirm-banner inline-confirm">
                <span class="confirm-text">从本地源会话中删除这段正文？</span>
                <div class="confirm-actions">
                  <button type="button" class="btn-secondary btn-compact" disabled={saving} onclick={() => { removingKey = null; }}>
                    取消
                  </button>
                  <button
                    type="button"
                    class="btn-danger btn-compact"
                    disabled={saving || isGenerating}
                    onclick={() => void save(message, null)}
                  >
                    {saving ? '正在删除…' : '确认删除正文'}
                  </button>
                </div>
              </div>
            {/if}
          {/if}
        </article>
      {/each}
    {/if}
  </div>

  <footer class="dialog-footer">
    <div class="footer-left">
      {#if history && history.total > 0}
        <span class="page-counter tabular">
          第 {offset + 1}–{offset + history.messages.length} 条 / 共 {history.total} 条
        </span>
        <button
          type="button"
          class="btn-secondary btn-compact"
          disabled={busy || hasActiveAction || offset === 0}
          onclick={() => void load(Math.max(0, offset - pageSize))}
        >
          上一页
        </button>
        <div class="page-jump-group">
          <input
            type="number"
            class="field-input page-jump-input tabular"
            aria-label="跳转到页码"
            min="1"
            max={totalPages}
            bind:value={pageInput}
            disabled={busy || hasActiveAction || totalPages <= 1}
            onkeydown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault();
                commitPageJump();
              }
            }}
            onblur={commitPageJump}
          />
          <span class="page-total-label tabular">/ {totalPages} 页</span>
          <button
            type="button"
            class="btn-secondary btn-compact"
            disabled={busy || hasActiveAction || totalPages <= 1 || Number(pageInput) === currentPage}
            onclick={commitPageJump}
          >
            跳转
          </button>
        </div>
        <button
          type="button"
          class="btn-secondary btn-compact"
          disabled={busy || hasActiveAction || offset + pageSize >= history.total}
          onclick={() => void load(offset + pageSize)}
        >
          下一页
        </button>
      {/if}
      {#if !backupPath}
        <button
          type="button"
          class="btn-secondary btn-compact"
          disabled={busy || removingKey !== null}
          onclick={() => void load(history ? offset : 'last')}
        >
          {@html icons.refresh}{selectedKey ? '放弃修改并重读' : '重新读取'}
        </button>
      {/if}
    </div>

    {#if history?.writable && history.partCount === 1 && !isGenerating}
      <button
        type="button"
        class="btn-danger-ghost btn-compact"
        disabled={busy || hasActiveAction}
        onclick={() => {
          removingKey = 'session';
          error = '';
          notice = '';
        }}
      >
        {@html icons.trash}删除整个会话
      </button>
    {/if}
  </footer>
</dialog>

<style>
  dialog {
    width: min(780px, calc(100vw - 32px));
    height: min(84vh, 780px);
    max-height: min(84vh, 780px);
    padding: 0;
    color: var(--text-primary);
    background: var(--bg-card);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-elevated);
    overflow: hidden;
  }

  dialog[open] {
    display: flex;
    flex-direction: column;
  }

  dialog::backdrop {
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(2px);
  }

  .dialog-header {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }

  .header-meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .header-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }

  .header-subtitle {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .session-id-code {
    font-family: var(--font-mono);
    font-size: 11px;
    padding: 1px 5px;
    border-radius: var(--radius-xs, 4px);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    color: var(--text-muted);
  }

  .status-pill.readonly {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    font-weight: 500;
    padding: 1px 7px;
    border-radius: var(--radius-pill);
    background: var(--color-estimated-bg);
    border: 1px solid var(--color-estimated-border);
    color: var(--color-estimated);
  }

  .status-pill.readonly :global(svg) {
    width: 11px;
    height: 11px;
  }

  .dialog-banners {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 18px 0;
    flex-shrink: 0;
  }

  .banner {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    padding: 7px 10px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    line-height: 1.45;
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-secondary);
  }

  .banner code {
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .banner-icon {
    display: inline-flex;
    margin-top: 1px;
    flex-shrink: 0;
  }

  .banner-icon :global(svg) {
    width: 14px;
    height: 14px;
  }

  .banner-warning {
    background: var(--color-estimated-bg);
    border-color: var(--color-estimated-border);
    color: var(--color-estimated);
  }

  .banner-success {
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
    color: var(--color-exact);
  }

  .banner-error {
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
    color: var(--color-danger);
    overflow-wrap: anywhere;
  }

  .part-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    padding: 4px 0 2px;
  }

  .part-select-label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .part-select {
    min-height: 28px;
    padding: 0 8px;
    font-size: 12px;
  }

  .part-hint {
    font-size: 11.5px;
    color: var(--text-muted);
  }

  .confirm-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    background: var(--color-danger-bg);
    border: 1px solid var(--color-danger-border);
  }

  .confirm-banner.inline-confirm {
    margin-top: 8px;
  }

  .confirm-text {
    font-size: 12px;
    font-weight: 500;
    color: var(--color-danger);
  }

  .confirm-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .messages-viewport {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 10px 18px 14px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .loading-viewport {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding-top: 4px;
  }

  .loading-pill-row {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    padding: 4px 2px;
  }

  .skeleton-message-card {
    padding: 12px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .skeleton-header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .sk-role {
    width: 44px;
    height: 18px;
    border-radius: var(--radius-xs, 4px);
  }

  .sk-line {
    width: 56px;
    height: 14px;
  }

  .sk-text-long {
    width: 92%;
    height: 14px;
  }

  .sk-text-mid {
    width: 64%;
    height: 14px;
  }

  .empty-state {
    flex: 1;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 8px;
    padding: 42px 16px;
    color: var(--text-muted);
    font-size: 12.5px;
  }

  .backup-result-card {
    padding: 14px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .backup-title {
    margin: 0;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .backup-code {
    font-family: var(--font-mono);
    font-size: 11.5px;
    padding: 8px 10px;
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    color: var(--text-primary);
    overflow-wrap: anywhere;
    user-select: all;
  }

  .message-card {
    padding: 10px 12px;
    border-radius: var(--radius-md);
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }

  .message-card:hover {
    border-color: var(--border-strong);
  }

  .message-card.editing {
    border-color: var(--brand-border);
    background: var(--bg-card);
  }

  .message-card.removing {
    border-color: var(--color-danger-border);
  }

  .message-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
  }

  .message-role-group {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  .role-badge {
    font-size: 11px;
    font-weight: 600;
    padding: 1px 7px;
    border-radius: var(--radius-xs, 4px);
    line-height: 1.4;
  }

  .role-badge.user {
    background: var(--brand-subtle);
    border: 1px solid var(--brand-border);
    color: var(--brand-primary);
  }

  .role-badge.assistant {
    background: var(--color-aggregate-bg);
    border: 1px solid var(--color-aggregate-border);
    color: var(--color-aggregate);
  }

  .line-meta {
    font-size: 11px;
    color: var(--text-muted);
    font-family: var(--font-mono);
  }

  .message-actions {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .message-text {
    margin: 7px 0 0;
    font-size: 12.5px;
    line-height: 1.55;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    color: var(--text-primary);
    max-height: 260px;
    overflow-y: auto;
  }

  .editor-box {
    margin-top: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  textarea {
    display: block;
    width: 100%;
    box-sizing: border-box;
    font-family: var(--font-sans);
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-primary);
    background: var(--bg-input);
    padding: 8px 10px;
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    resize: vertical;
    min-height: 100px;
  }

  textarea:focus {
    outline: none;
    border-color: var(--brand-primary);
  }

  .editor-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    flex-wrap: wrap;
  }

  .editor-shortcut-hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  .dialog-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
    padding: 10px 18px;
    border-top: 1px solid var(--border-subtle);
    background: var(--bg-elevated);
    flex-shrink: 0;
  }

  .footer-left {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .page-counter {
    font-size: 12px;
    color: var(--text-secondary);
    margin-right: 2px;
  }

  .page-jump-group {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .page-jump-input {
    width: 52px;
    min-height: 26px;
    height: 26px;
    padding: 0 6px;
    font-size: 11.5px;
    text-align: center;
    appearance: textfield;
    -moz-appearance: textfield;
  }

  .page-jump-input::-webkit-outer-spin-button,
  .page-jump-input::-webkit-inner-spin-button {
    -webkit-appearance: none;
    margin: 0;
  }

  .page-total-label {
    font-size: 11.5px;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .btn-compact {
    min-height: 26px;
    padding: 0 9px;
    font-size: 11.5px;
    gap: 4px;
  }

  .btn-compact :global(svg) {
    width: 12px;
    height: 12px;
  }

  :is(button, select, textarea, input):focus-visible {
    outline: 2px solid var(--brand-primary);
    outline-offset: 2px;
  }
</style>
