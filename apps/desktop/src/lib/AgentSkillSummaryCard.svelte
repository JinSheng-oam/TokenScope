<script lang="ts">
  import { icons } from './icons';
  import type { SkillSummary } from './agentSkillSourceTypes';

  export let skillSummary: SkillSummary;
  export let analyzingSkill = false;
  export let showSummaryCard = true;
  export let onRegenerate: () => void = () => {};
</script>

<div class="summary-card">
  <div class="summary-header">
    <div class="summary-title">
      <span class="summary-sparkle">{@html icons.sparkle}</span>
      <span>AI 智能摘要分析</span>
      <span class="summary-meta">模型: {skillSummary.model}</span>
    </div>
    <div class="summary-actions">
      <button class="btn-compact" onclick={() => showSummaryCard = !showSummaryCard}>
        {showSummaryCard ? '收起' : '展开'}
      </button>
      <button
        class="btn-compact"
        disabled={analyzingSkill}
        onclick={onRegenerate}
      >
        {analyzingSkill ? '生成中…' : '重新分析'}
      </button>
    </div>
  </div>
  {#if showSummaryCard}
    <div class="summary-body">
      <div class="summary-section">
        <h4>💡 功能定位（这个 Skill 是干什么的）：</h4>
        <p>{skillSummary.overview}</p>
      </div>
      {#if skillSummary.usage_guide}
        <div class="summary-section">
          <h4>📖 推荐使用方式与调用指南：</h4>
          <pre class="summary-pre">{skillSummary.usage_guide}</pre>
        </div>
      {/if}
    </div>
  {/if}
</div>

<style>
  pre {
    max-height: 220px;
    overflow: auto;
    white-space: pre-wrap;
    font: 11px/1.5 var(--font-mono, monospace);
    background: var(--bg-secondary);
    padding: 8px;
    border-radius: var(--radius-xs);
    border: 1px solid var(--border-subtle);
  }

  .summary-card {
    margin: 12px 18px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    overflow: hidden;
    font-size: 12px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.03);
  }

  .summary-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--brand-border);
    background: var(--brand-subtle);
  }

  .summary-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    color: var(--brand-strong);
  }

  .summary-sparkle {
    display: inline-flex;
    width: 14px;
    height: 14px;
  }

  .summary-sparkle :global(svg) {
    width: 100%;
    height: 100%;
  }

  .summary-meta {
    font-size: 10.5px;
    color: var(--text-muted);
    font-weight: normal;
  }

  .summary-actions {
    display: flex;
    gap: 6px;
  }

  .summary-body {
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .summary-section h4 {
    margin: 0 0 4px;
    font-size: 12px;
    color: var(--text-primary);
  }

  .summary-section p {
    margin: 0;
    line-height: 1.55;
    color: var(--text-secondary);
  }

  .summary-pre {
    margin: 0;
    padding: 8px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    white-space: pre-wrap;
  }
</style>
