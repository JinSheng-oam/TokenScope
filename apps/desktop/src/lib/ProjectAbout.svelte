<script lang="ts">
  import { onMount } from 'svelte';
  import { getVersion } from '@tauri-apps/api/app';
  import { invoke } from '@tauri-apps/api/core';
  import { icons } from './icons';

  export let runtimeAvailable: boolean;

  type ProjectUpdate = { latest_version: string; update_available: boolean };
  const repository = 'https://github.com/JinSheng-oam/TokenScope';
  let version = '';
  let checking = false;
  let result: ProjectUpdate | null = null;
  let noRelease = false;
  let checkError = '';
  let linkError = '';

  onMount(() => {
    if (runtimeAvailable) void getVersion().then((value) => (version = value)).catch(() => {});
  });

  async function checkUpdates() {
    if (!runtimeAvailable || checking) return;
    checking = true;
    result = null;
    noRelease = false;
    checkError = '';
    try {
      result = await invoke<ProjectUpdate | null>('check_project_update');
      noRelease = result === null;
    } catch (error) {
      checkError = error instanceof Error ? error.message : String(error);
    } finally {
      checking = false;
    }
  }

  async function openPage(event: MouseEvent, releases: boolean) {
    if (!runtimeAvailable) return;
    event.preventDefault();
    linkError = '';
    try {
      await invoke('open_project_page', { releases });
    } catch (error) {
      linkError = error instanceof Error ? error.message : String(error);
    }
  }
</script>

<section class="panel-card about-card" aria-labelledby="project-about-title">
  <div class="panel-head">
    <div>
      <h2 id="project-about-title">关于项目</h2>
      <p>TokenScope</p>
    </div>
    <button type="button" class="btn-secondary" disabled={checking || !runtimeAvailable} onclick={checkUpdates}>
      {@html icons.refresh}
      {checking ? '检查中…' : '检查更新'}
    </button>
  </div>

  <dl class="project-details">
    <div><dt>当前版本</dt><dd class="mono">{version ? 'v' + version : '—'}</dd></div>
    <div><dt>作者</dt><dd>JinSheng-oam</dd></div>
    <div>
      <dt>GitHub</dt>
      <dd><a href={repository} target="_blank" rel="noopener noreferrer" onclick={(event) => openPage(event, false)}>JinSheng-oam/TokenScope ↗</a></dd>
    </div>
  </dl>

  <div class="release-row">
    <span role="status" class:failed={Boolean(checkError)}>
      {#if checking}正在查询 GitHub 正式发布…
      {:else if checkError}{checkError}
      {:else if noRelease}GitHub 尚未发布正式版本
      {:else if result?.update_available}发现新版本 v{result.latest_version}
      {:else if result}已是最新版本（GitHub 正式版 v{result.latest_version}）
      {/if}
    </span>
    <a href={repository + '/releases'} target="_blank" rel="noopener noreferrer" onclick={(event) => openPage(event, true)}>查看发布页 ↗</a>
  </div>
  {#if linkError}<p role="alert" class="failed">{linkError}</p>{/if}
</section>

<style>
  .about-card { margin: 0; }
  .project-details {
    display: flex;
    flex-wrap: wrap;
    gap: 12px 28px;
    margin: 0;
  }
  dt, .release-row {
    font-size: 12px;
    color: var(--text-muted);
  }
  dd {
    margin: 4px 0 0;
    font-size: 13px;
    color: var(--text-primary);
  }
  a {
    color: var(--brand-primary);
    text-underline-offset: 3px;
  }
  .release-row {
    display: flex;
    align-items: baseline;
    flex-wrap: wrap;
    gap: 6px 14px;
    padding-top: 12px;
    margin-top: 12px;
    border-top: 1px solid var(--border-subtle);
  }
  .release-row span:empty { display: none; }
  .failed { color: var(--color-danger); }
</style>
