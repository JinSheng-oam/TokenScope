<script context="module" lang="ts">
  export type SelectOption = {
    key: string;
    label: string;
    count?: number;
    subtext?: string;
  };
</script>

<script lang="ts">
  import { onMount } from 'svelte';

  export let options: SelectOption[] = [];
  export let value: string = '';
  export let placeholder: string = '请选择';
  export let ariaLabel: string = '选择框';
  export let disabled: boolean = false;
  export let onChange: ((val: string) => void) | undefined = undefined;

  let isOpen = false;
  let containerRef: HTMLDivElement | null = null;
  const listboxId = `select-listbox-${Math.random().toString(36).slice(2, 9)}`;

  $: selectedOption = options.find((opt) => opt.key === value);

  function toggleOpen() {
    if (disabled) return;
    isOpen = !isOpen;
  }

  function handleSelect(optionKey: string) {
    if (disabled) return;
    value = optionKey;
    isOpen = false;
    onChange?.(optionKey);
  }

  function handleKeydown(event: KeyboardEvent) {
    if (disabled) return;
    if (event.key === 'Escape') {
      isOpen = false;
    } else if (event.key === 'Enter' || event.key === ' ') {
      if (!isOpen) {
        isOpen = true;
        event.preventDefault();
      }
    }
  }

  function handleClickOutside(event: MouseEvent) {
    if (containerRef && !containerRef.contains(event.target as Node)) {
      isOpen = false;
    }
  }

  onMount(() => {
    document.addEventListener('pointerdown', handleClickOutside);
    return () => {
      document.removeEventListener('pointerdown', handleClickOutside);
    };
  });
</script>

<div
  class="custom-select-container"
  bind:this={containerRef}
  class:is-open={isOpen}
  class:is-disabled={disabled}
  role="group"
  aria-label={ariaLabel}
>
  <button
    type="button"
    class="select-trigger"
    onclick={toggleOpen}
    onkeydown={handleKeydown}
    {disabled}
    aria-label={ariaLabel}
    aria-haspopup="listbox"
    aria-expanded={isOpen}
    aria-controls={isOpen ? listboxId : undefined}
  >
    <span class="trigger-label" class:is-placeholder={!selectedOption}>
      {selectedOption ? selectedOption.label : placeholder}
    </span>

    {#if selectedOption?.count !== undefined}
      <span class="trigger-count">· {selectedOption.count}</span>
    {/if}

    <span class="chevron-icon" class:rotated={isOpen} aria-hidden="true">
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <polyline points="6 9 12 15 18 9" />
      </svg>
    </span>
  </button>

  {#if isOpen}
    <div id={listboxId} class="select-dropdown" role="listbox" aria-label={ariaLabel}>
      <div class="options-scroll">
        {#each options as opt}
          {@const isSelected = opt.key === value}
          <button
            type="button"
            class="select-option"
            class:selected={isSelected}
            role="option"
            aria-selected={isSelected}
            onclick={() => handleSelect(opt.key)}
          >
            <span class="option-label">{opt.label}</span>

            {#if opt.count !== undefined}
              <span class="option-count">· {opt.count}</span>
            {:else if opt.subtext}
              <span class="option-subtext">{opt.subtext}</span>
            {/if}

            {#if isSelected}
              <span class="check-icon" aria-hidden="true">
                <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round">
                  <polyline points="20 6 9 17 4 12" />
                </svg>
              </span>
            {/if}
          </button>
        {/each}

        {#if options.length === 0}
          <div class="empty-option">暂无可用选项</div>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .custom-select-container {
    position: relative;
    width: 100%;
    min-width: 0;
    user-select: none;
    font-size: 12px;
  }

  .select-trigger {
    width: 100%;
    height: 34px;
    padding: 0 10px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 6px;
    background-color: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    color: var(--text-primary);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    outline: none;
    transition: border-color var(--transition-fast), background-color var(--transition-fast), box-shadow var(--transition-fast);
  }

  .select-trigger:hover:not(:disabled) {
    border-color: var(--border-strong);
    background-color: var(--bg-card);
  }

  .custom-select-container.is-open .select-trigger,
  .select-trigger:focus-visible {
    border-color: var(--brand-primary);
    background-color: var(--bg-card);
    box-shadow: 0 0 0 1px var(--brand-primary);
  }

  .select-trigger:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .trigger-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    text-align: left;
    flex: 1;
    min-width: 0;
    color: var(--text-primary);
    font-weight: 500;
  }

  .trigger-label.is-placeholder {
    color: var(--text-muted);
    font-weight: 400;
  }

  .trigger-count {
    color: var(--text-muted);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .chevron-icon {
    width: 13px;
    height: 13px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-muted);
    transition: transform var(--transition-fast);
    flex-shrink: 0;
  }

  .chevron-icon svg {
    width: 100%;
    height: 100%;
  }

  .chevron-icon.rotated {
    transform: rotate(180deg);
    color: var(--brand-primary);
  }

  /* Dropdown Popover */
  .select-dropdown {
    position: absolute;
    top: calc(100% + 4px);
    left: 0;
    width: 100%;
    min-width: 150px;
    max-width: 320px;
    background-color: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    box-shadow: var(--shadow-lg, 0 10px 25px -5px rgba(0, 0, 0, 0.15), 0 8px 10px -6px rgba(0, 0, 0, 0.1));
    z-index: 999;
    overflow: hidden;
    animation: selectFadeIn 0.15s cubic-bezier(0.16, 1, 0.3, 1);
  }

  @keyframes selectFadeIn {
    from {
      opacity: 0;
      transform: translateY(-4px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  .options-scroll {
    max-height: 240px;
    overflow-y: auto;
    padding: 4px;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }

  .options-scroll::-webkit-scrollbar {
    width: 4px;
  }
  .options-scroll::-webkit-scrollbar-thumb {
    background-color: var(--border-strong);
    border-radius: 9999px;
  }

  .select-option {
    width: 100%;
    padding: 6px 8px;
    display: flex;
    align-items: center;
    gap: 6px;
    background: transparent;
    border: none;
    border-radius: 4px;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11.5px;
    line-height: 1.4;
    text-align: left;
    cursor: pointer;
    transition: background-color var(--transition-fast), color var(--transition-fast);
  }

  .select-option:hover {
    background-color: var(--bg-secondary);
    color: var(--text-primary);
  }

  .select-option.selected {
    background-color: var(--brand-subtle);
    color: var(--brand-strong);
    font-weight: 600;
  }

  .option-label {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .option-count,
  .option-subtext {
    font-size: 10.5px;
    color: var(--text-muted);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }

  .select-option.selected .option-count {
    color: var(--brand-strong);
    opacity: 0.8;
  }

  .check-icon {
    width: 12px;
    height: 12px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--brand-primary);
    flex-shrink: 0;
  }

  .check-icon svg {
    width: 100%;
    height: 100%;
  }

  .empty-option {
    padding: 12px 8px;
    text-align: center;
    font-size: 11px;
    color: var(--text-muted);
  }
</style>
