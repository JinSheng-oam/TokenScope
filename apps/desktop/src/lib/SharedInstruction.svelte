<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  let { source = null, unsaved = $bindable(false) }: { source?: string | null; unsaved?: boolean } = $props();
  type Target = { key: string; label: string; path: string; initialized: boolean };
  type Change = { key: string; path: string; before: string | null; after: string | null; error: string | null; token: string | null };
  type View = { draft: string | null; enabled: boolean; auto_apply: boolean; selected: string[]; targets: Target[]; changes: Change[]; results: string[]; conflict: string | null; conflict_token: string | null; message: string };
  let view = $state<View | null>(null);
  let draft = $state('');
  let busy = $state(false);
  let error = $state('');
  let conflict = $state<{ text: string; token: string } | null>(null);
  let dirty = $derived(draft !== (view?.draft ?? ''));
  $effect(() => { unsaved = dirty; });
  async function run(kind: string, args: Record<string, unknown> = {}, updateDraft = false) {
    if (busy) return;
    busy = true;
    error = '';
    try {
      const next = await invoke<View>('shared_instruction', { action: { kind, ...args } });
      view = next;
      if (updateDraft) draft = next.draft ?? '';
      if (kind === 'sync') conflict = next.conflict !== null && next.conflict_token ? {text: next.conflict, token: next.conflict_token} : null;
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
  onMount(() => { void run('load', {}, true); });
  function configure(selected: string[], enabled = view?.enabled ?? false, auto_apply = view?.auto_apply ?? false) {
    void run('configure', { selected, enabled, auto_apply });
  }
  function toggle(key: string) {
    const selected = view?.selected ?? [];
    configure(selected.includes(key) ? selected.filter(item => item !== key) : [...selected, key]);
  }
  function apply() {
    const approved = Object.fromEntries((view?.changes ?? []).filter(c => c.token).map(c => [c.key, c.token]));
    void run('apply', { approved });
  }
</script>

<section class="shared" aria-label="统一指令">
  <div class="heading"><div><h2>统一指令</h2></div>
    <button disabled={busy || source === null} onclick={() => { if (source !== null) draft = source; }}>从当前文件导入</button>
  </div>
  <label for="shared-instruction-draft">统一指令主稿</label>
  <textarea id="shared-instruction-draft" bind:value={draft} disabled={busy || !view} rows="10" spellcheck="false"></textarea>
  <div class="actions">
    <button class="primary" disabled={busy || !view || (!dirty && view.draft !== null)} onclick={() => run('save', { markdown: draft, expected: view?.draft ?? null }, true)}>保存主稿</button>
    <button disabled={busy || dirty || !view?.enabled} onclick={() => run('sync', {}, true)}>同步到设备</button>
    {#if view?.targets.some(t => t.initialized && view?.selected.includes(t.key))}<button disabled={busy || dirty} onclick={() => run("quick_apply")}>一键应用到本机</button>{/if}
    <button disabled={busy || dirty || view?.draft == null || !view.selected.length} onclick={() => run('preview')}>预览并应用到本机</button>
  </div>
  {#if view}
    <fieldset disabled={busy}>
      <legend>设备同步</legend>
      <label><input type="checkbox" checked={view.enabled} onchange={(e) => configure(view?.selected ?? [], e.currentTarget.checked)} /> 启用统一指令 WebDAV 同步</label>
      <p>仅同步统一指令正文。</p>
      <label><input type="checkbox" disabled={!view.enabled} checked={view.auto_apply} onchange={(e) => configure(view?.selected ?? [], true, e.currentTarget.checked)} /> 收到更新后自动应用到本机已启用的目标</label>
    </fieldset>
    <fieldset disabled={busy}>
      <legend>本机应用目标</legend>
      <button onclick={() => configure(view?.targets.map(t => t.key) ?? [])}>全选目标</button>
      {#each view.targets as target (target.key)}
        <div class="target">
          <label><input type="checkbox" checked={view.selected.includes(target.key)} onchange={() => toggle(target.key)} /><span>{target.label}<small>{target.path} · {target.initialized ? "已启用应用" : "待首次应用"}</small></span></label>
          <button disabled={!target.initialized} onclick={() => { if (window.confirm('恢复此目标上一次应用前的版本？')) void run('restore', { key: target.key }); }}>恢复备份</button>
        </div>
      {/each}
    </fieldset>
    {#if view.changes.length}
      <div class="review">
        <h3>应用预览</h3>
        {#each view.changes as change (change.key)}
          <details>
            <summary>{change.path}{change.error ? ' · 无法应用' : ''}</summary>
            {#if change.error}<p>{change.error}</p>
            {:else}<div class="versions"><div>当前文件<pre>{change.before ?? '文件尚不存在'}</pre></div><div>应用后<pre>{change.after}</pre></div></div>{/if}
          </details>
        {/each}
        <button class="primary" disabled={busy || dirty || !view.changes.some(c => c.token)} onclick={apply}>应用可写目标</button>
      </div>
    {/if}
    {#if conflict}
      <div class="review">
        <h3>两台设备都修改了主稿</h3>
        <p>可载入远端版本，或保存合并稿后发布。</p>
        <pre>{conflict.text}</pre>
        <div class="actions">
          <button disabled={busy} onclick={() => { if (conflict) draft = conflict.text; }}>载入远端到编辑器</button>
          <button disabled={busy || dirty} onclick={() => run('sync', { resolve: conflict?.token }, true)}>发布已保存的主稿</button>
        </div>
      </div>
    {/if}
    <div role="status">{busy ? '处理中…' : view.message}{#each view.results as result}<div>{result}</div>{/each}</div>
  {/if}
  {#if error}<p role="alert" class="error">{error}</p>{/if}
</section>

<style>
  .shared { display:grid; gap:12px; padding:16px; border:1px solid var(--border, #8b929c); border-radius:12px; }
  .heading,.actions,.target { display:flex; flex-wrap:wrap; align-items:center; gap:8px; }
  .heading,.target { justify-content:space-between; }
  h2,h3,p { margin:0; }
  p,small { font-size:.85rem; }
  small { display:block; overflow-wrap:anywhere; }
  textarea,pre { font-family:monospace; }
  textarea { width:100%; resize:vertical; box-sizing:border-box; padding:10px; color:inherit; background:transparent; border:1px solid var(--border,#8b929c); border-radius:6px; }
  button { font:inherit; color:inherit; background:transparent; border:1px solid var(--border,#8b929c); border-radius:6px; padding:5px 10px; cursor:pointer; }
  button:disabled { opacity:.55; cursor:default; }
  .primary { background:#4849ad; color:white; border-color:transparent; }
  fieldset { display:grid; gap:8px; min-width:0; border:1px solid var(--border,#8b929c); border-radius:6px; }
  fieldset > button { justify-self:start; }
  label { display:flex; align-items:flex-start; gap:8px; }
  .target { padding:6px 0; }
  .review { display:grid; gap:8px; min-width:0; }
  .versions { display:grid; gap:8px; grid-template-columns:repeat(auto-fit,minmax(min(240px,100%),1fr)); }
  pre { white-space:pre-wrap; overflow-wrap:anywhere; max-height:18rem; overflow:auto; }
  .error { color:var(--danger,#b54132); }
</style>
