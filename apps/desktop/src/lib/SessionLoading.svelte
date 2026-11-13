<script lang="ts">
  import LoadingStatus from './LoadingStatus.svelte';
  export let rows: number;
  export let paginating = false;

  const widths = [42, 57, 33, 51, 38, 46, 54];
</script>

<div class="session-loading-preview" role="status">
  <LoadingStatus message={paginating ? '正在加载会话页面…' : '正在加载会话…'} />
  <div class="loading-skeleton-panel" aria-hidden="true">
    <div class="sk-session-list">
      {#each widths.slice(0, rows) as width, index}
        <div class="sk-session-row" style:--delay={index * 70 + 'ms'}>
          <div class="sk-stack">
            <span class="sk-bone sk-title" style:width={width + '%'}></span>
            <span class="sk-bone sk-foot" style:width="29%"></span>
          </div>
          <span class="sk-bone sk-title" style:width="82px"></span>
          <span class="sk-bone sk-title" style:width="48px"></span>
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .session-loading-preview { display: grid; gap: 10px; padding-block: 6px; animation: loading-fade-in 180ms ease-out; }
  .sk-session-row { min-height: 46px; }
  .sk-stack { flex: 1; min-width: 0; }
  @media (prefers-reduced-motion: reduce) {
    .session-loading-preview { animation: none; }
  }
</style>
