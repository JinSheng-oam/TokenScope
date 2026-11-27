<script lang="ts">
  import {
    HEATMAP_COLOR_PRESETS,
    HEATMAP_RANGE_OPTIONS,
    heatmapConfig,
    resetHeatmapConfig,
    resolveHeatmapPaletteStyle,
    updateHeatmapConfig,
    type HeatmapColorScheme,
    type HeatmapRangeMode,
    type HeatmapThresholdMode,
  } from './heatmapConfig';

  export let todayIso = '';
  export let onEnsureHeatmapMode: () => void = () => {};

  function setRange(range: HeatmapRangeMode) {
    onEnsureHeatmapMode();
    if (range === 'custom') {
      const end = $heatmapConfig.customEnd || todayIso;
      let start = $heatmapConfig.customStart;
      if (!start && end) {
        const d = new Date(`${end}T00:00:00`);
        d.setDate(d.getDate() - 83);
        const y = d.getFullYear();
        const m = String(d.getMonth() + 1).padStart(2, '0');
        const day = String(d.getDate()).padStart(2, '0');
        start = `${y}-${m}-${day}`;
      }
      updateHeatmapConfig({ range, customStart: start, customEnd: end });
      return;
    }
    updateHeatmapConfig({ range });
  }

  function setThresholdMode(thresholdMode: HeatmapThresholdMode) {
    onEnsureHeatmapMode();
    updateHeatmapConfig({ thresholdMode });
  }

  function setCustomThreshold(index: 0 | 1 | 2, rawValue: string) {
    const parsed = Number(rawValue);
    if (!Number.isFinite(parsed) || parsed <= 0) return;
    onEnsureHeatmapMode();
    const next: [number, number, number] = [...$heatmapConfig.customThresholdsM] as [number, number, number];
    next[index] = parsed;
    updateHeatmapConfig({ thresholdMode: 'custom', customThresholdsM: next });
  }

  function setColorScheme(colorScheme: HeatmapColorScheme) {
    onEnsureHeatmapMode();
    updateHeatmapConfig({ colorScheme });
  }

  function setCustomColor(color: string) {
    onEnsureHeatmapMode();
    updateHeatmapConfig({ colorScheme: 'custom', customColor: color });
  }

  $: previewStyle = resolveHeatmapPaletteStyle($heatmapConfig);
</script>

