<script lang="ts">
  import { onDestroy } from 'svelte';
  import { icons } from './icons';
  import { parseSummaryMarkdown } from './skillSummaryMarkdown';
  import type { SkillSummary } from './agentSkillSourceTypes';

  export let skillSummary: SkillSummary;
  export let analyzingSkill = false;
  export let showSummaryCard = true;
  export let onRegenerate: () => void = () => {};

  $: overviewBlocks = parseSummaryMarkdown(skillSummary.overview ?? '');
  $: usageBlocks = parseSummaryMarkdown(skillSummary.usage_guide ?? '');

  let copiedBlockKey = '';
  let copyTimer: ReturnType<typeof setTimeout> | null = null;

  async function copyCodeBlock(key: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copiedBlockKey = key;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedBlockKey = '';
      }, 1500);
    } catch {
      // Ignore clipboard permission failure
    }
  }

  onDestroy(() => {
    if (copyTimer) clearTimeout(copyTimer);
  });
</script>

<div class="summary-drawer" class:collapsed={!showSummaryCard}>
  <div class="summary-header">
    <button
      type="button"
      class="summary-toggle-main"
      aria-expanded={showSummaryCard}
      onclick={() => showSummaryCard = !showSummaryCard}
    >
      <span class="summary-chevron" class:open={showSummaryCard} aria-hidden="true">
        {@html icons.chevronRight}
      </span>
      <span class="summary-sparkle" aria-hidden="true">{@html icons.sparkle}</span>
      <span class="summary-label">AI 智能摘要</span>
      {#if skillSummary.model}
        <span class="summary-model-pill" title={`由 ${skillSummary.model} 生成`}>{skillSummary.model}</span>
      {/if}
    </button>

    <div class="summary-actions">
      <button
        type="button"
        class="summary-action-btn"
        disabled={analyzingSkill}
        onclick={onRegenerate}
        title="使用当前 AI 模型重新分析"
      >
        <span class="action-icon" class:spin={analyzingSkill} aria-hidden="true">{@html icons.refresh}</span>
        <span>{analyzingSkill ? '分析中…' : '重新分析'}</span>
      </button>
      <button
        type="button"
        class="summary-action-btn"
        onclick={() => showSummaryCard = !showSummaryCard}
      >
        {showSummaryCard ? '收起' : '展开'}
      </button>
    </div>
  </div>

  {#if showSummaryCard}
    <div class="summary-body">
      {#if overviewBlocks.length > 0}
        <div class="summary-section">
          <div class="summary-section-head">
            <span class="section-dot overview"></span>
            <h4>功能定位</h4>
          </div>
          <div class="md-content">
            {#each overviewBlocks as block, idx}
              {#if block.type === 'heading'}
                <h5 class="md-heading">
                  {#each block.tokens as token}
                    {#if token.type === 'strong'}<strong>{token.text}</strong>
                    {:else if token.type === 'em'}<em>{token.text}</em>
                    {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                    {:else}{token.text}{/if}
                  {/each}
                </h5>
              {:else if block.type === 'paragraph'}
                <p class="md-p">
                  {#each block.tokens as token}
                    {#if token.type === 'strong'}<strong>{token.text}</strong>
                    {:else if token.type === 'em'}<em>{token.text}</em>
                    {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                    {:else}{token.text}{/if}
                  {/each}
                </p>
              {:else if block.type === 'list'}
                {#if block.ordered}
                  <ol class="md-list">
                    {#each block.items as item}
                      <li>
                        {#each item as token}
                          {#if token.type === 'strong'}<strong>{token.text}</strong>
                          {:else if token.type === 'em'}<em>{token.text}</em>
                          {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                          {:else}{token.text}{/if}
                        {/each}
                      </li>
                    {/each}
                  </ol>
                {:else}
                  <ul class="md-list">
                    {#each block.items as item}
                      <li>
                        {#each item as token}
                          {#if token.type === 'strong'}<strong>{token.text}</strong>
                          {:else if token.type === 'em'}<em>{token.text}</em>
                          {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                          {:else}{token.text}{/if}
                        {/each}
                      </li>
                    {/each}
                  </ul>
                {/if}
              {:else if block.type === 'blockquote'}
                <blockquote class="md-quote">
                  {#each block.tokens as token}
                    {#if token.type === 'strong'}<strong>{token.text}</strong>
                    {:else if token.type === 'em'}<em>{token.text}</em>
                    {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                    {:else}{token.text}{/if}
                  {/each}
                </blockquote>
              {:else if block.type === 'code'}
                {@const key = `ov-${idx}`}
                <div class="md-code-block">
                  <div class="md-code-bar">
                    {#if block.lang}<span class="md-code-lang">{block.lang}</span>{/if}
                    <button
                      type="button"
                      class="md-code-copy"
                      onclick={() => void copyCodeBlock(key, block.code)}
                    >
                      {copiedBlockKey === key ? '已复制' : '复制'}
                    </button>
                  </div>
                  <pre><code>{block.code}</code></pre>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      {/if}

      {#if usageBlocks.length > 0}
        <div class="summary-section">
          <div class="summary-section-head">
            <span class="section-dot guide"></span>
            <h4>使用方式与调用指南</h4>
          </div>
          <div class="md-content">
            {#each usageBlocks as block, idx}
              {#if block.type === 'heading'}
                <h5 class="md-heading">
                  {#each block.tokens as token}
                    {#if token.type === 'strong'}<strong>{token.text}</strong>
                    {:else if token.type === 'em'}<em>{token.text}</em>
                    {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                    {:else}{token.text}{/if}
                  {/each}
                </h5>
              {:else if block.type === 'paragraph'}
                <p class="md-p">
                  {#each block.tokens as token}
                    {#if token.type === 'strong'}<strong>{token.text}</strong>
                    {:else if token.type === 'em'}<em>{token.text}</em>
                    {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                    {:else}{token.text}{/if}
                  {/each}
                </p>
              {:else if block.type === 'list'}
                {#if block.ordered}
                  <ol class="md-list">
                    {#each block.items as item}
                      <li>
                        {#each item as token}
                          {#if token.type === 'strong'}<strong>{token.text}</strong>
                          {:else if token.type === 'em'}<em>{token.text}</em>
                          {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                          {:else}{token.text}{/if}
                        {/each}
                      </li>
                    {/each}
                  </ol>
                {:else}
                  <ul class="md-list">
                    {#each block.items as item}
                      <li>
                        {#each item as token}
                          {#if token.type === 'strong'}<strong>{token.text}</strong>
                          {:else if token.type === 'em'}<em>{token.text}</em>
                          {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                          {:else}{token.text}{/if}
                        {/each}
                      </li>
                    {/each}
                  </ul>
                {/if}
              {:else if block.type === 'blockquote'}
                <blockquote class="md-quote">
                  {#each block.tokens as token}
                    {#if token.type === 'strong'}<strong>{token.text}</strong>
                    {:else if token.type === 'em'}<em>{token.text}</em>
                    {:else if token.type === 'code'}<code class="md-inline-code">{token.text}</code>
                    {:else}{token.text}{/if}
                  {/each}
                </blockquote>
              {:else if block.type === 'code'}
                {@const key = `ug-${idx}`}
                <div class="md-code-block">
                  <div class="md-code-bar">
                    {#if block.lang}<span class="md-code-lang">{block.lang}</span>{/if}
                    <button
                      type="button"
                      class="md-code-copy"
                      onclick={() => void copyCodeBlock(key, block.code)}
                    >
                      {copiedBlockKey === key ? '已复制' : '复制'}
                    </button>
                  </div>
                  <pre><code>{block.code}</code></pre>
                </div>
              {/if}
            {/each}
          </div>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .summary-drawer {
    flex-shrink: 0;
    border-bottom: 1px solid var(--border-subtle);
    background: color-mix(in srgb, var(--brand-subtle) 22%, var(--bg-card));
  }

  .summary-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 6px 14px 6px 12px;
    background: color-mix(in srgb, var(--brand-subtle) 58%, var(--bg-card));
  }

  .summary-drawer:not(.collapsed) .summary-header {
    border-bottom: 1px solid color-mix(in srgb, var(--brand-border) 70%, transparent);
  }

  .summary-toggle-main {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 2px 4px;
    border: none;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--brand-strong);
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: background-color var(--transition-fast);
  }

  .summary-toggle-main:hover {
    background: color-mix(in srgb, var(--brand-subtle) 85%, transparent);
  }

  .summary-chevron {
    display: inline-flex;
    width: 13px;
    height: 13px;
    color: var(--text-muted);
    transition: transform 150ms ease;
    flex-shrink: 0;
  }

  .summary-chevron.open {
    transform: rotate(90deg);
    color: var(--brand-strong);
  }

  .summary-chevron :global(svg),
  .summary-sparkle :global(svg),
  .action-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .summary-sparkle {
    display: inline-flex;
    width: 13px;
    height: 13px;
    flex-shrink: 0;
    color: var(--brand-primary);
  }

  .summary-label {
    white-space: nowrap;
  }

  .summary-model-pill {
    padding: 1px 6px;
    border-radius: 999px;
    border: 1px solid color-mix(in srgb, var(--brand-border) 80%, transparent);
    background: var(--bg-card);
    color: var(--text-secondary);
    font-family: var(--font-mono, monospace);
    font-size: 10px;
    font-weight: 500;
    line-height: 1.4;
    max-width: 180px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .summary-actions {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-shrink: 0;
  }

  .summary-action-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .summary-action-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .summary-action-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  .action-icon {
    display: inline-flex;
    width: 11px;
    height: 11px;
  }

  .action-icon.spin {
    animation: spin 0.8s linear infinite;
  }

  .summary-body {
    padding: 10px 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-height: clamp(140px, 31vh, 218px);
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
  }

  .summary-section {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .summary-section-head {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .section-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    flex-shrink: 0;
  }

  .section-dot.overview {
    background: var(--brand-primary);
  }

  .section-dot.guide {
    background: var(--color-exact);
  }

  .summary-section-head h4 {
    margin: 0;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-primary);
    letter-spacing: 0.01em;
  }

  .md-content {
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
    line-height: 1.62;
    color: var(--text-secondary);
  }

  .md-heading {
    margin: 2px 0 0;
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-primary);
  }

  .md-p {
    margin: 0;
    white-space: pre-line;
  }

  .md-content strong {
    color: var(--text-primary);
    font-weight: 600;
  }

  .md-inline-code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
  }

  .md-list {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .md-list li::marker {
    color: var(--text-muted);
  }

  .md-quote {
    margin: 0;
    padding: 4px 10px;
    border-left: 2px solid var(--brand-border);
    background: color-mix(in srgb, var(--bg-secondary) 70%, transparent);
    border-radius: 0 var(--radius-xs) var(--radius-xs) 0;
    color: var(--text-secondary);
  }

  .md-code-block {
    position: relative;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    overflow: hidden;
  }

  .md-code-bar {
    position: absolute;
    top: 5px;
    right: 6px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    z-index: 1;
  }

  .md-code-lang {
    padding: 1px 5px;
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text-muted);
    font-family: var(--font-mono, monospace);
    font-size: 9.5px;
    text-transform: uppercase;
  }

  .md-code-copy {
    height: 20px;
    padding: 0 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    background: var(--bg-card);
    color: var(--text-muted);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
    opacity: 0.85;
    transition: all var(--transition-fast);
  }

  .md-code-copy:hover {
    opacity: 1;
    color: var(--brand-text);
    border-color: var(--brand-border);
    background: var(--brand-subtle);
  }

  .md-code-block pre {
    margin: 0;
    padding: 8px 54px 8px 10px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    font: 11.5px/1.55 var(--font-mono, 'JetBrains Mono', 'Cascadia Code', Consolas, monospace);
    color: var(--text-primary);
  }
</style>
