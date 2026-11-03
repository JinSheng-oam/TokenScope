import { readable } from 'svelte/store';

// A single shared clock keeps relative times and quota resets live without
// scheduling one timer for every visible card.
export const liveNow = readable(Date.now(), (set) => {
  const update = () => set(Date.now());
  update();
  const timer = setInterval(update, 1_000);
  return () => clearInterval(timer);
});
