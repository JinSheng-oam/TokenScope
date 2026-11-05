import type { DashboardRange } from './types';

export function dashboardRequestKey(range: DashboardRange, start = '', end = '') {
  return `${range}|${range === 'custom' ? start : ''}|${range === 'custom' ? end : ''}|0`;
}

/** A late result can never replace a newer selection or clear its loading state. */
export function createRequestGate() {
  let sequence = 0;
  return {
    begin: () => ++sequence,
    isCurrent: (request: number) => request === sequence,
    invalidate: () => { sequence += 1; },
  };
}
