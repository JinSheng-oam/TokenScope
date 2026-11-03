export function nextChartIndex(
  current: number | null,
  direction: 'prev' | 'next' | 'home' | 'end',
  length: number,
): number | null {
  if (length <= 0) return null;
  if (direction === 'home') return 0;
  if (direction === 'end') return length - 1;
  const safeCurrent = current === null ? 0 : Math.max(0, Math.min(current, length - 1));
  return Math.max(0, Math.min(length - 1, safeCurrent + (direction === 'next' ? 1 : -1)));
}
