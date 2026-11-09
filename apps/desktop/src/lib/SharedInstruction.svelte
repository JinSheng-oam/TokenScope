<script lang="ts">
  import { onMount, onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  let {
    source = null,
    unsaved = $bindable(false),
    configured = $bindable(false),
    onClose = undefined,
  }: {
    source?: string | null;
    unsaved?: boolean;
    configured?: boolean;
    onClose?: () => void;
  } = $props();

  type Target = { key: string; label: string; path: string; initialized: boolean };
  type Change = { key: string; path: string; before: string | null; after: string | null; error: string | null; token: string | null };
  type View = {
    draft: string | null;
    enabled: boolean;
    auto_apply: boolean;
    selected: string[];
    targets: Target[];
    changes: Change[];
    results: string[];
    conflict: string | null;
    conflict_token: string | null;
    message: string;
  };

  function normalizeLines(val: string | null | undefined): string {
    return (val ?? '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  }

  let view = $state<View | null>(null);
  let draft = $state('');
  let busy = $state(false);
  let error = $state('');
  let conflict = $state<{ text: string; token: string } | null>(null);
  let dirty = $derived(normalizeLines(draft) !== normalizeLines(view?.draft));
  let draftLines = $derived(draft ? draft.split('\n').length : 0);

  $effect(() => {
    unsaved = dirty;
    configured = Boolean(view && view.draft?.trim());
  });

  onDestroy(() => {
    unsaved = false;
    configured = false;
  });

  async function run(kind: string, args: Record<string, unknown> = {}, updateDraft = false) {
    if (busy) return;
    busy = true;
    error = '';
    try {
      const next = await invoke<View>('shared_instruction', { action: { kind, ...args } });
      const replaceDraft = updateDraft || (kind === 'configure' && !dirty);
      view = next;
      if (replaceDraft) draft = normalizeLines(next.draft ?? '');
      if (kind === 'sync' || kind === 'save' || next.conflict !== null || !next.enabled) {
        conflict = next.conflict !== null && next.conflict_token
          ? { text: next.conflict, token: next.conflict_token }
          : null;
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  onMount(() => {
    void run('load', {}, true);
  });

  function configure(selected: string[], enabled = view?.enabled ?? false, auto_apply = view?.auto_apply ?? false) {
    void run('configure', { selected, enabled, auto_apply });
  }

  function toggle(key: string) {
    const selected = view?.selected ?? [];
    configure(selected.includes(key) ? selected.filter((item) => item !== key) : [...selected, key]);
  }

  function apply() {
    const approved = Object.fromEntries((view?.changes ?? []).filter((c) => c.token).map((c) => [c.key, c.token]));
    void run('apply', { approved });
  }
</script>

<section class="shared" aria-label="统一指令">
  <div class="shared-head">
    <div class="shared-title-group">
      <div class="shared-title-row">
        <h2>统一指令</h2>
        {#if dirty}
          <span class="state-pill warn">有未保存修改</span>
        {:else if view?.draft}
          <span class="state-pill ok">主稿已就绪</span>
        {:else}
          <span class="state-pill muted">主稿为空</span>
        {/if}
      </div>
      <p class="shared-subtitle">集中维护跨工具通用指令主稿，一键预览并分发至本机各 Agent 全局入口文件</p>
    </div>

    <div class="shared-head-actions">
      <button
        type="button"
        class="btn-secondary btn-compact-action"
        disabled={busy || source === null}
        title={source === null ? '请先在下方文件列表中选中一个指令文件' : '将下方当前打开的指令文件内容导入为主稿'}
        onclick={() => {
          if (source !== null) draft = normalizeLines(source);
        }}
      >
        从当前文件导入
      </button>
      {#if onClose}
        <button type="button" class="btn-close-panel" onclick={onClose} title="收起统一指令面板">✕ 收起</button>
      {/if}
    </div>
  </div>

  <div class="shared-workbench">
    <!-- Left Column: Master Draft Editor -->
    <div class="shared-editor-pane">
      <div class="editor-label-bar">
        <label for="shared-instruction-draft">统一指令主稿</label>
        <span class="draft-metrics">{draftLines} 行 · {draft.length} 字符</span>
      </div>

      <textarea
        id="shared-instruction-draft"
        bind:value={draft}
        disabled={busy || !view}
        rows="9"
        spellcheck="false"
        placeholder="在此输入跨 Agent 工具共享的 Markdown 指令主稿（例如：终端环境约束、编码风格、Git 提交规范等）…"
      ></textarea>

      <div class="actions">
        <button
          type="button"
          class="btn-primary"
          disabled={busy || !view || (!dirty && view.draft !== null)}
          onclick={() => {
            const toSave = view?.draft && view.draft.includes('\r\n') ? draft.replace(/\r?\n/g, '\r\n') : draft;
            void run('save', { markdown: toSave, expected: view?.draft ?? null }, true);
          }}
        >
          保存主稿
        </button>
        <button
          type="button"
          class="btn-secondary"
          disabled={busy || dirty || view?.draft == null || !view.selected.length}
          onclick={() => run('preview')}
        >
          预览并应用到本机
        </button>
        {#if view?.targets.some((t) => t.initialized && view?.selected.includes(t.key))}
          <button
            type="button"
            class="btn-secondary selected"
            disabled={busy || dirty}
            onclick={() => run('quick_apply')}
          >
            一键应用到本机
          </button>
        {/if}
        <button
          type="button"
          class="btn-secondary"
          disabled={busy || dirty || !view?.enabled}
          onclick={() => run('sync', {}, true)}
        >
          同步到设备
        </button>
      </div>
    </div>

    <!-- Right Column: Sync & Local Application Targets -->
    {#if view}
      <div class="shared-side-pane">
        <fieldset class="shared-card-fieldset" disabled={busy}>
          <div class="fieldset-header">
            <legend>设备同步</legend>
            <span class="sync-badge" class:enabled={view.enabled}>
              {view.enabled ? 'WebDAV 已启用' : '仅本机'}
            </span>
          </div>

          <div class="sync-options">
            <label class="check-row">
              <input
                type="checkbox"
                checked={view.enabled}
                onchange={(e) => configure(view?.selected ?? [], e.currentTarget.checked)}
              />
              <span class="check-text">
                <strong>启用统一指令 WebDAV 同步</strong>
                <small>启用后立即同步；保存主稿时也会同步。仅同步正文。</small>
              </span>
            </label>

            <label class="check-row" class:disabled={!view.enabled}>
              <input
                type="checkbox"
                disabled={!view.enabled}
                checked={view.auto_apply}
                onchange={(e) => configure(view?.selected ?? [], true, e.currentTarget.checked)}
              />
              <span class="check-text">
                <span>收到更新后自动应用到本机已启用的目标</span>
              </span>
            </label>
          </div>
        </fieldset>

        <fieldset class="shared-card-fieldset targets-fieldset" disabled={busy}>
          <div class="fieldset-header">
            <div class="fieldset-title-left">
              <legend>本机应用目标</legend>
              <span class="target-count-pill">{view.selected.length} / {view.targets.length} 已选</span>
            </div>
            <button
              type="button"
              class="btn-select-all"
              onclick={() => configure(view?.targets.map((t) => t.key) ?? [])}
            >
              全选目标
            </button>
          </div>

          <div class="target-list">
            {#each view.targets as target (target.key)}
              {@const isChecked = view.selected.includes(target.key)}
              <div class="target" class:selected={isChecked}>
                <label class="target-label">
                  <input
                    type="checkbox"
                    checked={isChecked}
                    onchange={() => toggle(target.key)}
                  />
                  <span class="target-info">
                    <span class="target-name-row">
                      <strong class="target-title">{target.label}</strong>
                      <span class="target-status-tag" class:initialized={target.initialized}>
                        {target.initialized ? '已启用应用' : '待首次应用'}
                      </span>
                    </span>
                    <small class="target-path">{target.path} · {target.initialized ? '已启用应用' : '待首次应用'}</small>
                  </span>
                </label>
                <button
                  type="button"
                  class="btn-restore"
                  disabled={!target.initialized}
                  onclick={() => {
                    if (window.confirm('恢复此目标上一次应用前的版本？')) void run('restore', { key: target.key });
                  }}
                >
                  恢复备份
                </button>
              </div>
            {/each}
          </div>
        </fieldset>
      </div>
    {/if}
  </div>

  {#if view}
    {#if view.changes.length}
      <div class="review">
        <div class="review-head">
          <h3>应用预览</h3>
          <button
            type="button"
            class="btn-primary btn-compact-action"
            disabled={busy || dirty || !view.changes.some((c) => c.token)}
            onclick={apply}
          >
            应用可写目标
          </button>
        </div>
        <div class="review-list">
          {#each view.changes as change (change.key)}
            <details class="diff-details" open>
              <summary>{change.path}{change.error ? ' · 无法应用' : ''}</summary>
              {#if change.error}
                <p class="error">{change.error}</p>
              {:else}
                <div class="versions">
                  <div class="version-col">
                    <span class="version-label">当前文件</span>
                    <pre>{change.before ?? '文件尚不存在'}</pre>
                  </div>
                  <div class="version-col">
                    <span class="version-label after">应用后</span>
                    <pre>{change.after}</pre>
                  </div>
                </div>
              {/if}
            </details>
          {/each}
        </div>
      </div>
    {/if}

    {#if conflict}
      <div class="review conflict-review">
        <div class="review-head">
          <div>
            <h3>两台设备都修改了主稿</h3>
            <p>可载入远端版本，或保存合并稿后发布。</p>
          </div>
          <div class="actions">
            <button
              type="button"
              class="btn-secondary btn-compact-action"
              disabled={busy}
              onclick={() => {
                if (conflict) draft = normalizeLines(conflict.text);
              }}
            >
              载入远端到编辑器
            </button>
            <button
              type="button"
              class="btn-primary btn-compact-action"
              disabled={busy || dirty}
              onclick={() => run('sync', { resolve: conflict?.token }, true)}
            >
              发布已保存的主稿
            </button>
          </div>
        </div>
        <pre>{conflict.text}</pre>
      </div>
    {/if}

    {#if busy || view.message || view.results.length > 0}
      <div class="shared-status-bar" role="status">
        <span class="status-main">{busy ? '处理中…' : view.message}</span>
        {#if view.results.length > 0}
          <div class="status-results">
            {#each view.results as result}
              <span class="result-chip">{result}</span>
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  {/if}

  {#if error}
    <p role="alert" class="error-banner">{error}</p>
  {/if}
</section>

<style>
  .shared {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 18px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    box-shadow: var(--shadow-card);
    animation: sharedFadeIn 150ms ease-out;
  }

  @keyframes sharedFadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .shared-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    flex-wrap: wrap;
    padding-bottom: 12px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .shared-title-group {
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .shared-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h2,
  h3,
  p {
    margin: 0;
  }

  h2 {
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.015em;
    color: var(--text-primary);
  }

  .shared-subtitle {
    font-size: 11.5px;
    color: var(--text-muted);
  }

  .state-pill {
    display: inline-flex;
    align-items: center;
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 10.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  .state-pill.ok {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border: 1px solid var(--color-exact-border);
  }

  .state-pill.warn {
    background: var(--color-estimated-bg);
    color: var(--color-estimated);
    border: 1px solid var(--color-estimated-border);
  }

  .state-pill.muted {
    background: var(--bg-secondary);
    color: var(--text-muted);
    border: 1px solid var(--border-subtle);
  }

  .shared-head-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .btn-compact-action {
    min-height: 28px;
    padding: 0 10px;
    font-size: 11.5px;
  }

  .btn-close-panel {
    height: 28px;
    padding: 0 8px;
    font-size: 11px;
    color: var(--text-muted);
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-close-panel:hover {
    color: var(--text-primary);
    background: var(--bg-secondary);
    border-color: var(--border-subtle);
  }

  /* Two-Column Workbench Layout */
  .shared-workbench {
    display: grid;
    grid-template-columns: minmax(0, 1.25fr) minmax(320px, 1fr);
    gap: 16px;
    align-items: stretch;
  }

  .shared-editor-pane {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
  }

  .editor-label-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .editor-label-bar label {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-secondary);
  }

  .draft-metrics {
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
  }

  textarea {
    width: 100%;
    flex: 1;
    min-height: 184px;
    resize: vertical;
    box-sizing: border-box;
    padding: 12px 14px;
    color: var(--text-primary);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    font: 12px/1.6 var(--font-mono);
    transition: border-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  textarea::placeholder {
    color: var(--text-muted);
  }

  textarea:focus {
    outline: none;
    border-color: var(--brand-primary);
    box-shadow: 0 0 0 2px var(--brand-subtle);
    background: var(--bg-card);
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding-top: 2px;
  }

  /* Right Column: Sync & Targets */
  .shared-side-pane {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }

  .shared-card-fieldset {
    margin: 0;
    padding: 11px 13px;
    min-width: 0;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .fieldset-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
  }

  .fieldset-title-left {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  legend {
    padding: 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
    float: left;
  }

  .sync-badge {
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-muted);
    border: 1px solid var(--border-subtle);
  }

  .sync-badge.enabled {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border-color: var(--color-exact-border);
  }

  .sync-options {
    display: flex;
    flex-direction: column;
    gap: 6px;
    clear: both;
  }

  .check-row {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    font-size: 12px;
    color: var(--text-primary);
    cursor: pointer;
    user-select: none;
  }

  .check-row.disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .check-row input[type='checkbox'],
  .target-label input[type='checkbox'] {
    margin-top: 2px;
    accent-color: var(--brand-strong);
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    cursor: pointer;
  }

  .check-text {
    display: flex;
    flex-direction: column;
    gap: 1px;
    line-height: 1.35;
  }

  .check-text strong {
    font-weight: 500;
    font-size: 12px;
  }

  .check-text small {
    font-size: 11px;
    color: var(--text-muted);
  }

  .target-count-pill {
    font-family: var(--font-mono);
    font-size: 10px;
    font-weight: 600;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--bg-card);
    color: var(--text-secondary);
    border: 1px solid var(--border-subtle);
  }

  .btn-select-all {
    height: 22px;
    padding: 0 8px;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--brand-strong);
    background: var(--brand-subtle);
    border: 1px solid var(--brand-border);
    border-radius: 4px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-select-all:hover:not(:disabled) {
    background: var(--brand-strong);
    color: #ffffff;
  }

  .target-list {
    display: flex;
    flex-direction: column;
    gap: 5px;
    clear: both;
  }

  .target {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 7px 10px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    transition: border-color var(--transition-fast), background-color var(--transition-fast);
  }

  .target:hover {
    border-color: var(--border-strong);
  }

  .target.selected {
    border-color: var(--brand-border);
    background: linear-gradient(90deg, var(--brand-subtle) 0%, var(--bg-card) 100%);
  }

  .target-label {
    display: flex;
    align-items: flex-start;
    gap: 8px;
    flex: 1;
    min-width: 0;
    cursor: pointer;
  }

  .target-info {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .target-name-row {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .target-title {
    font-size: 12px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .target-status-tag {
    font-size: 10px;
    font-weight: 500;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text-muted);
    border: 1px solid var(--border-subtle);
  }

  .target-status-tag.initialized {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border-color: var(--color-exact-border);
  }

  .target-path {
    display: block;
    font-family: var(--font-mono);
    font-size: 10.5px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .btn-restore {
    height: 24px;
    padding: 0 8px;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    white-space: nowrap;
    flex-shrink: 0;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-restore:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-strong);
    background: var(--bg-card-hover);
  }

  .btn-restore:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }

  /* Diff Preview & Conflict Panels */
  .review {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 12px 14px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    min-width: 0;
  }

  .conflict-review {
    border-color: var(--color-estimated-border);
    background: var(--color-estimated-bg);
  }

  .review-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    flex-wrap: wrap;
  }

  .review-head h3 {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .review-head p {
    font-size: 11.5px;
    color: var(--text-secondary);
    margin-top: 2px;
  }

  .review-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .diff-details {
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    padding: 8px 10px;
  }

  .diff-details summary {
    font-family: var(--font-mono);
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-primary);
    cursor: pointer;
  }

  .versions {
    display: grid;
    gap: 10px;
    margin-top: 8px;
    grid-template-columns: repeat(auto-fit, minmax(min(240px, 100%), 1fr));
  }

  .version-col {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
  }

  .version-label {
    font-size: 10.5px;
    font-weight: 600;
    color: var(--text-muted);
  }

  .version-label.after {
    color: var(--color-exact);
  }

  pre {
    margin: 0;
    padding: 8px 10px;
    font: 11px/1.5 var(--font-mono);
    color: var(--text-primary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    max-height: 15rem;
    overflow: auto;
  }

  .shared-status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px;
    padding: 7px 12px;
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    font-size: 11.5px;
    color: var(--text-secondary);
  }

  .status-results {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .result-chip {
    font-family: var(--font-mono);
    font-size: 10.5px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    color: var(--text-primary);
  }

  .error,
  .error-banner {
    font-size: 11.5px;
    color: var(--color-danger);
  }

  .error-banner {
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    background: var(--color-danger-bg);
    border: 1px solid var(--color-danger-border);
  }

  @media (max-width: 980px) {
    .shared-workbench {
      grid-template-columns: 1fr;
    }
  }
</style>
