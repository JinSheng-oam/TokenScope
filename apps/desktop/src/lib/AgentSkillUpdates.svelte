<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { Entry } from "./agentCustomizationEntries";
  import type { UpdateInfo } from "./agentSkillSourceTypes";

  export let selected: Entry;
  export let updateInfo: UpdateInfo | null;
  export let busy: boolean;
  export let dirty: boolean;
  export let error: string;
  export let notice: string;
  export let onEntries: (entries: Entry[]) => void;
  export let onContent: (value: string | null) => void;

  // Adopt state
  let adopting = false;
  let adoptSourceInput = '';
  let adoptingBusy = false;

  async function beginAdoptSkill() {
    if (selected.kind !== 'skill') return;
    adopting = true;
    error = '';
    notice = '';
    adoptSourceInput = '';
    try {
      const detected = await invoke<string | null>('detect_skill_source', { id: selected.id });
      if (detected) {
        adoptSourceInput = detected;
        notice = `已自动检测到 Git 来源: ${detected}，请确认后点击纳管。`;
      }
    } catch {
      // ignore
    }
  }

  async function confirmAdoptSkill() {
    if (!adoptSourceInput.trim()) {
      error = '请输入 GitHub 仓库或 skills.sh 来源地址';
      return;
    }
    adoptingBusy = true;
    error = '';
    notice = '';
    try {
      const info = await invoke<UpdateInfo>('adopt_skill_source', {
        id: selected.id,
        sourceInput: adoptSourceInput.trim(),
        candidatePath: null,
      });
      updateInfo = info;
      adopting = false;
      notice = `纳管成功！来源已绑定，当前状态：${info.detail}`;
      onEntries(await invoke<Entry[]>('list_customizations', { provider: selected.id.provider }));
    } catch (e) {
      error = String(e);
    } finally {
      adoptingBusy = false;
    }
  }

  async function applyUpdate() {
    if (!updateInfo || dirty) return;
    const msg = updateInfo.kind === 'manager' ? 'Skills Manager 更新可能覆盖该 skill 的本地修改，也会影响引用同一目标的其他工具。继续更新？' : '将从来源更新此 skill；更新前会再次检查本地文件是否被修改。继续？';
    if (!window.confirm(msg)) return;
    busy = true; error = ''; notice = '';
    try {
      notice = await invoke<string>('update_skill_source', { id: selected.id });
      const value = await invoke<string | null>('read_customization', { id: selected.id });
      onContent(value);
      try {
        onEntries(await invoke<Entry[]>('list_customizations', { provider: selected.id.provider }));
      } finally {
        updateInfo = null;
      }
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

</script>

{#if updateInfo}
  <div class="update-panel">
    <div class="update-info-text">
      <strong>更新状态：</strong>{updateInfo.detail}
      {#if updateInfo.source} · <span>{updateInfo.source}</span>{/if}
    </div>
    <div class="update-actions">
      {#if updateInfo.status === 'unsupported' && selected.kind === 'skill'}
        <button class="btn-primary btn-compact" disabled={busy} onclick={() => void beginAdoptSkill()}>
          纳入更新管理
        </button>
      {/if}
      {#if updateInfo.kind === 'manager' || updateInfo.status === 'available'}
        <button class="btn-secondary btn-compact" disabled={busy} onclick={() => void applyUpdate()}>
          更新 skill
        </button>
      {/if}
    </div>
  </div>
{/if}

{#if adopting}
  <div class="adopt-panel">
    <div class="adopt-title">
      <strong>🔗 纳入更新管理 (绑定来源仓库)</strong>
      <button class="btn-compact" onclick={() => adopting = false}>✕</button>
    </div>
    <div class="adopt-form">
      <label for="adopt-source">GitHub 仓库或 skills.sh 地址：</label>
      <input
        id="adopt-source"
        bind:value={adoptSourceInput}
        placeholder="https://github.com/owner/repo"
      />
      <div class="adopt-buttons">
        <button
          class="btn-primary"
          disabled={adoptingBusy || !adoptSourceInput.trim()}
          onclick={() => void confirmAdoptSkill()}
        >
          {adoptingBusy ? '正在拉取验证并绑定…' : '确认纳管'}
        </button>
        <button class="btn-secondary" onclick={() => adopting = false}>取消</button>
      </div>
    </div>
  </div>
{/if}

<style>
  /* Update and Adopt Panels */
  .update-panel {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 8px 18px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--brand-subtle);
    color: var(--brand-strong);
    font-size: 11.5px;
  }

  .update-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .adopt-panel {
    padding: 12px 18px;
    background: rgba(245, 158, 11, 0.06);
    border-bottom: 1px solid rgba(245, 158, 11, 0.2);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }

  .adopt-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #d97706;
  }

  .adopt-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 520px;
    margin-top: 4px;
  }

  .adopt-form input {
    height: 30px;
    padding: 0 8px;
    font: inherit;
    font-size: 12px;
    background: var(--bg-card);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
  }

  .adopt-buttons {
    display: flex;
    gap: 8px;
  }

  .btn-compact {
    min-height: 22px;
    padding: 0 8px;
    font-size: 10.5px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-compact:hover {
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

</style>
