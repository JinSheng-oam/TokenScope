<script lang="ts">
  import { onDestroy, onMount } from 'svelte';
  import { formatNumber } from './format';

  export let value: number | null | undefined;
  export let enabled = true;

  const DURATION_MS = 520;

  let displayValue = 0;
  let pulsing = false;
  let mounted = false;
  let rafId: number | null = null;
  let pulseTimer: ReturnType<typeof setTimeout> | null = null;

  function stopAnimation() {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  }

  function animateTo(target: number, isInitial: boolean) {
    stopAnimation();
    if (!enabled || (typeof window !== 'undefined' && window.matchMedia?.('(prefers-reduced-motion: reduce)').matches)) {
      displayValue = target;
      return;
    }

    const from = isInitial ? 0 : displayValue;
    const delta = target - from;
    if (delta === 0) {
      displayValue = target;
      return;
    }

    if (!isInitial) {
      pulsing = true;
      if (pulseTimer) clearTimeout(pulseTimer);
      pulseTimer = setTimeout(() => {
        pulsing = false;
      }, DURATION_MS);
    }

    const startTime = performance.now();
    const step = (now: number) => {
      const progress = Math.min(1, (now - startTime) / DURATION_MS);
      // Ease-out quartic: fast start, smooth deceleration on the final digits
      const eased = 1 - Math.pow(1 - progress, 4);
      displayValue = Math.round(from + delta * eased);
      if (progress < 1) {
        rafId = requestAnimationFrame(step);
      } else {
        displayValue = target;
        rafId = null;
      }
    };
    rafId = requestAnimationFrame(step);
  }

  $: targetValue = value ?? 0;
  $: if (mounted) {
    if (!enabled) {
      stopAnimation();
      pulsing = false;
      displayValue = targetValue;
    } else {
      animateTo(targetValue, false);
    }
  }

  onMount(() => {
    mounted = true;
    if (enabled) {
      animateTo(targetValue, true);
    } else {
      displayValue = targetValue;
    }
  });

  onDestroy(() => {
    stopAnimation();
    if (pulseTimer) clearTimeout(pulseTimer);
  });
</script>

<span class="rolling-num" class:is-updating={pulsing}>{formatNumber(enabled ? displayValue : targetValue)}</span>
