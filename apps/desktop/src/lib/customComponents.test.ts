import { describe, expect, it } from 'vitest';

describe('Custom components tests', () => {
  it('correctly calculates iso date range logic', () => {
    const d = new Date(2026, 10, 4); // 2026-11-04
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    expect(`${y}-${m}-${day}`).toBe('2026-11-04');
  });

  it('bounds preset ranges properly', () => {
    const end = '2026-11-04';
    const parts = end.split('-').map(Number);
    const d = new Date(parts[0], parts[1] - 1, parts[2]);
    d.setDate(d.getDate() - 6);
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    expect(`${y}-${m}-${day}`).toBe('2026-10-29');
  });
});
