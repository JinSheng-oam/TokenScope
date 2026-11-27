<script lang="ts">
  export let rows: number;
  export let paginating = false;

  const titleWidths = [46, 62, 38, 54, 42, 58, 48, 52, 40, 56];
  const metaWidths = [34, 42, 30, 38, 32, 40, 36, 34, 28, 38];
  const barWidths = [86, 64, 52, 74, 44, 68, 38, 58, 32, 48];

  $: clampedRows = Math.max(4, Math.min(10, rows || 8));
  $: firstGroupCount = Math.min(4, Math.ceil(clampedRows / 2));
  $: secondGroupCount = Math.max(0, clampedRows - firstGroupCount);
  $: groups = [
    { key: 'g0', pillWidth: '72px', start: 0, count: firstGroupCount },
    ...(secondGroupCount > 0
      ? [{ key: 'g1', pillWidth: '88px', start: firstGroupCount, count: secondGroupCount }]
      : []),
  ];
</script>

<div
  class="session-loading-shell"
  role="status"
  aria-live="polite"
  aria-label={paginating ? '正在加载会话页面' : '正在加载会话'}
>
  <div class="sk-sort-bar" aria-hidden="true">
    <span class="sk-bone sk-sort-label"></span>
    <span class="sk-bone sk-sort-chip w1"></span>
    <span class="sk-bone sk-sort-chip w2"></span>
    <span class="sk-bone sk-sort-chip w3"></span>
    <span class="sk-bone sk-sort-chip w2"></span>
  </div>

  <div class="session-loading-timeline" aria-hidden="true">
    <div class="session-loading-bar"></div>
    {#each groups as grp (grp.key)}
      <div class="sk-day-group">
        <div class="sk-day-head" style={`--delay: ${grp.start * 45}ms`}>
          <span class="sk-bone sk-day-pill" style={`width: ${grp.pillWidth}`}></span>
        </div>
        <ul class="sk-day-list">
          {#each Array(grp.count) as _, offset}
            {@const idx = grp.start + offset}
            <li class="sk-timeline-item" style={`--delay: ${idx * 45}ms`}>
              <div class="sk-entry-grid">
                <div class="sk-entry-main">
                  <span class="sk-bone sk-entry-title" style={`width: ${titleWidths[idx % titleWidths.length]}%`}></span>
                  <span class="sk-bone sk-entry-meta" style={`width: ${metaWidths[idx % metaWidths.length]}%`}></span>
                </div>
                <div class="sk-entry-tokens">
                  <span class="sk-bone sk-entry-num"></span>
                  <span class="sk-bone sk-entry-bar" style={`width: ${barWidths[idx % barWidths.length]}%`}></span>
                </div>
                <div class="sk-entry-rounds">
                  <span class="sk-bone sk-entry-round-val"></span>
                  <span class="sk-bone sk-entry-round-sub"></span>
                </div>
                <span class="sk-bone sk-entry-chevron"></span>
              </div>
            </li>
          {/each}
        </ul>
      </div>
    {/each}
  </div>
</div>

<style>
  .session-loading-shell {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .sk-sort-bar {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 8px;
    min-height: 25px;
  }

  .sk-sort-label {
    width: 34px;
    height: 12px;
    border-radius: 4px;
  }

  .sk-sort-chip {
    height: 22px;
    border-radius: 999px;
  }

  .sk-sort-chip.w1 { width: 58px; }
  .sk-sort-chip.w2 { width: 68px; }
  .sk-sort-chip.w3 { width: 52px; }

  .session-loading-timeline {
    position: relative;
    border-top: 1px solid var(--border-subtle);
    margin-top: 8px;
    padding-top: 2px;
  }

  .session-loading-bar {
    position: absolute;
    top: -1px;
    left: 0;
    right: 0;
    height: 2px;
    overflow: hidden;
    background: transparent;
  }

  .session-loading-bar::after {
    content: '';
    position: absolute;
    inset: 0 auto 0 0;
    width: 34%;
    border-radius: 999px;
    background: linear-gradient(90deg, transparent 0%, var(--brand-primary) 50%, transparent 100%);
    animation: session-progress-slide 1.2s cubic-bezier(0.4, 0, 0.2, 1) infinite;
  }

  @keyframes session-progress-slide {
    0% { transform: translateX(-110%); }
    100% { transform: translateX(320%); }
  }

  @keyframes session-sk-enter {
    from {
      opacity: 0;
      transform: translateY(4px);
    }
    to {
      opacity: 1;
      transform: translateY(0);
    }
  }

  .sk-day-group {
    padding-top: 6px;
  }

  .sk-day-head {
    margin-bottom: 4px;
    animation: session-sk-enter 220ms ease both;
    animation-delay: var(--delay, 0ms);
  }

  .sk-day-pill {
    height: 20px;
    border-radius: 999px;
  }

  .sk-day-list {
    list-style: none;
    margin: 0;
    padding: 0;
    margin-inline-start: 14px;
    padding-inline-start: 11px;
    border-inline-start: 1px solid var(--border-subtle);
  }

  .sk-timeline-item {
    position: relative;
    border-bottom: 1px solid var(--border-subtle);
    animation: session-sk-enter 220ms ease both;
    animation-delay: var(--delay, 0ms);
  }

  .sk-timeline-item::before {
    content: '';
    position: absolute;
    inset-inline-start: -16px;
    top: 20px;
    width: 7px;
    height: 7px;
    border: 1px solid var(--border-strong);
    border-radius: 50%;
    background: var(--bg-canvas);
  }

  .sk-entry-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(116px, 19%) minmax(96px, 13%) 18px;
    gap: 16px;
    align-items: center;
    padding: 7px 9px 7px 12px;
    min-height: 46px;
  }

  .sk-entry-main {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
  }

  .sk-entry-title {
    height: 13px;
    border-radius: 5px;
  }

  .sk-entry-meta {
    height: 10px;
    border-radius: 4px;
    opacity: 0.78;
  }

  .sk-entry-tokens {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 6px;
  }

  .sk-entry-num {
    width: 74px;
    height: 13px;
    border-radius: 5px;
  }

  .sk-entry-bar {
    height: 4px;
    border-radius: 999px;
    opacity: 0.75;
  }

  .sk-entry-rounds {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 5px;
  }

  .sk-entry-round-val {
    width: 44px;
    height: 12px;
    border-radius: 4px;
  }

  .sk-entry-round-sub {
    width: 56px;
    height: 9px;
    border-radius: 4px;
    opacity: 0.7;
  }

  .sk-entry-chevron {
    width: 12px;
    height: 12px;
    border-radius: 4px;
    justify-self: end;
    opacity: 0.55;
  }

  @container (max-width: 640px) {
    .sk-entry-grid {
      grid-template-columns: minmax(0, 1fr) auto 18px;
      gap: 10px;
    }

    .sk-entry-rounds {
      display: none;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .session-loading-bar::after,
    .sk-day-head,
    .sk-timeline-item {
      animation: none;
    }
  }
</style>
