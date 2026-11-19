import { readable, type Readable } from 'svelte/store';

function createVisibilityClock(intervalMs: number): Readable<number> {
  return readable(Date.now(), (set) => {
    const update = () => set(Date.now());
    let timer: ReturnType<typeof setInterval> | undefined;
    const syncVisibility = () => {
      if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
        if (timer !== undefined) clearInterval(timer);
        timer = undefined;
        return;
      }
      update();
      if (timer === undefined) timer = setInterval(update, intervalMs);
    };
    syncVisibility();
    if (typeof document !== 'undefined') document.addEventListener('visibilitychange', syncVisibility);
    return () => {
      if (timer !== undefined) clearInterval(timer);
      if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', syncVisibility);
    };
  });
}

// 1-second clock for live countdowns (HH:MM:SS) and real-time generation rates.
export const liveNow = createVisibilityClock(1_000);

// 30-second coarse clock for relative timestamps ("3m ago"), stale checks,
// and timeline grouping without waking UI trees every second.
export const liveMinute = createVisibilityClock(30_000);
