<script lang="ts">
  export let message: string;
  export let stage = '';
</script>

<div class="loading-status-bar">
  <div class="loading-message">
    <span class="loading-spinner-ring" aria-hidden="true">
      <span class="loading-spinner-core"></span>
    </span>
    <p>{message}</p>
  </div>
  {#if stage}<span class="loading-stage-pill" aria-hidden="true">{stage}</span>{/if}
</div>
