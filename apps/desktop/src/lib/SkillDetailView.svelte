<script lang="ts">
  import { onDestroy } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { confirmDialog, promptDialog } from './dialogStore';
  import { icons } from './icons';
  import { ALL_PROVIDERS } from './providerBadges';
  import { parseSummaryMarkdown } from './skillSummaryMarkdown';
  import {
    deriveCardBrief,
    getProviderSyncMode,
    isProviderEnabled,
    isSkillAnalyzable,
    sourceTypeLabel,
    type SkillLibraryItem,
    type SkillSyncMode,
  } from './skillLibraryTypes';
  import { syncModeLabel, type SkillSyncOverride } from './skillSyncPrefs';
  import type { BatchItemState } from './skillBatchAnalyzer';
  import type { Entry, Provider } from './agentCustomizationEntries';
  import type { UpdateInfo } from './agentSkillSourceTypes';
  import AgentFileEditor from './AgentFileEditor.svelte';
  import AgentSkillUpdates from './AgentSkillUpdates.svelte';

  export let item: SkillLibraryItem;
  export let batchState: BatchItemState | undefined = undefined;
  export let globalSyncMode: SkillSyncMode = 'link';
  export let syncOverride: SkillSyncOverride = 'inherit';
  export let hasAiConfig = false;
  export let dirty = false;
  export let onClose: () => void = () => {};
  export let onRefreshLibrary: () => Promise<void> = async () => {};
  export let onAnalyzeSingle: (item: SkillLibraryItem) => void = () => {};
  export let onToggleEnabled: (item: SkillLibraryItem) => Promise<void> = async () => {};
  export let onToggleProvider: (item: SkillLibraryItem, provider: Provider, enabled: boolean) => Promise<void> = async () => {};
  export let onChangeSyncOverride: (item: SkillLibraryItem, next: SkillSyncOverride, applyNow: boolean) => Promise<void> = async () => {};
  export let onOpenAiConfig: () => void = () => {};

  let contentTab: 'preview' | 'edit' = 'preview';
  let sourceExpanded = true;
  let loadingContent = false;
  let saving = false;
  let checkingUpdate = false;
  let content = '';
  let original: string | null = null;
  let error = '';
  let notice = '';
  let updateInfo: UpdateInfo | null = null;
  let copiedPath = false;
  let copiedCodeKey = '';
  let lastLoadedKey = '';
  let copyTimer: ReturnType<typeof setTimeout> | undefined;
  let codeCopyTimer: ReturnType<typeof setTimeout> | undefined;

  function normalizeLines(val: string | null | undefined): string {
    return (val ?? '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  }

  function stripFrontmatter(md: string): string {
    const norm = normalizeLines(md);
    if (!norm.startsWith('---\n')) return norm;
    const end = norm.indexOf('\n---\n', 4);
    if (end === -1) return norm;
    return norm.slice(end + 5).trimStart();
  }

  $: dirty = normalizeLines(content) !== normalizeLines(original);
  $: brief = deriveCardBrief(item);
  $: analyzable = isSkillAnalyzable(item);
  $: isAnalyzing = batchState?.status === 'running' || batchState?.status === 'retrying';
  $: effectiveSyncMode = syncOverride === 'inherit' ? globalSyncMode : syncOverride;
  $: enabledCount = ALL_PROVIDERS.filter((p) => isProviderEnabled(item, p.id)).length;

  $: overviewBlocks = parseSummaryMarkdown(item.summary?.overview ?? '');
  $: scenarioBlocks = parseSummaryMarkdown(item.summary?.scenarios ?? '');
  $: usageBlocks = parseSummaryMarkdown(item.summary?.usage_guide ?? '');
  $: previewBlocks = parseSummaryMarkdown(stripFrontmatter(content));

  $: selectedAsEntry = {
    id: item.primary_id as unknown as Entry['id'],
    path: item.path,
    kind: 'skill',
    exists: item.exists,
    editable: item.editable,
    builtin: item.builtin,
    linked: item.linked,
    enabled: item.enabled,
    instructionEntry: false,
  } as Entry;

  $: if (item.key !== lastLoadedKey) {
    lastLoadedKey = item.key;
    updateInfo = null;
    error = '';
    notice = '';
    contentTab = 'preview';
    void loadContent(item);
  }

  async function loadContent(target: SkillLibraryItem) {
    if (!target.path) {
      original = target.description || '';
      content = target.description || '';
      return;
    }
    loadingContent = true;
    try {
      const raw = await invoke<string | null>('read_customization', { id: target.primary_id });
      original = raw;
      content = raw ? normalizeLines(raw) : '';
    } catch (e) {
      error = String(e);
    } finally {
      loadingContent = false;
    }
  }

  async function handleClose() {
    if (dirty) {
      const ok = await confirmDialog({
        title: '未保存的更改',
        message: '当前 SKILL.md 有未保存的修改，确定放弃并返回技能库？',
        confirmText: '放弃更改',
      });
      if (!ok) return;
      dirty = false;
    }
    onClose();
  }

  async function saveContent() {
    if (!item.editable || saving) return;
    saving = true;
    error = '';
    notice = '';
    try {
      const toSave = original && original.includes('\r\n') ? content.replace(/\r?\n/g, '\r\n') : content;
      await invoke('save_customization', { id: item.primary_id, expected: original, content: toSave });
      original = toSave;
      content = normalizeLines(toSave);
      notice = '已保存到本地 SKILL.md。';
      await onRefreshLibrary();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function renameSkill() {
    if (!item.editable || saving) return;
    const nextName = await promptDialog({
      title: '重命名 Skill',
      message: '请输入新的 Skill 名称（仅允许小写字母、数字和连字符）：',
      defaultValue: item.name,
      confirmText: '重命名',
    });
    if (!nextName || nextName.trim() === '' || nextName.trim() === item.name) return;
    saving = true;
    error = '';
    notice = '';
    try {
      await invoke('rename_customization', { id: item.primary_id, newName: nextName.trim() });
      notice = `已重命名为 ${nextName.trim()}`;
      await onRefreshLibrary();
      onClose();
    } catch (e) {
      error = String(e);
    } finally {
      saving = false;
    }
  }

  async function checkUpdate() {
    if (checkingUpdate || dirty) return;
    checkingUpdate = true;
    error = '';
    notice = '';
    try {
      updateInfo = await invoke<UpdateInfo>('check_skill_update', { id: item.primary_id });
    } catch (e) {
      error = String(e);
    } finally {
      checkingUpdate = false;
    }
  }

  async function copyPath() {
    if (!item.path) return;
    try {
      await navigator.clipboard.writeText(item.path);
      copiedPath = true;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedPath = false;
      }, 1800);
    } catch {
      // ignore
    }
  }

  async function copyCode(key: string, text: string) {
    try {
      await navigator.clipboard.writeText(text);
      copiedCodeKey = key;
      if (codeCopyTimer) clearTimeout(codeCopyTimer);
      codeCopyTimer = setTimeout(() => {
        copiedCodeKey = '';
      }, 1500);
    } catch {
      // ignore
    }
  }

  function formatTimestamp(ms: number | undefined): string {
    if (!ms) return '';
    try {
      return new Date(ms).toLocaleString('zh-CN', {
        month: '2-digit',
        day: '2-digit',
        hour: '2-digit',
        minute: '2-digit',
      });
    } catch {
      return '';
    }
  }

  onDestroy(() => {
    if (copyTimer) clearTimeout(copyTimer);
    if (codeCopyTimer) clearTimeout(codeCopyTimer);
  });
</script>

<div class="detail-shell">
  <div class="detail-top">
    <div class="detail-header-main">
      <div class="detail-title-row">
        <h1>{item.name}</h1>
        <span class="detail-status-pill" class:enabled={item.enabled}>
          <span class="dot"></span>
          {item.enabled ? '已启用' : '已停用'}
        </span>
        {#if item.linked}
          <span class="detail-meta-pill">软链接</span>
        {/if}
        {#if item.builtin}
          <span class="detail-meta-pill">系统内置</span>
        {/if}
      </div>

      {#if brief.text}
        <p class="detail-subtitle">{brief.text}</p>
      {/if}

      <div class="detail-meta-strip">
        <div class="provider-icons-row">
          {#each ALL_PROVIDERS as p (p.id)}
            {@const isOn = isProviderEnabled(item, p.id)}
            <span
              class="provider-chip"
              class:inactive={!isOn}
              style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}
              title={`${p.label}（${isOn ? '已启用' : '已禁用'}）`}
            >
              {@html p.icon}
            </span>
          {/each}
        </div>

        {#if item.path}
          <div class="detail-path-wrap">
            <span class="detail-path" title={item.path}>{item.path}</span>
            <button type="button" class="mini-copy-btn" onclick={() => void copyPath()}>
              {copiedPath ? '已复制' : '复制路径'}
            </button>
          </div>
        {/if}
      </div>
    </div>

    <div class="detail-top-actions">
      {#if !item.builtin}
        <button
          type="button"
          class="btn-secondary btn-sm"
          disabled={saving}
          onclick={() => void onToggleEnabled(item)}
        >
          {item.enabled ? '禁用 Skill' : '启用 Skill'}
        </button>
      {/if}
      {#if item.editable && item.exists}
        <button
          type="button"
          class="btn-secondary btn-sm"
          disabled={saving}
          onclick={() => void renameSkill()}
        >
          改名
        </button>
      {/if}
      <button
        type="button"
        class="detail-close-btn"
        onclick={() => void handleClose()}
        title="返回技能库"
        aria-label="返回技能库"
      >
        {@html icons.close}
      </button>
    </div>
  </div>

  {#if error}<p class="msg error" role="alert">{error}</p>{/if}
  {#if notice}<p class="msg success" role="status">{notice}</p>{/if}

  <!-- 1. Detailed AI Analysis Section -->
  <section class="detail-card ai-detail-card" class:is-analyzing={isAnalyzing}>
    <div class="detail-card-head">
      <div class="card-head-left">
        <span class="head-sparkle" aria-hidden="true">{@html icons.sparkle}</span>
        <strong>AI 深度技能分析</strong>
        {#if item.summary?.model}
          <span class="model-badge">{item.summary.model}</span>
        {/if}
        {#if item.summary?.updated_at_ms}
          <span class="time-text">{formatTimestamp(item.summary.updated_at_ms)}</span>
        {/if}
      </div>
      <div class="card-head-right">
        {#if !hasAiConfig}
          <button type="button" class="btn-secondary btn-xs" onclick={onOpenAiConfig}>
            配置 AI 接口
          </button>
        {:else if analyzable}
          <button
            type="button"
            class="btn-secondary btn-xs ai-trigger-btn"
            disabled={isAnalyzing}
            onclick={() => onAnalyzeSingle(item)}
          >
            <span class="btn-icon" class:spin={isAnalyzing} aria-hidden="true">{@html icons.sparkle}</span>
            <span>{isAnalyzing ? '分析中…' : item.summary ? '重新分析' : '立即生成分析'}</span>
          </button>
        {/if}
      </div>
    </div>

    <div class="ai-detail-body">
      {#if isAnalyzing}
        <div class="ai-loading-banner">
          <span class="pulse-dot"></span>
          <span>
            {batchState?.status === 'retrying'
              ? `正在进行第 ${batchState.attempt}/${batchState.maxRetries} 次自动重试…`
              : '正在调用 AI 深度解读 SKILL.md，生成核心定位、适用场景与调用指南…'}
          </span>
        </div>
      {:else if batchState?.status === 'failed'}
        <div class="ai-error-banner">
          <span>上次分析失败：{batchState.error}</span>
          <button type="button" class="btn-secondary btn-xs" onclick={() => onAnalyzeSingle(item)}>重试</button>
        </div>
      {/if}

      {#if item.summary}
        <div class="ai-sections-grid">
          {#if overviewBlocks.length > 0}
            <div class="ai-block">
              <h4 class="ai-block-title"><span class="dot overview"></span>功能定位与核心能力</h4>
              <div class="md-render">
                {#each overviewBlocks as block, idx}
                  {#if block.type === 'paragraph'}
                    <p>
                      {#each block.tokens as token}
                        {#if token.type === 'strong'}<strong>{token.text}</strong>
                        {:else if token.type === 'em'}<em>{token.text}</em>
                        {:else if token.type === 'code'}<code>{token.text}</code>
                        {:else}{token.text}{/if}
                      {/each}
                    </p>
                  {:else if block.type === 'list'}
                    <ul>
                      {#each block.items as li}
                        <li>
                          {#each li as token}
                            {#if token.type === 'strong'}<strong>{token.text}</strong>
                            {:else if token.type === 'em'}<em>{token.text}</em>
                            {:else if token.type === 'code'}<code>{token.text}</code>
                            {:else}{token.text}{/if}
                          {/each}
                        </li>
                      {/each}
                    </ul>
                  {:else if block.type === 'code'}
                    {@const key = `ov-${idx}`}
                    <div class="code-box">
                      <button type="button" class="code-copy" onclick={() => void copyCode(key, block.code)}>
                        {copiedCodeKey === key ? '已复制' : '复制'}
                      </button>
                      <pre><code>{block.code}</code></pre>
                    </div>
                  {/if}
                {/each}
              </div>
            </div>
          {/if}

          {#if scenarioBlocks.length > 0}
            <div class="ai-block">
              <h4 class="ai-block-title"><span class="dot scenario"></span>适用场景与触发时机</h4>
              <div class="md-render">
                {#each scenarioBlocks as block, idx}
                  {#if block.type === 'paragraph'}
                    <p>
                      {#each block.tokens as token}
                        {#if token.type === 'strong'}<strong>{token.text}</strong>
                        {:else if token.type === 'em'}<em>{token.text}</em>
                        {:else if token.type === 'code'}<code>{token.text}</code>
                        {:else}{token.text}{/if}
                      {/each}
                    </p>
                  {:else if block.type === 'list'}
                    <ul>
                      {#each block.items as li}
                        <li>
                          {#each li as token}
                            {#if token.type === 'strong'}<strong>{token.text}</strong>
                            {:else if token.type === 'em'}<em>{token.text}</em>
                            {:else if token.type === 'code'}<code>{token.text}</code>
                            {:else}{token.text}{/if}
                          {/each}
                        </li>
                      {/each}
                    </ul>
                  {:else if block.type === 'code'}
                    {@const key = `sc-${idx}`}
                    <div class="code-box">
                      <button type="button" class="code-copy" onclick={() => void copyCode(key, block.code)}>
                        {copiedCodeKey === key ? '已复制' : '复制'}
                      </button>
                      <pre><code>{block.code}</code></pre>
                    </div>
                  {/if}
                {/each}
              </div>
            </div>
          {/if}

          {#if usageBlocks.length > 0}
            <div class="ai-block full-span">
              <h4 class="ai-block-title"><span class="dot guide"></span>使用方式与调用指南</h4>
              <div class="md-render">
                {#each usageBlocks as block, idx}
                  {#if block.type === 'heading'}
                    <h5>
                      {#each block.tokens as token}
                        {#if token.type === 'strong'}<strong>{token.text}</strong>
                        {:else if token.type === 'code'}<code>{token.text}</code>
                        {:else}{token.text}{/if}
                      {/each}
                    </h5>
                  {:else if block.type === 'paragraph'}
                    <p>
                      {#each block.tokens as token}
                        {#if token.type === 'strong'}<strong>{token.text}</strong>
                        {:else if token.type === 'em'}<em>{token.text}</em>
                        {:else if token.type === 'code'}<code>{token.text}</code>
                        {:else}{token.text}{/if}
                      {/each}
                    </p>
                  {:else if block.type === 'list'}
                    <ul>
                      {#each block.items as li}
                        <li>
                          {#each li as token}
                            {#if token.type === 'strong'}<strong>{token.text}</strong>
                            {:else if token.type === 'em'}<em>{token.text}</em>
                            {:else if token.type === 'code'}<code>{token.text}</code>
                            {:else}{token.text}{/if}
                          {/each}
                        </li>
                      {/each}
                    </ul>
                  {:else if block.type === 'blockquote'}
                    <blockquote>
                      {#each block.tokens as token}
                        {#if token.type === 'strong'}<strong>{token.text}</strong>
                        {:else if token.type === 'code'}<code>{token.text}</code>
                        {:else}{token.text}{/if}
                      {/each}
                    </blockquote>
                  {:else if block.type === 'code'}
                    {@const key = `ug-${idx}`}
                    <div class="code-box">
                      {#if block.lang}<span class="code-lang">{block.lang}</span>{/if}
                      <button type="button" class="code-copy" onclick={() => void copyCode(key, block.code)}>
                        {copiedCodeKey === key ? '已复制' : '复制'}
                      </button>
                      <pre><code>{block.code}</code></pre>
                    </div>
                  {/if}
                {/each}
              </div>
            </div>
          {/if}
        </div>
      {:else if !isAnalyzing}
        <div class="ai-empty-state">
          <span>尚未生成 AI 分析摘要。点击右上角「立即生成分析」即可自动提炼一句话卡片简介、核心定位、适用场景与调用示例。</span>
        </div>
      {/if}
    </div>
  </section>

  <!-- 2. Collapsible Source Metadata Card -->
  <section class="detail-card">
    <div class="detail-card-head">
      <div class="card-head-left">
        <span class="source-type-badge">{sourceTypeLabel(item.source.source_type)}</span>
        <strong>来源信息</strong>
      </div>
      <div class="card-head-right">
        {#if !item.builtin && item.exists}
          <button
            type="button"
            class="btn-secondary btn-xs"
            disabled={checkingUpdate || dirty}
            onclick={() => void checkUpdate()}
          >
            {checkingUpdate ? '检查中…' : '检查更新'}
          </button>
        {/if}
        <button
          type="button"
          class="collapse-btn"
          onclick={() => (sourceExpanded = !sourceExpanded)}
        >
          {sourceExpanded ? '收起 ▴' : '展开 ▾'}
        </button>
      </div>
    </div>

    {#if sourceExpanded}
      <div class="source-grid">
        <div class="source-field">
          <span class="field-label">来源类型</span>
          <span class="field-val mono">{item.source.source_type}</span>
        </div>
        <div class="source-field">
          <span class="field-label">来源分组</span>
          <span class="field-val mono">{item.source.source_group}</span>
        </div>
        {#if item.source.source_ref}
          <div class="source-field">
            <span class="field-label">来源位置</span>
            <span class="field-val mono">{item.source.source_ref}</span>
          </div>
        {/if}
        {#if item.source.resolved_url}
          <div class="source-field">
            <span class="field-label">解析后来源</span>
            <span class="field-val mono">{item.source.resolved_url}</span>
          </div>
        {/if}
        {#if item.source.subpath}
          <div class="source-field">
            <span class="field-label">子路径</span>
            <span class="field-val mono">{item.source.subpath}</span>
          </div>
        {/if}
        {#if item.source.revision}
          <div class="source-field">
            <span class="field-label">版本</span>
            <span class="field-val mono">{item.source.revision}</span>
          </div>
        {/if}
      </div>

      {#if updateInfo}
        <div class="update-wrap">
          <AgentSkillUpdates
            selected={selectedAsEntry}
            bind:updateInfo
            bind:busy={saving}
            bind:error
            bind:notice
            {dirty}
            onEntries={() => { void onRefreshLibrary(); }}
            onContent={(val) => {
              original = val;
              content = val ? normalizeLines(val) : '';
            }}
          />
        </div>
      {/if}
    {/if}
  </section>

  <!-- 3. Cross-Agent Deployment Matrix & Per-Skill Sync Mode -->
  <section class="detail-card">
    <div class="detail-card-head">
      <div class="card-head-left">
        <strong>工具工作区</strong>
        <span class="sync-pill">已启用 {enabledCount} / {ALL_PROVIDERS.length}</span>
      </div>
      {#if !item.builtin}
        <div class="card-head-right sync-mode-controls">
          <span class="sync-mode-label">同步方式：</span>
          <div class="sync-seg" role="group" aria-label="设置此技能同步方式">
            <button
              type="button"
              class="sync-seg-btn"
              class:active={syncOverride === 'inherit'}
              disabled={saving}
              onclick={() => void onChangeSyncOverride(item, 'inherit', true)}
            >
              跟随全局 ({syncModeLabel(globalSyncMode)})
            </button>
            <button
              type="button"
              class="sync-seg-btn"
              class:active={syncOverride === 'link'}
              disabled={saving}
              onclick={() => void onChangeSyncOverride(item, 'link', true)}
            >
              静态链接
            </button>
            <button
              type="button"
              class="sync-seg-btn"
              class:active={syncOverride === 'copy'}
              disabled={saving}
              onclick={() => void onChangeSyncOverride(item, 'copy', true)}
            >
              文件复制
            </button>
          </div>
        </div>
      {/if}
    </div>

    <div class="workspace-matrix">
      {#each ALL_PROVIDERS as p (p.id)}
        {@const isOn = isProviderEnabled(item, p.id)}
        {@const pMode = getProviderSyncMode(item, p.id)}
        <button
          type="button"
          class="agent-cell"
          class:enabled={isOn}
          disabled={item.builtin || saving}
          title={item.builtin
            ? `${p.label}（系统内置）`
            : isOn
              ? `${p.label}：已启用（${pMode === 'copy' ? '文件复制' : '静态链接'}）· 点击禁用`
              : `${p.label}：已禁用 · 点击启用（将按${syncModeLabel(effectiveSyncMode)}同步）`}
          onclick={() => {
            if (!item.builtin) void onToggleProvider(item, p.id, !isOn);
          }}
        >
          <span class="agent-check" class:on={isOn} class:off={!isOn}>
            {isOn ? '✓' : '−'}
          </span>
          <span class="agent-icon" class:dim={!isOn} style={`--p-color: ${p.color}; --p-bg: ${p.bg}`}>{@html p.icon}</span>
          <span class="agent-label">{p.label}</span>
          <span class="agent-state" class:on={isOn}>
            {isOn ? (pMode === 'copy' ? '已启用 · 复制' : '已启用 · 链接') : '已禁用'}
          </span>
        </button>
      {/each}
    </div>
  </section>

  <!-- 4. SKILL.md Preview / Raw Editor -->
  <section class="detail-card content-card">
    <div class="content-tabs-bar">
      <div class="content-tabs">
        <button
          type="button"
          class="content-tab"
          class:active={contentTab === 'preview'}
          onclick={() => (contentTab = 'preview')}
        >
          本地预览
        </button>
        <button
          type="button"
          class="content-tab"
          class:active={contentTab === 'edit'}
          onclick={() => (contentTab = 'edit')}
        >
          编辑 SKILL.md{dirty ? ' •' : ''}
        </button>
      </div>

      {#if contentTab === 'edit' && item.editable}
        <button
          type="button"
          class="btn-primary btn-xs"
          disabled={saving || (!dirty && item.exists)}
          onclick={() => void saveContent()}
        >
          {saving ? '保存中…' : '保存更改'}
        </button>
      {/if}
    </div>

    {#if loadingContent}
      <div class="content-loading">正在读取 SKILL.md…</div>
    {:else if contentTab === 'preview'}
      <div class="skill-doc-preview md-render">
        {#each previewBlocks as block, idx}
          {#if block.type === 'heading'}
            <h3 class={`doc-h${Math.min(block.level, 4)}`}>
              {#each block.tokens as token}
                {#if token.type === 'strong'}<strong>{token.text}</strong>
                {:else if token.type === 'code'}<code>{token.text}</code>
                {:else}{token.text}{/if}
              {/each}
            </h3>
          {:else if block.type === 'paragraph'}
            <p>
              {#each block.tokens as token}
                {#if token.type === 'strong'}<strong>{token.text}</strong>
                {:else if token.type === 'em'}<em>{token.text}</em>
                {:else if token.type === 'code'}<code>{token.text}</code>
                {:else}{token.text}{/if}
              {/each}
            </p>
          {:else if block.type === 'list'}
            {#if block.ordered}
              <ol>
                {#each block.items as li}
                  <li>
                    {#each li as token}
                      {#if token.type === 'strong'}<strong>{token.text}</strong>
                      {:else if token.type === 'code'}<code>{token.text}</code>
                      {:else}{token.text}{/if}
                    {/each}
                  </li>
                {/each}
              </ol>
            {:else}
              <ul>
                {#each block.items as li}
                  <li>
                    {#each li as token}
                      {#if token.type === 'strong'}<strong>{token.text}</strong>
                      {:else if token.type === 'code'}<code>{token.text}</code>
                      {:else}{token.text}{/if}
                    {/each}
                  </li>
                {/each}
              </ul>
            {/if}
          {:else if block.type === 'blockquote'}
            <blockquote>
              {#each block.tokens as token}
                {#if token.type === 'strong'}<strong>{token.text}</strong>
                {:else if token.type === 'code'}<code>{token.text}</code>
                {:else}{token.text}{/if}
              {/each}
            </blockquote>
          {:else if block.type === 'code'}
            {@const key = `doc-${idx}`}
            <div class="code-box">
              {#if block.lang}<span class="code-lang">{block.lang}</span>{/if}
              <button type="button" class="code-copy" onclick={() => void copyCode(key, block.code)}>
                {copiedCodeKey === key ? '已复制' : '复制'}
              </button>
              <pre><code>{block.code}</code></pre>
            </div>
          {/if}
        {:else}
          <p class="empty-doc">文件内容为空。</p>
        {/each}
      </div>
    {:else}
      <div class="detail-editor-box">
        <AgentFileEditor bind:content busy={saving} {dirty} exists={item.exists} readOnly={!item.editable} />
      </div>
    {/if}
  </section>
</div>

<style>
  .detail-shell {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding-bottom: 16px;
  }

  .detail-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    padding: 16px 18px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
  }

  .detail-header-main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .detail-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
  }

  .detail-title-row h1 {
    margin: 0;
    font-size: 20px;
    font-weight: 700;
    letter-spacing: -0.025em;
    color: var(--text-primary);
  }

  .detail-status-pill {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 20px;
    padding: 0 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
    background: var(--color-danger-bg);
    color: var(--color-danger);
    border: 1px solid var(--color-danger-border);
  }

  .detail-status-pill.enabled {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border-color: var(--color-exact-border);
  }

  .detail-status-pill .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
  }

  .detail-meta-pill {
    padding: 1px 7px;
    border-radius: 999px;
    font-size: 10.5px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    color: var(--text-muted);
  }

  .detail-subtitle {
    margin: 0;
    font-size: 13px;
    line-height: 1.55;
    color: var(--text-secondary);
  }

  .detail-meta-strip {
    display: flex;
    align-items: center;
    gap: 12px;
    flex-wrap: wrap;
  }

  .provider-icons-row {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .provider-chip,
  .agent-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 3px;
    border-radius: 5px;
    color: var(--p-color);
    background: var(--p-bg);
    flex-shrink: 0;
  }

  .provider-chip.inactive {
    opacity: 0.35;
    filter: grayscale(1);
  }

  .provider-chip :global(svg),
  .agent-icon :global(svg),
  .detail-close-btn :global(svg),
  .head-sparkle :global(svg),
  .btn-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .detail-path-wrap {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    max-width: 100%;
  }

  .detail-path {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .mini-copy-btn {
    height: 21px;
    padding: 0 7px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font: inherit;
    font-size: 10.5px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .mini-copy-btn:hover {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .detail-top-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .btn-sm {
    height: 28px;
    padding: 0 11px;
    font-size: 11.5px;
  }

  .btn-xs {
    height: 25px;
    padding: 0 9px;
    font-size: 11px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .detail-close-btn {
    width: 30px;
    height: 30px;
    padding: 7px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .detail-close-btn:hover {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .msg {
    margin: 0;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
    border: 1px solid var(--border-subtle);
  }

  .msg.error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }

  .msg.success {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
  }

  .detail-card {
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    overflow: hidden;
  }

  .ai-detail-card {
    border-color: var(--brand-border);
  }

  .detail-card-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-subtle);
    background: color-mix(in srgb, var(--bg-secondary) 45%, var(--bg-card));
  }

  .ai-detail-card .detail-card-head {
    background: color-mix(in srgb, var(--brand-subtle) 55%, var(--bg-card));
    border-bottom-color: color-mix(in srgb, var(--brand-border) 70%, transparent);
  }

  .card-head-left {
    display: flex;
    align-items: center;
    gap: 7px;
    font-size: 12.5px;
    color: var(--text-primary);
    flex-wrap: wrap;
  }

  .head-sparkle {
    display: inline-flex;
    width: 14px;
    height: 14px;
    color: var(--brand-primary);
  }

  .model-badge {
    padding: 1px 7px;
    border-radius: 999px;
    border: 1px solid var(--brand-border);
    background: var(--bg-card);
    font-family: var(--font-mono, monospace);
    font-size: 10.5px;
    color: var(--brand-text);
  }

  .time-text,
  .shared-hint {
    font-size: 11px;
    color: var(--text-muted);
  }

  .card-head-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .ai-trigger-btn {
    color: var(--brand-text);
    background: var(--bg-card);
    border-color: var(--brand-border);
  }

  .btn-icon {
    display: inline-flex;
    width: 12px;
    height: 12px;
  }

  .btn-icon.spin {
    animation: spin 1s linear infinite;
  }

  .ai-detail-body {
    padding: 14px 16px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .ai-loading-banner,
  .ai-error-banner {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 8px 12px;
    border-radius: var(--radius-sm);
    font-size: 12px;
  }

  .ai-loading-banner {
    justify-content: flex-start;
    background: var(--brand-subtle);
    color: var(--brand-strong);
  }

  .ai-error-banner {
    background: var(--color-danger-bg);
    color: var(--color-danger);
    border: 1px solid var(--color-danger-border);
  }

  .pulse-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--brand-primary);
    animation: pulse-dot 1s ease-in-out infinite;
  }

  @keyframes pulse-dot {
    0%, 100% { opacity: 0.35; }
    50% { opacity: 1; }
  }

  .ai-sections-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 12px;
  }

  .ai-block {
    padding: 12px 14px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: color-mix(in srgb, var(--bg-secondary) 35%, var(--bg-card));
    display: flex;
    flex-direction: column;
    gap: 7px;
  }

  .ai-block.full-span {
    grid-column: 1 / -1;
  }

  .ai-block-title {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12px;
    font-weight: 650;
    color: var(--text-primary);
  }

  .ai-block-title .dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
  }

  .ai-block-title .dot.overview { background: var(--brand-primary); }
  .ai-block-title .dot.scenario { background: var(--color-estimated); }
  .ai-block-title .dot.guide { background: var(--color-exact); }

  .ai-empty-state {
    font-size: 12px;
    color: var(--text-muted);
    padding: 4px 0;
  }

  .source-type-badge,
  .sync-pill {
    padding: 1px 8px;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    color: var(--text-secondary);
  }

  .collapse-btn {
    border: none;
    background: transparent;
    color: var(--text-muted);
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    padding: 2px 6px;
  }

  .collapse-btn:hover {
    color: var(--text-primary);
  }

  .source-grid {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px 18px;
    padding: 14px 16px;
  }

  .source-field {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .field-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .field-val {
    font-size: 12px;
    color: var(--text-primary);
    overflow-wrap: anywhere;
  }

  .field-val.mono {
    font-family: var(--font-mono, monospace);
    font-size: 11.5px;
  }

  .update-wrap {
    padding: 0 16px 14px;
  }

  .workspace-matrix {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(195px, 1fr));
    gap: 8px;
    padding: 14px 16px;
  }

  .agent-cell {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 9px 11px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    color: var(--text-primary);
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .agent-cell:hover:not(:disabled) {
    border-color: var(--brand-border);
    background: var(--bg-card-hover);
  }

  .agent-cell.enabled {
    border-color: color-mix(in srgb, var(--color-exact-border) 80%, var(--border-subtle));
    background: color-mix(in srgb, var(--color-exact-bg) 35%, var(--bg-card));
  }

  .agent-icon.dim {
    opacity: 0.4;
    filter: grayscale(1);
  }

  .agent-state.on {
    color: var(--color-exact);
    font-weight: 600;
  }

  .sync-mode-controls {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .sync-mode-label {
    font-size: 11px;
    color: var(--text-muted);
  }

  .sync-seg {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
  }

  .sync-seg-btn {
    height: 22px;
    padding: 0 8px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
    white-space: nowrap;
  }

  .sync-seg-btn.active {
    background: var(--brand-subtle);
    color: var(--brand-text);
    font-weight: 600;
  }

  .agent-check {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    font-weight: 700;
    background: var(--bg-secondary);
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .agent-check.on {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border: 1px solid var(--color-exact-border);
  }

  .agent-check.off {
    background: var(--color-estimated-bg);
    color: var(--color-estimated);
  }

  .agent-label {
    font-size: 12px;
    font-weight: 600;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .agent-state {
    font-size: 10.5px;
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .content-tabs-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    padding: 10px 16px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .content-tabs {
    display: inline-flex;
    gap: 6px;
  }

  .content-tab {
    height: 28px;
    padding: 0 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .content-tab.active {
    background: var(--brand-primary);
    border-color: var(--brand-primary);
    color: #fff;
    font-weight: 600;
  }

  .content-loading,
  .empty-doc {
    padding: 20px 16px;
    font-size: 12px;
    color: var(--text-muted);
    margin: 0;
  }

  .skill-doc-preview {
    padding: 16px 20px 20px;
    max-height: 520px;
    overflow-y: auto;
    scrollbar-width: thin;
  }

  .detail-editor-box {
    display: flex;
    flex-direction: column;
    height: 440px;
  }

  .md-render {
    display: flex;
    flex-direction: column;
    gap: 7px;
    font-size: 12.5px;
    line-height: 1.65;
    color: var(--text-secondary);
  }

  .md-render p {
    margin: 0;
    white-space: pre-line;
  }

  .md-render strong {
    color: var(--text-primary);
    font-weight: 600;
  }

  .md-render h5,
  .doc-h1,
  .doc-h2,
  .doc-h3,
  .doc-h4 {
    margin: 6px 0 0;
    color: var(--text-primary);
    font-weight: 700;
  }

  .doc-h1 { font-size: 17px; }
  .doc-h2 { font-size: 15px; }
  .doc-h3 { font-size: 13.5px; }
  .doc-h4 { font-size: 12.5px; }

  .md-render code {
    padding: 0 4px;
    border-radius: 4px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    color: var(--text-primary);
    font-family: var(--font-mono, monospace);
    font-size: 11px;
  }

  .md-render ul,
  .md-render ol {
    margin: 0;
    padding-left: 18px;
    display: flex;
    flex-direction: column;
    gap: 3px;
  }

  .md-render blockquote {
    margin: 0;
    padding: 5px 10px;
    border-left: 2px solid var(--brand-border);
    background: var(--bg-secondary);
    border-radius: 0 var(--radius-xs) var(--radius-xs) 0;
  }

  .code-box {
    position: relative;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    overflow: hidden;
  }

  .code-box pre {
    margin: 0;
    padding: 9px 56px 9px 11px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    font: 11.5px/1.55 var(--font-mono, monospace);
    color: var(--text-primary);
  }

  .code-box pre code {
    padding: 0;
    border: none;
    background: transparent;
  }

  .code-lang {
    position: absolute;
    top: 6px;
    right: 48px;
    font-family: var(--font-mono, monospace);
    font-size: 9.5px;
    color: var(--text-muted);
    text-transform: uppercase;
  }

  .code-copy {
    position: absolute;
    top: 5px;
    right: 6px;
    height: 20px;
    padding: 0 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    background: var(--bg-secondary);
    color: var(--text-secondary);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
  }

  .code-copy:hover {
    border-color: var(--brand-border);
    color: var(--brand-text);
    background: var(--brand-subtle);
  }

  @media (max-width: 820px) {
    .ai-sections-grid,
    .source-grid {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
