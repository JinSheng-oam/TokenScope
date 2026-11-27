<script lang="ts">
  import "./agentCustomizationBadges.css";
  import { onDestroy, onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { confirmDialog, promptDialog } from './dialogStore';
  import { icons } from './icons';
  import {
    instructionInfo,
    instructionRootLabel,
    isReferenced,
    addReference,
    removeReference,
    getMainEntry,
    type Entry,
    type Provider,
    type Id,
  } from './agentCustomizationEntries';
  import type { SkillSummary, UpdateInfo } from './agentSkillSourceTypes';
  import AgentSkillImport from './AgentSkillImport.svelte';
  import AgentSkillUpdates from './AgentSkillUpdates.svelte';
  import AgentAiConfig from './AgentAiConfig.svelte';
  import AgentFileEditor from './AgentFileEditor.svelte';
  import AgentFileCatalog from './AgentFileCatalog.svelte';
  import AgentCreatePanel from './AgentCreatePanel.svelte';
  import AgentAuxReferenceBar from './AgentAuxReferenceBar.svelte';
  import AgentSkillSummaryCard from './AgentSkillSummaryCard.svelte';

  export let runtimeAvailable: boolean;
  export let dirty = false;

  const providers: { id: Provider; label: string; roots: { id: string; label: string }[] }[] = [
    { id: 'codex', label: 'Codex', roots: [{ id: 'shared-skills', label: '共享 skills' }, { id: 'codex-skills', label: 'Codex skills' }] },
    { id: 'claude', label: 'Claude Code', roots: [{ id: 'claude-skills', label: 'Claude skills' }] },
    { id: 'antigravity', label: 'Antigravity', roots: [{ id: 'antigravity-skills', label: 'Antigravity skills' }] },
    { id: 'pi', label: 'Pi Agent', roots: [{ id: 'pi-skills', label: 'Pi skills' }, { id: 'pi-shared-skills', label: '共享 skills' }] },
    { id: 'deepseek', label: 'DeepSeek Harness', roots: [{ id: 'dsh-skills', label: 'DeepSeek skills' }, { id: 'dsh-shared-skills', label: '共享 skills' }] },
  ];

  let provider: Provider = 'codex';
  let entries: Entry[] = [];
  let selected: Entry | null = null;
  let content = '';
  let original: string | null = null;
  let busy = false;
  let error = '';
  let notice = '';
  let creating = false;
  let creatingInstruction = false;
  let importing = false;
  let updateInfo: UpdateInfo | null = null;
  let skillName = '';
  let skillRoot = providers[0].roots[0].id;
  let instructionName = '';
  let instructionRoot = '';
  let instructionPreset = '';
  let request = 0;

  // AI assistant state
  let showAiConfig = false;
  let skillSummary: SkillSummary | null = null;
  let analyzingSkill = false;
  let showSummaryCard = true;

  let copiedPath = false;

  $: instructionPresets = instructions.filter(
    (entry) => entry.id.root === instructionRoot && entry.instructionEntry && !entry.exists
  );
  $: instructionRootOptions = instructions
    .map((entry) => entry.id.root)
    .filter((root, index, roots) => roots.indexOf(root) === index)
    .map((root) => ({ key: root, label: instructionRootLabel(root) }));
  $: instructionPresetOptions = instructionPresets.map((entry) => ({ key: entry.id.name, label: entry.id.name }));
  $: providerRootOptions = currentProvider.roots.map((root) => ({ key: root.id, label: root.label }));
  let hasAiConfig = false;

  // Auxiliary reference to main entry state
  let isReferencedInMain = false;
  let checkingReference = false;
  let togglingReference = false;
  let targetMainEntryName = '';

  $: availableMainEntries = selected && selected.id.provider !== 'pi' && selected.kind === 'instruction' && !selected.instructionEntry
    ? instructions.filter((item) => item.id.root === selected?.id.root && item.instructionEntry)
    : [];

  $: {
    if (availableMainEntries.length > 0) {
      if (!targetMainEntryName || !availableMainEntries.some((item) => item.id.name === targetMainEntryName)) {
        const preferred = getMainEntry(selected!, instructions);
        targetMainEntryName = preferred?.id.name ?? availableMainEntries[0].id.name;
      }
    } else {
      targetMainEntryName = '';
    }
  }

  $: mainEntryForSelected = availableMainEntries.find((item) => item.id.name === targetMainEntryName) ?? null;

  let lastCheckedKey = '';
  $: if (selected && selected.kind === 'instruction' && !selected.instructionEntry && mainEntryForSelected) {
    const key = `${selected.id.provider}:${selected.id.root}:${selected.id.name}->${mainEntryForSelected.id.name}`;
    if (key !== lastCheckedKey) {
      lastCheckedKey = key;
      void checkReferenceStatus(selected, mainEntryForSelected);
    }
  } else {
    lastCheckedKey = '';
    isReferencedInMain = false;
  }

  async function checkReferenceStatus(entry: Entry, targetMain: Entry) {
    checkingReference = true;
    try {
      const mainRaw = await invoke<string | null>('read_customization', { id: targetMain.id });
      isReferencedInMain = mainRaw !== null && isReferenced(mainRaw, entry);
    } catch {
      isReferencedInMain = false;
    } finally {
      checkingReference = false;
    }
  }

  async function toggleMainReference() {
    if (!selected || selected.kind !== 'instruction' || selected.instructionEntry || togglingReference) return;
    const targetEntry = mainEntryForSelected;
    if (!targetEntry) return;

    togglingReference = true;
    error = '';
    notice = '';

    try {
      if (!selected.exists || dirty) {
        await save();
      }

      const mainRaw = await invoke<string | null>('read_customization', { id: targetEntry.id });
      const mainContent = mainRaw ?? '';
      const currentlyReferenced = isReferenced(mainContent, selected);
      const nextReferenced = !currentlyReferenced;

      let nextContent: string;
      const refLine = selected.path.includes(' ') ? `@"${selected.path}"` : `@${selected.path}`;

      if (nextReferenced) {
        nextContent = addReference(mainContent, refLine);
      } else {
        nextContent = removeReference(mainContent, selected);
      }

      const toSave = mainRaw && mainRaw.includes('\r\n')
        ? nextContent.replace(/\r?\n/g, '\r\n')
        : nextContent;

      await invoke('save_customization', {
        id: targetEntry.id,
        expected: mainRaw,
        content: toSave,
      });

      isReferencedInMain = nextReferenced;
      notice = nextReferenced
        ? `已在 ${targetEntry.id.name} 中添加引用：${refLine}`
        : `已从 ${targetEntry.id.name} 中移除引用`;

      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) {
      error = `更新主入口引用失败：${String(e)}`;
    } finally {
      togglingReference = false;
    }
  }
  function normalizeLines(val: string | null | undefined): string {
    return (val ?? '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
  }

  $: dirty = selected !== null && normalizeLines(content) !== normalizeLines(original);
  $: instructions = entries.filter((entry) => entry.kind === 'instruction');
  $: skills = entries.filter((entry) => entry.kind === 'skill');
  $: currentProvider = providers.find((p) => p.id === provider)!;

  let copyPathTimer: ReturnType<typeof setTimeout> | undefined;

  onDestroy(() => {
    if (copyPathTimer !== undefined) clearTimeout(copyPathTimer);
  });

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      copiedPath = true;
      if (copyPathTimer !== undefined) clearTimeout(copyPathTimer);
      copyPathTimer = setTimeout(() => {
        copiedPath = false;
        copyPathTimer = undefined;
      }, 2000);
    } catch {
      // fallback
    }
  }

  async function canLeave(): Promise<boolean> {
    if (!dirty) return true;
    return confirmDialog({
      title: '未保存的更改',
      message: '当前文件有未保存的更改，确定放弃更改？',
      confirmText: '放弃更改',
    });
  }

  async function loadSkillSummary(entry: Entry) {
    if (entry.kind !== 'skill' || !entry.exists) {
      skillSummary = null;
      return;
    }
    try {
      skillSummary = await invoke<SkillSummary | null>('get_skill_summary', { id: entry.id });
    } catch {
      skillSummary = null;
    }
  }

  async function generateSkillSummary() {
    if (!selected || selected.kind !== 'skill' || !selected.exists) return;
    analyzingSkill = true;
    error = '';
    notice = '';
    try {
      skillSummary = await invoke<SkillSummary>('generate_skill_summary', { id: selected.id });
      notice = `AI 分析完成 (${skillSummary.model})。`;
      showSummaryCard = true;
    } catch (e) {
      error = String(e);
    } finally {
      analyzingSkill = false;
    }
  }

  async function refresh() {
    if (!runtimeAvailable) return;
    const current = ++request;
    busy = true; error = '';
    try {
      const result = await invoke<Entry[]>('list_customizations', { provider });
      if (current === request) entries = result;
    } catch (e) { if (current === request) error = String(e); }
    finally { if (current === request) busy = false; }
  }

  async function switchProvider(next: Provider) {
    if (provider === next || !(await canLeave())) return;
    provider = next;
    selected = null;
    content = '';
    original = null;
    creating = false;
    creatingInstruction = false;
    importing = false;

    updateInfo = null;
    skillSummary = null;
    notice = '';
    skillRoot = providers.find((p) => p.id === next)!.roots[0].id;
    await refresh();
  }

  async function select(entry: Entry) {
    if (selected?.id.root === entry.id.root && selected?.id.name === entry.id.name) return;
    if (!(await canLeave()) || (!entry.editable && !entry.builtin)) return;
    const current = ++request;
    busy = true; error = ''; notice = '';
    try {
      const value = entry.builtin_description
        ? entry.builtin_description + '\n\n此条目来自 Claude Code 官方目录。实际可用性取决于本机版本和设置，正文由 Claude Code 管理。'
        : await invoke<string | null>('read_customization', { id: entry.id });
      if (current !== request) return;
      selected = entry;
      original = value;
      content = value ? normalizeLines(value) : '';
      creating = false;
      creatingInstruction = false;
      importing = false;

      updateInfo = null;
      skillSummary = null;
      if (!entry.builtin_description) void loadSkillSummary(entry);
    } catch (e) { if (current === request) error = String(e); }
    finally { if (current === request) busy = false; }
  }

  async function save() {
    if (!selected || !selected.editable || busy) return;
    busy = true; error = ''; notice = '';
    try {
      const toSave = original && original.includes('\r\n') ? content.replace(/\r?\n/g, '\r\n') : content;
      await invoke('save_customization', { id: selected.id, expected: original, content: toSave });
      original = toSave;
      content = normalizeLines(toSave);
      selected = { ...selected, exists: true };
      notice = '已保存到工具的本地文件。';
      const result = await invoke<Entry[]>('list_customizations', { provider });
      entries = result;
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function renameCustomization() {
    if (!selected || !selected.exists || busy) return;
    const isInstruction = selected.kind === 'instruction';
    const promptText = isInstruction
      ? '请输入新的指令文件名（必须以 .md 结尾）：'
      : '请输入新的 Skill 名称（仅允许小写字母、数字和连字符）：';
    const newName = await promptDialog({
      title: isInstruction ? '重命名指令文件' : '重命名 Skill',
      message: promptText,
      defaultValue: selected.id.name,
      confirmText: '重命名',
    });
    if (!newName || newName.trim() === '' || newName.trim() === selected.id.name) return;
    busy = true; error = ''; notice = '';
    const oldEntry = selected;
    const wasReferenced = isReferencedInMain && mainEntryForSelected;
    const targetMain = mainEntryForSelected;
    try {
      const updated = await invoke<Entry>('rename_customization', {
        id: selected.id,
        newName: newName.trim(),
      });
      notice = `已成功重命名为 ${updated.id.name}`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
      selected = updated;

      if (wasReferenced && targetMain) {
        try {
          const mainRaw = await invoke<string | null>('read_customization', { id: targetMain.id });
          if (mainRaw) {
            let next = removeReference(mainRaw, oldEntry);
            const refLine = updated.path.includes(' ') ? `@"${updated.path}"` : `@${updated.path}`;
            next = addReference(next, refLine);
            const toSave = mainRaw.includes('\r\n') ? next.replace(/\r?\n/g, '\r\n') : next;
            await invoke('save_customization', { id: targetMain.id, expected: mainRaw, content: toSave });
            isReferencedInMain = true;
          }
        } catch {
          // Best effort sync of rename in main md
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function deleteInstruction() {
    if (!selected || selected.kind !== 'instruction' || !selected.exists || busy || selected.linked) return;
    const unsavedNote = dirty ? '\n\n未保存的编辑也会丢弃。' : '';
    const confirmed = await confirmDialog({
      title: '删除指令文件',
      message: `确定删除指令文件“${selected.id.name}”？\n\n${selected.path}\n\n此操作无法撤销。${unsavedNote}`,
      tone: 'danger',
      confirmText: '确认删除',
    });
    if (!confirmed) return;

    const deletedEntry = selected;
    const wasReferenced = isReferencedInMain && mainEntryForSelected;
    const targetMain = mainEntryForSelected;
    const deletedName = selected.id.name;
    busy = true;
    error = '';
    notice = '';
    try {
      await invoke('delete_instruction', { id: selected.id });
      selected = null;
      original = null;
      content = '';
      entries = await invoke<Entry[]>('list_customizations', { provider });
      notice = `已删除指令文件：${deletedName}`;

      if (wasReferenced && targetMain) {
        try {
          const mainRaw = await invoke<string | null>('read_customization', { id: targetMain.id });
          if (mainRaw) {
            const next = removeReference(mainRaw, deletedEntry);
            const toSave = mainRaw.includes('\r\n') ? next.replace(/\r?\n/g, '\r\n') : next;
            await invoke('save_customization', { id: targetMain.id, expected: mainRaw, content: toSave });
          }
        } catch {
          // Best effort cleanup of reference
        }
      }
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleSkill() {
    if (!selected || selected.kind !== 'skill' || busy) return;
    busy = true; error = ''; notice = '';
    try {
      const nextEnabled = !selected.enabled;
      await invoke('toggle_skill_enabled', { id: selected.id, enabled: nextEnabled });
      selected = { ...selected, enabled: nextEnabled };
      notice = nextEnabled
        ? `已启用 Skill: ${selected.id.name}`
        : `已停用 Skill: ${selected.id.name}（Agent 将不再识别和载入）`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function beginCreateInstruction() {
    if (!(await canLeave())) return;
    selected = null;
    original = null;
    content = '';
    instructionName = '';
    instructionPreset = '';
    const instRoots = entries.filter((e) => e.kind === 'instruction').map((e) => e.id.root);
    instructionRoot = instRoots[0] || `${provider}-instructions`;
    creatingInstruction = true;
    creating = false;
    importing = false;

    updateInfo = null;
    skillSummary = null;
    error = '';
    notice = '';
  }

  async function createInstruction() {
    let name = instructionName.trim();
    if (!name.toLowerCase().endsWith('.md')) {
      name = `${name}.md`;
    }
    if (name.length < 4 || name.length > 64) {
      error = '请输入有效的文件名（必须以 .md 结尾）';
      return;
    }
    busy = true;
    error = '';
    try {
      const newEntry = await invoke<Entry>('prepare_instruction', {
        provider,
        root: instructionRoot,
        name,
      });
      creatingInstruction = false;
      selected = newEntry;
      original = null;
      content = `# ${name}\n\n`;
      notice = '这是尚未落盘的草稿；保存后才会创建文件。';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function createSkill() {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillName) || skillName.length > 64) {
      error = '名称使用小写字母、数字和连字符，最长 64 个字符。'; return;
    }
    const id: Id = { provider, root: skillRoot, name: skillName };
    busy = true; error = '';
    try {
      const existing = await invoke<string | null>('read_customization', { id });
      if (existing !== null) { error = '这个 skill 已存在，请在列表中打开。'; return; }
      selected = { id, path: `${currentProvider.roots.find((r) => r.id === skillRoot)?.label} / ${skillName} / SKILL.md`, kind: 'skill', exists: false, editable: true, linked: false, enabled: true, instructionEntry: false };
      original = null;
      content = `---\nname: ${skillName}\ndescription: 描述何时使用此 skill。\n---\n\n# ${skillName}\n\n`;
      creating = false; notice = '编辑后点击保存，才会创建文件。';
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function beginCreate() {
    if (!(await canLeave())) return;
    selected = null; original = null; content = ''; skillName = ''; creating = true; creatingInstruction = false; importing = false; updateInfo = null; error = ''; notice = '';
  }

  async function beginImport() {
    if (!(await canLeave())) return;
    selected = null; original = null; content = ''; skillName = ''; importing = true; creating = false; creatingInstruction = false; updateInfo = null; notice = ''; error = '';
  }

  async function checkUpdate() {
    if (!selected || dirty) return;
    busy = true; error = ''; notice = ''; updateInfo = null;
    try { updateInfo = await invoke<UpdateInfo>('check_skill_update', { id: selected.id }); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  onMount(() => {
    void refresh();
  });
</script>

<div class="custom-page" class:fit-workspace={!showAiConfig}>
  <div class="page-header custom-heading">
    <h1>Agent 配置与技能管理</h1>
    <span class="local-badge" title="普通指令与 Skill 仅操作本机文件">
      {@html icons.shield}
      <span>仅操作本机文件</span>
    </span>
  </div>

  <div class="workspace-toolbar">
    <div class="provider-tabs" role="group" aria-label="选择工具">
      {#each providers as item}
        <button
          type="button"
          class="provider-tab-btn"
          class:selected={provider === item.id}
          aria-pressed={provider === item.id}
          disabled={busy}
          onclick={() => void switchProvider(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </div>
    <div class="configuration-actions">
      <button
        type="button"
        class="ai-toggle-btn"
        class:active={showAiConfig}
        aria-expanded={showAiConfig}
        onclick={() => showAiConfig = !showAiConfig}
        title="配置用于 Skill 智能分析的 AI 模型 API"
      >
        <span class="ai-status-dot" class:configured={hasAiConfig}></span>
        <span class="ai-btn-icon">{@html icons.sparkle}</span>
        <span>AI 助手配置</span>
      </button>
    </div>
  </div>

  <AgentAiConfig {runtimeAvailable} bind:showAiConfig bind:hasAiConfig bind:error bind:notice />

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理本地文件。</div>
  {:else}
    {#if error}<p class="message error" role="alert">{error}</p>{/if}
    {#if notice}<p class="message success" role="status">{notice}</p>{/if}

    <div class="workspace">
      <AgentFileCatalog {instructions} {skills} {selected} {busy} {refresh} {select} {beginCreateInstruction} {beginImport} {beginCreate} />

      <section
        class="editor"
        class:is-editing={Boolean(selected && !importing && !creatingInstruction && !creating)}
        aria-label="文件编辑器"
      >
        {#if importing}
          <AgentSkillImport {provider} {providerRootOptions} bind:skillRoot bind:busy bind:error
            onInstall={(items) => { entries = items; importing = false; notice = "安装成功；选择左侧 skill 可查看或检查更新。"; }}
            onCancel={() => importing = false} />
        {:else if creatingInstruction || creating}
          <AgentCreatePanel
            mode={creatingInstruction ? 'instruction' : 'skill'}
            {provider}
            {busy}
            {instructionRootOptions}
            {instructionPresets}
            {instructionPresetOptions}
            {providerRootOptions}
            bind:instructionRoot
            bind:instructionPreset
            bind:instructionName
            bind:skillRoot
            bind:skillName
            onCreateInstruction={() => void createInstruction()}
            onCreateSkill={() => void createSkill()}
            onCancel={() => { creatingInstruction = false; creating = false; }}
          />

        {:else if selected}
          <div class="editor-head">
            <div class="editor-head-top">
              <div class="editor-title-group">
                <div class="editor-title-row">
                  <h2 class="editor-file-title" title={selected.id.name}>
                    {selected.builtin_description ? '/' : ''}{selected.id.name}{selected.kind === 'skill' && !selected.builtin_description ? ' / SKILL.md' : ''}
                  </h2>
                  <div class="editor-badges">
                    {#if selected.kind === 'instruction'}
                      {@const info = instructionInfo(selected, instructions)}
                      <span class={`instruction-badge ${info.tone}`} title={info.detail}>{info.label}</span>
                    {/if}
                    {#if selected.kind === 'skill'}
                      <span class="status-badge" class:disabled={!selected.enabled} class:enabled={selected.enabled}>
                        <span class="status-dot"></span>
                        {selected.enabled ? '已启用' : '已停用'}
                      </span>
                    {/if}
                    {#if selected.linked}
                      <span class="link-badge">软链接目标</span>
                    {/if}
                  </div>
                </div>

                <div class="editor-path-row">
                  <span class="editor-path" title={selected.path}>{selected.path}</span>
                  {#if selected.path}
                    <button
                      type="button"
                      class="copy-path-btn"
                      class:copied={copiedPath}
                      onclick={() => { if (selected) void copyPath(selected.path); }}
                      title="复制文件绝对路径"
                      aria-label={copiedPath ? "路径已复制" : "复制路径"}
                    >
                      {#if copiedPath}
                        <span class="copy-success" role="status">已复制</span>
                      {:else}
                        <span>复制路径</span>
                      {/if}
                    </button>
                  {/if}
                </div>
              </div>

              {#if selected.editable}
                <button
                  type="button"
                  class="btn-primary btn-save"
                  disabled={busy || (!dirty && selected.exists)}
                  onclick={() => void save()}
                >
                  {busy ? '处理中…' : selected.exists ? '保存更改' : '创建文件'}
                </button>
              {/if}
            </div>

            {#if selected.kind === 'instruction' && (selected.instructionEntry || !mainEntryForSelected)}
              {@const info = instructionInfo(selected, instructions)}
              <p class={`instruction-status-note ${info.tone}`}>{info.detail}</p>
            {/if}

            <div class="editor-actions">
              {#if selected.kind === 'skill' && selected.exists && !selected.builtin}
                <button type="button"
                  class="btn-secondary btn-action"
                  disabled={busy}
                  onclick={() => void toggleSkill()}
                  title={selected.enabled ? '停用此 Skill，Agent 将不再载入' : '启用此 Skill'}
                >
                  {selected.enabled ? '停用 Skill' : '启用 Skill'}
                </button>
                <button type="button"
                  class="btn-secondary btn-action ai-action-btn"
                  disabled={busy || analyzingSkill}
                  onclick={() => void generateSkillSummary()}
                  title="使用 AI 深度分析此 Skill，给出中文核心功能摘要与调用指南"
                >
                  {#if analyzingSkill}
                    <span class="spinner-small" aria-hidden="true"></span> 分析中…
                  {:else}
                    <span class="ai-sparkle-icon">{@html icons.sparkle}</span> AI 智能分析
                  {/if}
                </button>
                <button type="button"
                  class="btn-secondary btn-action"
                  disabled={busy || dirty}
                  onclick={() => void checkUpdate()}
                >
                  检查更新
                </button>
              {/if}

              {#if selected.exists && selected.editable}
                <button type="button"
                  class="btn-secondary btn-action"
                  disabled={busy}
                  onclick={() => void renameCustomization()}
                  title="重命名文件或 Skill 目录"
                >
                  改名
                </button>
              {/if}

              {#if selected.kind === 'instruction' && selected.exists}
                <button type="button"
                  class="btn-secondary btn-action btn-danger"
                  disabled={busy || selected.linked}
                  onclick={() => void deleteInstruction()}
                  title={selected.linked ? '符号链接文件请在原位置管理' : '删除此指令文件'}
                >
                  删除
                </button>
              {/if}
            </div>
          </div>

          {#if selected.kind === 'instruction' && !selected.instructionEntry && mainEntryForSelected}
            <AgentAuxReferenceBar
              {selected}
              {mainEntryForSelected}
              {availableMainEntries}
              bind:targetMainEntryName
              {isReferencedInMain}
              {checkingReference}
              {togglingReference}
              {busy}
              onToggleReference={() => void toggleMainReference()}
            />
          {/if}

          {#if updateInfo}
            <AgentSkillUpdates {selected} bind:updateInfo bind:busy bind:error bind:notice {dirty}
              onEntries={(items) => entries = items}
              onContent={(value) => { original = value; content = value ? normalizeLines(value) : ""; }} />
          {/if}

          {#if skillSummary && selected.kind === 'skill'}
            <AgentSkillSummaryCard
              {skillSummary}
              {analyzingSkill}
              bind:showSummaryCard
              onRegenerate={() => void generateSkillSummary()}
            />
          {/if}

          <AgentFileEditor bind:content {busy} {dirty} exists={selected.exists} readOnly={!selected.editable} />
        {:else}
          <div class="editor-empty">
            <div class="empty-icon-box">{@html icons.tools}</div>
            <h2>从文件列表选择指令或 Skill</h2>
          </div>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .custom-page {
    max-width: 1440px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: var(--page-heading-gap, 14px);
  }

  .custom-heading {
    margin-bottom: 0;
  }

  .configuration-actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
  }

  .ai-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 30px;
    padding: 0 10px;
    white-space: nowrap;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .ai-toggle-btn:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .ai-toggle-btn.active {
    color: var(--brand-text);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .ai-status-dot {
    width: 6px;
    height: 6px;
    flex: 0 0 6px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .ai-status-dot.configured {
    background: var(--color-exact);
  }

  .ai-status-dot.dirty {
    background: var(--color-estimated);
  }

  .ai-btn-icon {
    display: inline-flex;
    width: 14px;
    height: 14px;
  }

  .ai-btn-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .local-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    white-space: nowrap;
  }

  .local-badge :global(svg) {
    width: 12px;
    height: 12px;
    color: var(--color-exact);
  }

  .workspace-toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 4px 12px;
    padding: 5px 6px;
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
  }

  .provider-tabs {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }

  .provider-tab-btn {
    min-height: 30px;
    padding: 0 11px;
    border: 1px solid transparent;
    background: transparent;
    border-radius: var(--radius-sm);
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background-color var(--transition-fast), color var(--transition-fast), border-color var(--transition-fast), box-shadow var(--transition-fast);
    white-space: nowrap;
  }

  .provider-tab-btn:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--bg-secondary);
  }

  .provider-tab-btn:focus-visible {
    outline: 2px solid var(--brand-primary);
    outline-offset: 1px;
  }

  .provider-tab-btn.selected {
    background: var(--brand-subtle);
    color: var(--brand-text);
    border-color: var(--brand-border);
    font-weight: 600;
  }

  .provider-tab-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  /* Workspace Layout */
  .workspace {
    display: grid;
    grid-template-columns: clamp(232px, 30%, 304px) minmax(0, 1fr);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    min-height: 360px;
    overflow: hidden;
  }

  .editor {
    display: flex;
    min-width: 0;
    min-height: 0;
    flex-direction: column;
    overflow: auto;
    background: var(--bg-card);
  }

  .editor-head {
    display: flex;
    flex-direction: column;
    flex-shrink: 0;
    gap: 8px;
    padding: 12px 16px 10px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-card);
  }

  .editor-head-top {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 12px;
    min-width: 0;
  }

  .editor-title-group {
    flex: 1;
    min-width: 0;
  }

  .editor-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .editor-file-title {
    font-size: 14.5px;
    font-weight: 700;
    letter-spacing: -0.015em;
    color: var(--text-primary);
    margin: 0;
    overflow-wrap: anywhere;
    line-height: 1.3;
  }

  .editor-badges {
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 5px;
  }

  .editor-path-row {
    display: flex;
    align-items: center;
    gap: 6px;
    margin-top: 3px;
    min-width: 0;
  }

  .instruction-status-note {
    max-width: 70ch;
    margin: 0;
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 1.45;
  }

  .instruction-status-note.warning {
    color: var(--color-estimated);
  }

  .editor-path {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
    line-height: 1.4;
  }

  .copy-path-btn {
    display: inline-flex;
    align-items: center;
    height: 21px;
    padding: 0 7px;
    font: inherit;
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
    flex-shrink: 0;
    white-space: nowrap;
  }

  .copy-path-btn:hover {
    color: var(--text-primary);
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .copy-path-btn.copied {
    border-color: var(--color-exact-border);
    background: var(--color-exact-bg);
  }

  .copy-success {
    color: var(--color-exact);
    font-weight: 600;
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    flex-wrap: wrap;
  }

  .editor-actions:empty {
    display: none;
  }

  .btn-action {
    height: 26px;
    padding: 0 9px;
    font-size: 11.5px;
    font-weight: 500;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }

  .ai-action-btn {
    color: var(--brand-text);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .ai-action-btn:hover:not(:disabled) {
    background: var(--brand-primary);
    color: #fff;
  }

  .ai-sparkle-icon {
    display: inline-flex;
    width: 12px;
    height: 12px;
  }

  .ai-sparkle-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .btn-save {
    height: 29px;
    padding: 0 14px;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
    flex-shrink: 0;
  }

  .editor-empty {
    margin: auto;
    text-align: center;
    padding: 48px 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .empty-icon-box {
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .empty-icon-box :global(svg) {
    width: 20px;
    height: 20px;
  }

  .editor-empty h2 {
    font-size: 14px;
    color: var(--text-primary);
    margin: 0;
  }

  .message {
    font-size: 11.5px;
    padding: 8px 14px;
    border-radius: var(--radius-sm);
    margin: 0;
    overflow-wrap: anywhere;
    border: 1px solid var(--border-subtle);
  }

  .message.error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }

  .message.success {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
  }

  .fit-workspace {
    height: 100%;
  }

  .fit-workspace .workspace {
    flex: 1;
    grid-template-rows: minmax(0, 1fr);
  }

  .fit-workspace .editor.is-editing {
    overflow: hidden;
  }

  @media (max-height: 600px) {
    .fit-workspace .workspace {
      min-height: 300px;
    }

    .fit-workspace .editor.is-editing {
      overflow: auto;
    }
  }

  @media (max-width: 640px) {
    .workspace {
      grid-template-columns: minmax(0, 1fr);
    }

    .fit-workspace .workspace {
      grid-template-rows: auto minmax(260px, 1fr);
    }
  }
</style>
