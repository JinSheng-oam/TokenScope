<script lang="ts">
  import CustomSelect from './CustomSelect.svelte';
  import type { Entry, Provider } from './agentCustomizationEntries';

  export let mode: 'instruction' | 'skill';
  export let provider: Provider;
  export let busy = false;
  export let instructionRootOptions: { key: string; label: string }[] = [];
  export let instructionPresets: Entry[] = [];
  export let instructionPresetOptions: { key: string; label: string }[] = [];
  export let providerRootOptions: { key: string; label: string }[] = [];
  export let instructionRoot = '';
  export let instructionPreset = '';
  export let instructionName = '';
  export let skillRoot = '';
  export let skillName = '';
  export let onCreateInstruction: () => void = () => {};
  export let onCreateSkill: () => void = () => {};
  export let onCancel: () => void = () => {};

  function instructionDraftHelp(currentProvider: Provider): string {
    if (currentProvider === 'codex') {
      return 'Codex 全局入口为 AGENTS.override.md（优先）或 AGENTS.md；其他文件名只是辅助文件，不会仅因位于此目录就自动加载。';
    }
    if (currentProvider === 'pi') return 'Pi 全局上下文优先读取 AGENTS.override.md，其次 AGENTS.md 或 CLAUDE.md；SYSTEM.md 和 APPEND_SYSTEM.md 单独管理系统提示词。';
    return 'TokenScope 会标出工具预设的入口文件；自定义 Markdown 是否自动加载取决于对应工具，必要时请从入口文件引用。';
  }
</script>

{#if mode === 'instruction'}
  <div class="editor-head">
    <div>
      <h2>新建指令文件草稿</h2>
      <p>辅助 Markdown 需要由入口文件引用。</p>
    </div>
  </div>
  <div class="create-form">
    {#if instructionRootOptions.length > 1}
      <span>保存目录</span>
      <CustomSelect
        bind:value={instructionRoot}
        options={instructionRootOptions}
        ariaLabel="选择指令文件保存目录"
        onChange={() => { instructionName = ''; instructionPreset = ''; }}
      />
    {:else if instructionRootOptions.length === 1}
      <div class="create-form-location">
        <span>保存目录</span>
        <strong>{instructionRootOptions[0].label}</strong>
      </div>
    {/if}
    {#if instructionPresets.length > 0}
      <span>入口预设</span>
      <CustomSelect
        bind:value={instructionPreset}
        options={instructionPresetOptions}
        placeholder="选择预设入口"
        ariaLabel="选择指令入口预设"
        onChange={(value) => instructionName = value}
      />
    {/if}
    <label for="instr-name">文件名</label>
    <input
      id="instr-name"
      bind:value={instructionName}
      oninput={() => instructionPreset = ''}
      placeholder="选择入口预设或输入自定义 .md 文件名"
      maxlength="64"
    />
    <p class="source-hint">{instructionDraftHelp(provider)}</p>
    <div class="form-btn-row">
      <button class="btn-primary" disabled={busy || !instructionName.trim()} onclick={onCreateInstruction}>开始编辑草稿</button>
      <button class="btn-secondary" onclick={onCancel}>取消</button>
    </div>
  </div>
{:else}
  <div class="editor-head">
    <div>
      <h2>新建 skill</h2>
    </div>
  </div>
  <div class="create-form">
    <span>保存位置</span>
    <CustomSelect bind:value={skillRoot} options={providerRootOptions} ariaLabel="选择 Skill 保存位置" />
    <label for="skill-name">名称</label>
    <input id="skill-name" bind:value={skillName} placeholder="例如 code-review" maxlength="64" />
    <div class="form-btn-row">
      <button class="btn-primary" disabled={busy} onclick={onCreateSkill}>开始编辑</button>
      <button class="btn-secondary" onclick={onCancel}>取消</button>
    </div>
  </div>
{/if}

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

  .create-form-location {
    min-height: 34px;
    padding: 0 10px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .create-form-location strong {
    color: var(--text-primary);
    font-weight: 500;
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
</style>
