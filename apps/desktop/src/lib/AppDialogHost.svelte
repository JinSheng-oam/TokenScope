<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import { appDialog, registerDialogHost, resolveActiveDialog } from './dialogStore';
  import { icons } from './icons';

  let promptValue = $state('');
  let inputEl = $state<HTMLInputElement | null>(null);
  let confirmBtnEl = $state<HTMLButtonElement | null>(null);
  let lastRequestId = $state<number | null>(null);

  let unregister: (() => void) | undefined;

  onMount(() => {
    unregister = registerDialogHost();
  });

  onDestroy(() => {
    unregister?.();
  });

  $effect(() => {
    const req = $appDialog;
    if (!req) {
      lastRequestId = null;
      return;
    }
    if (req.id !== lastRequestId) {
      lastRequestId = req.id;
      if (req.kind === 'prompt') {
        promptValue = req.defaultValue;
        void tick().then(() => {
          inputEl?.focus();
          inputEl?.select();
        });
      } else {
        void tick().then(() => {
          confirmBtnEl?.focus();
        });
      }
    }
  });

  function cancel() {
    resolveActiveDialog($appDialog?.kind === 'prompt' ? null : false);
  }

  function submit() {
    const req = $appDialog;
    if (!req) return;
    if (req.kind === 'prompt') {
      resolveActiveDialog(promptValue);
    } else {
      resolveActiveDialog(true);
    }
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!$appDialog) return;
    if (event.key === 'Escape') {
      event.preventDefault();
      cancel();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if $appDialog}
  <div class="app-dialog-backdrop" role="presentation" onclick={cancel}>
    <div
      class="app-dialog"
      class:danger={$appDialog.kind === 'confirm' && $appDialog.tone === 'danger'}
      role="dialog"
      aria-modal="true"
      aria-labelledby="app-dialog-title"
      aria-describedby="app-dialog-message"
      tabindex="-1"
      onclick={(e) => e.stopPropagation()}
      onkeydown={(e) => {
        if (e.key === 'Escape') {
          e.preventDefault();
          cancel();
        }
      }}
    >
      <div class="app-dialog-head">
        <div class="app-dialog-title-wrap">
          <span class="app-dialog-icon" class:danger={$appDialog.kind === 'confirm' && $appDialog.tone === 'danger'}>
            {@html $appDialog.kind === 'confirm' && $appDialog.tone === 'danger' ? icons.alert : icons.info}
          </span>
          <h2 id="app-dialog-title">{$appDialog.title}</h2>
        </div>
        <button type="button" class="icon-btn" aria-label="关闭弹窗" onclick={cancel}>
          {@html icons.close}
        </button>
      </div>

      <form
        class="app-dialog-body"
        onsubmit={(e) => {
          e.preventDefault();
          submit();
        }}
      >
        <p id="app-dialog-message" class="app-dialog-message">{$appDialog.message}</p>

        {#if $appDialog.kind === 'prompt'}
          <input
            bind:this={inputEl}
            type="text"
            class="app-dialog-input"
            bind:value={promptValue}
            placeholder={$appDialog.placeholder}
          />
        {/if}

        <div class="app-dialog-foot">
          <button type="button" class="btn-secondary" onclick={cancel}>
            {$appDialog.cancelText}
          </button>
          <button
            bind:this={confirmBtnEl}
            type="submit"
            class={$appDialog.kind === 'confirm' && $appDialog.tone === 'danger' ? 'btn-danger' : 'btn-primary'}
          >
            {$appDialog.confirmText}
          </button>
        </div>
      </form>
    </div>
  </div>
{/if}

<style>
  .app-dialog-backdrop {
    position: fixed;
    inset: 0;
    z-index: 1200;
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(4px);
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 20px;
    animation: dialogFade 120ms ease-out;
  }

  @keyframes dialogFade {
    from { opacity: 0; }
    to { opacity: 1; }
  }

  .app-dialog {
    width: 100%;
    max-width: 440px;
    background: var(--bg-card);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-lg);
    overflow: hidden;
    display: flex;
    flex-direction: column;
  }

  .app-dialog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .app-dialog-title-wrap {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .app-dialog-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border-radius: var(--radius-sm);
    background: var(--brand-subtle);
    color: var(--brand-primary);
    flex-shrink: 0;
  }

  .app-dialog-icon.danger {
    background: var(--color-danger-bg);
    color: var(--color-danger);
  }

  .app-dialog-icon :global(svg) {
    width: 15px;
    height: 15px;
  }

  h2 {
    margin: 0;
    font-size: 14.5px;
    font-weight: 700;
    color: var(--text-primary);
  }

  .app-dialog-body {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px 18px 18px;
  }

  .app-dialog-message {
    margin: 0;
    font-size: 12.5px;
    line-height: 1.55;
    color: var(--text-secondary);
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .app-dialog-input {
    width: 100%;
    box-sizing: border-box;
    min-height: 34px;
    padding: 0 10px;
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    background: var(--bg-input);
    color: var(--text-primary);
    font: inherit;
    font-size: 12.5px;
  }

  .app-dialog-input:focus {
    outline: none;
    border-color: var(--brand-primary);
    box-shadow: 0 0 0 2px var(--brand-subtle);
  }

  .app-dialog-foot {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 2px;
  }

  .btn-danger {
    min-height: 32px;
    padding: 0 14px;
    border-radius: var(--radius-sm);
    border: 1px solid var(--color-danger-border);
    background: var(--color-danger);
    color: #fff;
    font: inherit;
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
    transition: opacity var(--transition-fast);
  }

  .btn-danger:hover {
    opacity: 0.9;
  }
</style>
