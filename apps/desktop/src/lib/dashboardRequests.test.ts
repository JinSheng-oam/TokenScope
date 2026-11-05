import { describe, expect, it } from 'vitest';
import { createRequestGate, dashboardRequestKey } from './dashboardRequests';

describe('dashboard selection requests', () => {
  it('rejects an older result and its cleanup after a newer range has loaded', async () => {
    const gate = createRequestGate();
    let finishOld!: () => void;
    let displayed = '', loading = true;
    const old = gate.begin();
    const pending = new Promise<void>((resolve) => { finishOld = resolve; }).then(() => {
      if (gate.isCurrent(old)) { displayed = 'today'; loading = false; }
    });
    const current = gate.begin();
    if (gate.isCurrent(current)) displayed = '30d';
    finishOld();
    await pending;
    expect(displayed).toBe('30d');
    expect(loading).toBe(true);
    gate.invalidate();
    expect(gate.isCurrent(current)).toBe(false);
  });

  it('distinguishes custom end dates and ignores unselected custom inputs', () => {
    expect(dashboardRequestKey('custom', '2026-11-01', '2026-11-04'))
      .not.toBe(dashboardRequestKey('custom', '2026-11-01', '2026-11-05'));
    expect(dashboardRequestKey('today', '2026-11-01', '2026-11-05')).toBe('today|||0');
  });
});
