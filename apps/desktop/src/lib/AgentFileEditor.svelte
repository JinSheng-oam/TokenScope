<script lang="ts">
  export let content: string;
  export let busy: boolean;
  export let dirty: boolean;
  export let exists: boolean;

  $: editorLines = content ? content.split('\n').length : 0;
  $: editorChars = content ? content.length : 0;

</script>

<div class="editor-textarea-wrap">
  <label class="sr-only" for="agent-editor">文件内容</label>
  <textarea
    id="agent-editor"
    spellcheck="false"
    bind:value={content}
    disabled={busy}
    placeholder="文件为空，在此输入文本内容..."
  ></textarea>
</div>

<div class="editor-foot">
  <div class="editor-foot-left">
    {#if dirty}
      <span class="foot-status dirty">
        <span class="status-dot dirty"></span> 有未保存的更改
      </span>
    {:else if exists}
      <span class="foot-status saved">
        <span class="status-dot saved"></span> 已保存
      </span>
    {:else}
      <span class="foot-status unsaved">
        <span class="status-dot unsaved"></span> 文件尚未创建
      </span>
    {/if}
    <span class="foot-divider">·</span>
    <span class="foot-metrics">{editorLines} 行 · {editorChars} 字符</span>
  </div>
  <div class="editor-foot-right">
    <span>UTF-8</span>
    <span class="foot-divider">·</span>
    <span>最大 1 MiB</span>
  </div>
</div>
<style>
  /* Textarea Editor Box */
  .editor-textarea-wrap {
    flex: 1;
    min-height: 380px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }

  textarea {
    flex: 1;
    width: 100%;
    height: 100%;
    min-height: 380px;
    resize: none;
    padding: 16px;
    border: none;
    outline: none;
    color: var(--text-primary);
    background: transparent;
    font: 12.5px/1.65 var(--font-mono, 'JetBrains Mono', 'Cascadia Code', Consolas, monospace);
    tab-size: 2;
  }

  /* Editor Footer Status Bar */
  .editor-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 7px 18px;
    border-top: 1px solid var(--border-subtle);
    font-size: 11px;
    color: var(--text-muted);
    background: var(--bg-card);
  }

  .editor-foot-left, .editor-foot-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .foot-status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 500;
  }

  .foot-status.dirty {
    color: #f59e0b;
  }

  .foot-status.saved {
    color: var(--color-exact);
  }

  .foot-status .status-dot.dirty {
    background: #f59e0b;
  }

  .foot-status .status-dot.saved {
    background: var(--color-exact);
  }

  .foot-divider {
    color: var(--border-default);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  .status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  @media (min-width: 901px) and (min-height: 620px) {
    :global(.fit-workspace .editor.has-selection) .editor-textarea-wrap,
    :global(.fit-workspace .editor.has-selection) textarea { min-height: 0; }
  }
</style>
