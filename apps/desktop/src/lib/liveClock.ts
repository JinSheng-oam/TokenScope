import { readable } from 'svelte/store';

// A single shared clock keeps relative times and quota resets live without
// scheduling one timer for every visible card.
export const liveNow = readable(Date.now(), (set) => {
  const update = () => set(Date.now());
  let timer: ReturnType<typeof setInterval> | undefined;
  const syncVisibility = () => {
    if (typeof document !== 'undefined' && document.visibilityState !== 'visible') {
      if (timer !== undefined) clearInterval(timer);
      timer = undefined;
      return;
    }
    update();
    if (timer === undefined) timer = setInterval(update, 1_000);
  };
  syncVisibility();
  if (typeof document !== 'undefined') document.addEventListener('visibilitychange', syncVisibility);
  return () => {
    if (timer !== undefined) clearInterval(timer);
    if (typeof document !== 'undefined') document.removeEventListener('visibilitychange', syncVisibility);
  };
});
