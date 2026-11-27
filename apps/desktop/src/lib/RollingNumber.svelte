<script lang="ts">
  import { onDestroy, onMount, tick } from 'svelte';
  import { buildTokenTiers, formatTokenAtTier, type TokenUnitTier } from './adaptiveFormat';
  import { formatNumber } from './format';

  export let value: number | null | undefined;
  export let enabled = true;

  const DURATION_MS = 520;

  let displayValue = 0;
  let pulsing = false;
  let mounted = false;
  let activeTier: TokenUnitTier = 'full';
  let spanEl: HTMLSpanElement | null = null;
  let rafId: number | null = null;
  let fitRafId: number | null = null;
  let pulseTimer: ReturnType<typeof setTimeout> | null = null;
  let resizeObserver: ResizeObserver | null = null;

  function stopAnimation() {
    if (rafId !== null) {
      cancelAnimationFrame(rafId);
      rafId = null;
    }
  }

  function pickAdaptiveTier() {
    if (!spanEl || !spanEl.parentElement) return;
    const container = spanEl.parentElement;
    if (container.clientWidth <= 0) return;

    const tiers = buildTokenTiers(targetValue);
    let chosen: TokenUnitTier = tiers[0]?.tier ?? 'full';
    for (const candidate of tiers) {
      spanEl.textContent = candidate.text;
      chosen = candidate.tier;
      if (container.scrollWidth <= container.clientWidth + 1) break;
    }

    activeTier = chosen;
    spanEl.textContent = formatTokenAtTier(enabled ? displayValue : targetValue, activeTier);
  }

  function scheduleTierFit() {
    if (fitRafId !== null) cancelAnimationFrame(fitRafId);
    fitRafId = requestAnimationFrame(() => {
      fitRafId = null;
      pickAdaptiveTier();
    });
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
  $: formattedDisplay = formatTokenAtTier(enabled ? displayValue : targetValue, activeTier);
  $: if (spanEl) {
    spanEl.textContent = formattedDisplay;
  }
  $: if (mounted) {
    if (!enabled) {
      stopAnimation();
      pulsing = false;
      displayValue = targetValue;
    } else {
      animateTo(targetValue, false);
    }
    tick().then(() => pickAdaptiveTier());
  }

  onMount(() => {
    mounted = true;
    if (enabled) {
      animateTo(targetValue, true);
    } else {
      displayValue = targetValue;
    }

    if (spanEl?.parentElement && typeof ResizeObserver !== 'undefined') {
      resizeObserver = new ResizeObserver(() => {
        pickAdaptiveTier();
      });
      resizeObserver.observe(spanEl.parentElement);
    }
    pickAdaptiveTier();
    scheduleTierFit();
  });

  onDestroy(() => {
    stopAnimation();
    if (fitRafId !== null) cancelAnimationFrame(fitRafId);
    if (pulseTimer) clearTimeout(pulseTimer);
    resizeObserver?.disconnect();
  });
</script>

<span
  bind:this={spanEl}
  class="rolling-num"
  class:is-updating={pulsing}
  title={activeTier !== 'full' ? formatNumber(targetValue) : undefined}
></span>