<div class="hm-settings-panel" role="region" aria-label="热力图设置">
  <div class="hm-settings-row">
    <span class="hm-settings-label">热力图范围</span>
    <div class="card-size-group" role="group" aria-label="热力图日期范围">
      {#each HEATMAP_RANGE_OPTIONS as opt (opt.value)}
        <button
          type="button"
          class="size-pill"
          class:active={$heatmapConfig.range === opt.value}
          aria-pressed={$heatmapConfig.range === opt.value}
          onclick={() => setRange(opt.value)}
        >{opt.label}</button>
      {/each}
    </div>

    {#if $heatmapConfig.range === 'custom'}
      <div class="hm-date-range">
        <input
          type="date"
          class="hm-date-input"
          aria-label="热力图开始日期"
          max={$heatmapConfig.customEnd || todayIso || undefined}
          value={$heatmapConfig.customStart}
          onchange={(e) => {
            onEnsureHeatmapMode();
            updateHeatmapConfig({ range: 'custom', customStart: e.currentTarget.value });
          }}
        />
        <span class="hm-sep">至</span>
        <input
          type="date"
          class="hm-date-input"
          aria-label="热力图结束日期"
          min={$heatmapConfig.customStart || undefined}
          max={todayIso || undefined}
          value={$heatmapConfig.customEnd}
          onchange={(e) => {
            onEnsureHeatmapMode();
            updateHeatmapConfig({ range: 'custom', customEnd: e.currentTarget.value });
          }}
        />
      </div>
    {/if}

    <button
      type="button"
      class="hm-reset-btn"
      title="恢复热力图默认设置"
      onclick={() => {
        onEnsureHeatmapMode();
        resetHeatmapConfig();
      }}
    >重置</button>
  </div>

  <div class="hm-settings-row">
    <span class="hm-settings-label">少–多阈值</span>
    <div class="card-size-group" role="group" aria-label="热力图阈值模式">
      <button
        type="button"
        class="size-pill"
        class:active={$heatmapConfig.thresholdMode === 'auto'}
        aria-pressed={$heatmapConfig.thresholdMode === 'auto'}
        onclick={() => setThresholdMode('auto')}
      >智能</button>
      <button
        type="button"
        class="size-pill"
        class:active={$heatmapConfig.thresholdMode === 'custom'}
        aria-pressed={$heatmapConfig.thresholdMode === 'custom'}
        onclick={() => setThresholdMode('custom')}
      >自定义 (M)</button>
    </div>

    {#if $heatmapConfig.thresholdMode === 'custom'}
      <div class="hm-threshold-inputs">
        <label class="hm-threshold-field" title="第 2 级阈值（百万 Token）">
          <span>中浅≥</span>
          <input
            type="number"
            min="0.1"
            step="any"
            value={$heatmapConfig.customThresholdsM[0]}
            onchange={(e) => setCustomThreshold(0, e.currentTarget.value)}
          />
          <span>M</span>
        </label>
        <label class="hm-threshold-field" title="第 3 级阈值（百万 Token）">
          <span>中深≥</span>
          <input
            type="number"
            min="0.1"
            step="any"
            value={$heatmapConfig.customThresholdsM[1]}
            onchange={(e) => setCustomThreshold(1, e.currentTarget.value)}
          />
          <span>M</span>
        </label>
        <label class="hm-threshold-field" title="第 4 级阈值（百万 Token）">
          <span>最深≥</span>
          <input
            type="number"
            min="0.1"
            step="any"
            value={$heatmapConfig.customThresholdsM[2]}
            onchange={(e) => setCustomThreshold(2, e.currentTarget.value)}
          />
          <span>M</span>
        </label>
      </div>
    {/if}

    <div class="hm-color-group">
      <span class="hm-settings-label">配色</span>
      {#each HEATMAP_COLOR_PRESETS as preset (preset.id)}
        <button
          type="button"
          class="hm-color-dot"
          class:active={$heatmapConfig.colorScheme === preset.id}
          style={`--dot-color: ${preset.color};`}
          title={preset.label}
          aria-label={preset.label}
          aria-pressed={$heatmapConfig.colorScheme === preset.id}
          onclick={() => setColorScheme(preset.id)}
        ></button>
      {/each}
      <label
        class="hm-color-picker"
        class:active={$heatmapConfig.colorScheme === 'custom'}
        title="自定义颜色"
      >
        <input
          type="color"
          aria-label="自定义热力图颜色"
          value={$heatmapConfig.customColor}
          oninput={(e) => setCustomColor(e.currentTarget.value)}
        />
        <span class="hm-color-picker-swatch" style={`background: ${$heatmapConfig.customColor};`}></span>
      </label>
      <span class="hm-scale-preview" style={previewStyle} aria-hidden="true">
        <span class="sw s0"></span>
        <span class="sw s1"></span>
        <span class="sw s2"></span>
        <span class="sw s3"></span>
        <span class="sw s4"></span>
      </span>
    </div>
  </div>
</div>

<style>
  .hm-settings-panel {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 8px 10px;
    margin-bottom: 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    background: var(--bg-secondary);
    font-size: 11.5px;
  }

  .hm-settings-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 8px 10px;
  }

  .hm-settings-label {
    color: var(--text-muted);
    font-weight: 550;
    white-space: nowrap;
  }

  .hm-date-range {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  .hm-date-input {
    height: 24px;
    padding: 0 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    color: var(--text-primary);
    font-size: 11px;
    font-family: var(--font-mono);
  }

  .hm-sep {
    color: var(--text-muted);
  }

  .hm-reset-btn {
    margin-left: auto;
    height: 24px;
    padding: 0 8px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    color: var(--text-secondary);
    font-size: 11px;
    cursor: pointer;
    transition: border-color var(--transition-fast), color var(--transition-fast);
  }

  .hm-reset-btn:hover {
    border-color: var(--border-strong);
    color: var(--text-primary);
  }

  .hm-threshold-inputs {
    display: inline-flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px;
  }

  .hm-threshold-field {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 1px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-card);
    color: var(--text-muted);
    font-size: 11px;
  }

  .hm-threshold-field input {
    width: 44px;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--text-primary);
    font-size: 11.5px;
    font-family: var(--font-mono);
    text-align: right;
  }

  .hm-color-group {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    margin-left: auto;
  }

  .hm-color-dot {
    width: 16px;
    height: 16px;
    padding: 0;
    border-radius: 999px;
    border: 2px solid transparent;
    background: var(--dot-color);
    cursor: pointer;
    transition: transform 100ms ease, box-shadow 100ms ease;
  }

  .hm-color-dot:hover {
    transform: scale(1.12);
  }

  .hm-color-dot.active {
    box-shadow: 0 0 0 2px var(--bg-secondary), 0 0 0 3.5px var(--dot-color);
  }

  .hm-color-picker {
    position: relative;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 18px;
    height: 18px;
    border-radius: 999px;
    border: 1px dashed var(--border-strong);
    cursor: pointer;
    overflow: hidden;
  }

  .hm-color-picker.active {
    border-style: solid;
    border-color: var(--brand-primary);
  }

  .hm-color-picker input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
    width: 100%;
    height: 100%;
  }

  .hm-color-picker-swatch {
    width: 12px;
    height: 12px;
    border-radius: 999px;
  }

  .hm-scale-preview {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin-left: 2px;
  }

  .hm-scale-preview .sw {
    width: 9px;
    height: 9px;
    border-radius: 2px;
    border: 1px solid rgba(128, 128, 128, 0.14);
  }

  .hm-scale-preview .s0 { background: var(--heatmap-0); }
  .hm-scale-preview .s1 { background: var(--heatmap-1); }
  .hm-scale-preview .s2 { background: var(--heatmap-2); }
  .hm-scale-preview .s3 { background: var(--heatmap-3); }
  .hm-scale-preview .s4 { background: var(--heatmap-4); }
</style>
