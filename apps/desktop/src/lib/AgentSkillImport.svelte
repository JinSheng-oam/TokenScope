<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import CustomSelect from "./CustomSelect.svelte";
  import type { Inspection } from "./agentSkillSourceTypes";
  import type { Entry, Provider } from "./agentCustomizationEntries";

  export let provider: Provider;
  export let providerRootOptions: {key: string; label: string}[];
  export let skillRoot: string;
  export let busy: boolean;
  export let error: string;
  export let onInstall: (entries: Entry[]) => void;
  export let onCancel: () => void;

  let skillName = "";
  let sourceInput = '';
  let inspection: Inspection | null = null;
  let candidatePath = '';
  $: sourceSkillOptions = (inspection?.candidates ?? []).map((item) => ({
    key: item.path,
    label: item.name,
    subtext: item.path || '仓库根目录',
  }));

  async function inspectSource() {
    busy = true; error = ''; inspection = null;
    try {
      inspection = await invoke<Inspection>('inspect_skill_source', { input: sourceInput });
      candidatePath = inspection.candidates[0].path;
      skillName = inspection.candidates[0].name;
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  function chooseCandidate() { skillName = inspection?.candidates.find((c) => c.path === candidatePath)?.name ?? ''; }

  async function installSource() {
    if (!inspection || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillName) || skillName.length > 64) { error = '名称使用小写字母、数字和连字符，最长 64 个字符。'; return; }
    busy = true; error = '';
    try {
      await invoke('install_skill_source', { id: { provider, root: skillRoot, name: skillName }, source: inspection.source, revision: inspection.revision, skillPath: candidatePath });
      onInstall(await invoke<Entry[]>('list_customizations', { provider }));
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

</script>

<div class="editor-head">
  <div>
    <h2>从来源添加 skill</h2>
    <p>支持 skills.sh 和 GitHub 链接。</p>
  </div>
</div>
<div class="create-form">
  <label for="source-url">来源地址</label>
  <input id="source-url" bind:value={sourceInput} oninput={() => inspection = null} placeholder="https://skills.sh/owner/repo/skill 或 GitHub URL" />
  <div><button class="btn-secondary" disabled={busy || !sourceInput.trim()} onclick={() => void inspectSource()}>{busy ? '读取中…' : '预览来源'}</button></div>
  {#if inspection}
    <span>选择 skill</span>
    <CustomSelect
      bind:value={candidatePath}
      options={sourceSkillOptions}
      ariaLabel="选择要安装的 Skill"
      onChange={() => chooseCandidate()}
    />
    <details><summary>查看 SKILL.md 预览</summary><pre>{inspection.candidates.find((c) => c.path === candidatePath)?.preview}</pre></details>
    <span>安装位置</span>
    <CustomSelect bind:value={skillRoot} options={providerRootOptions} ariaLabel="选择 Skill 安装位置" />
    <label for="import-name">安装名称</label>
    <input id="import-name" bind:value={skillName} maxlength="64" />
    <p class="source-hint">{inspection.source.url} · {inspection.revision.slice(0, 12)}。请确认来源可信，skill 可能包含脚本。</p>
    <div class="form-btn-row">
      <button class="btn-primary" disabled={busy} onclick={() => void installSource()}>安装选中 skill</button>
      <button class="btn-secondary" onclick={onCancel}>取消</button>
    </div>
  {:else}
    <div><button class="btn-secondary" onclick={onCancel}>取消</button></div>
  {/if}
</div>

<style>
  .editor-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-card);
  }

  /* Forms & Create/Import Panels */
  .create-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 520px;
    padding: 20px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .create-form :global(.select-dropdown) {
    max-width: none;
  }

  .create-form input {
    height: 32px;
    padding: 0 10px;
    font: inherit;
    font-size: 12px;
    color: var(--text-primary);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
  }

  .create-form input:focus {
    border-color: var(--brand-primary);
    outline: none;
  }

  .form-btn-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .source-hint {
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.5;
    margin: 4px 0;
  }

  details {
    color: var(--text-secondary);
    font-size: 11.5px;
  }

  summary {
    cursor: pointer;
    color: var(--brand-strong);
  }

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

</style>
