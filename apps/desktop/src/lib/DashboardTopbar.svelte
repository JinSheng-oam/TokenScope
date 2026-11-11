<script lang="ts">
  import DateRangePicker from "./DateRangePicker.svelte";
  import { formatCompactTokens, type DeviceScope } from "./dashboardView";
  import { icons } from "./icons";
  import type { DashboardSnapshot, DashboardRange } from "./types";

  export let activeTabLabel: string;
  export let showControls = true;
  export let deviceScope: DeviceScope;
  export let deviceScopeLabel: string;
  export let selectableRemoteDevices: { device_id: string; display_name: string }[];
  export let snapshot: DashboardSnapshot | null;
  export let range: DashboardRange;
  export let customStart: string;
  export let customEnd: string;
  export let todayIso: string;
  export let refreshing: boolean;
  export let selectDeviceScope: (scope: DeviceScope) => void;
  export let handleSelectRange: (range: Exclude<DashboardRange, "custom">) => void;
  export let applyCustomRange: (selection: { start: string; end: string }) => void;
  export let loadDashboard: () => Promise<void>;
  export let isEditingOverviewLayout: boolean = false;
  export let onToggleOverviewLayout: () => void = () => {};

  let customOpen = false;
  let deviceMenuOpen = false;

  function chooseDevice(scope: DeviceScope) {
    deviceMenuOpen = false;
    selectDeviceScope(scope);
  }
</script>

<svelte:window
  onmousedown={(e) => {
    if (deviceMenuOpen && !(e.target as Element | null)?.closest('.device-switcher-wrap')) {
      deviceMenuOpen = false;
    }
  }}
  onkeydown={(e) => {
    if (e.key === 'Escape' && deviceMenuOpen) {
      deviceMenuOpen = false;
    }
  }}
/>

<header class="topbar">
  <div class="breadcrumbs">
    <span>TokenScope</span>
    <span class="sep">/</span>
    <strong>{activeTabLabel}</strong>
  </div>

  {#if showControls}
    <div class="topbar-actions">
      <div class="device-switcher-wrap">
        <button
          type="button"
          class="machine-capsule interactive"
          class:active={deviceMenuOpen || deviceScope !== 'local'}
          title="切换设备数据统计范围（可汇总全部设备或单看某台设备）"
          aria-haspopup="listbox"
          aria-expanded={deviceMenuOpen}
          onclick={() => (deviceMenuOpen = !deviceMenuOpen)}
        >
          {@html icons.cpu}
          <span>{deviceScopeLabel}</span>
          <svg class="capsule-chevron" class:rotated={deviceMenuOpen} viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
            <polyline points="6 9 12 15 18 9" />
          </svg>
        </button>

        {#if deviceMenuOpen}
          <div class="device-scope-dropdown" role="listbox" aria-label="选择统计设备范围">
            <button
              type="button"
              class="device-scope-option"
              class:selected={deviceScope === 'all'}
              role="option"
              aria-selected={deviceScope === 'all'}
              onclick={() => chooseDevice('all')}
            >
              <div class="scope-opt-main">
                <span class="scope-opt-title">全部设备汇总</span>
              </div>
              <span class="scope-opt-badge tabular">
                {formatCompactTokens((snapshot?.local.total_tokens ?? 0) + (snapshot?.remote.total_tokens ?? 0))}
              </span>
            </button>

            <button
              type="button"
              class="device-scope-option"
              class:selected={deviceScope === 'local'}
              role="option"
              aria-selected={deviceScope === 'local'}
              onclick={() => chooseDevice('local')}
            >
              <div class="scope-opt-main">
                <span class="scope-opt-title">本机 · {snapshot?.identity?.display_name ?? 'This device'}</span>
              </div>
              <span class="scope-opt-badge tabular">
                {formatCompactTokens(snapshot?.local.total_tokens ?? 0)}
              </span>
            </button>

            {#if selectableRemoteDevices.length > 0}
              <div class="device-scope-divider">已同步远端设备</div>
              {#each selectableRemoteDevices as rDev}
                {@const rTokens = snapshot?.remote_by_device.find((x) => x.owner_device_id === rDev.device_id)?.total_tokens ?? 0}
                <button
                  type="button"
                  class="device-scope-option"
                  class:selected={deviceScope === rDev.device_id}
                  role="option"
                  aria-selected={deviceScope === rDev.device_id}
                  onclick={() => chooseDevice(rDev.device_id)}
                >
                  <div class="scope-opt-main">
                    <span class="scope-opt-title">{rDev.display_name}</span>
                    <span class="scope-opt-sub">ID: …{rDev.device_id.slice(-8)}</span>
                  </div>
                  <span class="scope-opt-badge tabular">{formatCompactTokens(rTokens)}</span>
                </button>
              {/each}
            {/if}
          </div>
        {/if}
      </div>

      <div class="range-capsule" role="group" aria-label="时间跨度">
        {#each [
          { value: 'today', label: '今天' },
          { value: '7d', label: '7 天' },
          { value: '30d', label: '30 天' },
          { value: 'all', label: '全部' },
        ] as opt}
          <button
            class="range-btn"
            class:selected={range === opt.value}
            aria-pressed={range === opt.value}
            onclick={() => {
              customOpen = false;
              handleSelectRange(opt.value as Exclude<DashboardRange, "custom">);
            }}
          >
            {opt.label}
          </button>
        {/each}

        <div class="custom-range-wrap">
          <button
            class="range-btn"
            class:selected={range === 'custom'}
            aria-pressed={range === 'custom'}
            onclick={() => (customOpen = !customOpen)}
          >
            {#if range === 'custom' && customStart && customEnd}
              {customStart.slice(5)} ~ {customEnd.slice(5)}
            {:else}
              自定义
            {/if}
          </button>

          {#if customOpen}
            <DateRangePicker
              startDate={customStart}
              endDate={customEnd}
              maxDate={todayIso}
              onApply={({ start, end }) => {
                customOpen = false;
                applyCustomRange({ start, end });
              }}
              onClose={() => (customOpen = false)}
            />
          {/if}
        </div>
      </div>

      {#if activeTabLabel === '使用总览'}
        <button
          type="button"
          class="icon-btn"
          class:active={isEditingOverviewLayout}
          aria-label={isEditingOverviewLayout ? '完成看板编辑' : '自定义看板布局'}
          title={isEditingOverviewLayout ? '完成看板编辑' : '自定义看板布局'}
          onclick={onToggleOverviewLayout}
        >
          {@html isEditingOverviewLayout ? icons.check : icons.layout}
        </button>
      {/if}

      <button
        class="icon-btn"
        aria-label="刷新数据"
        title="刷新统计"
        disabled={refreshing}
        onclick={() => void loadDashboard()}
      >
        <span class:spin={refreshing}>{@html icons.refresh}</span>
      </button>
    </div>
  {/if}
</header>
