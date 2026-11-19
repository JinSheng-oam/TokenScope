import { afterEach, describe, expect, it, vi } from 'vitest';
import { liveMinute, liveNow } from './liveClock';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('live clock visibility', () => {
  it('pauses while hidden and catches up immediately when visible', () => {
    vi.useFakeTimers();
    vi.setSystemTime(10_000);
    let listener: (() => void) | undefined;
    const doc = {
      visibilityState: 'visible',
      addEventListener: (_type: string, callback: () => void) => { listener = callback; },
      removeEventListener: () => { listener = undefined; },
    };
    vi.stubGlobal('document', doc);
    const values: number[] = [];
    const unsubscribe = liveNow.subscribe((value) => values.push(value));
    vi.advanceTimersByTime(1_000);
    expect(values.at(-1)).toBe(11_000);
    doc.visibilityState = 'hidden';
    listener?.();
    vi.advanceTimersByTime(5_000);
    expect(values.at(-1)).toBe(11_000);
    expect(vi.getTimerCount()).toBe(0);
    doc.visibilityState = 'visible';
    listener?.();
    expect(values.at(-1)).toBe(16_000);
    vi.advanceTimersByTime(1_000);
    expect(values.at(-1)).toBe(17_000);
    unsubscribe();
    expect(vi.getTimerCount()).toBe(0);
    expect(listener).toBeUndefined();
  });

  it('ticks liveMinute at 30-second intervals and catches up when visible', () => {
    vi.useFakeTimers();
    vi.setSystemTime(100_000);
    let listener: (() => void) | undefined;
    const doc = {
      visibilityState: 'visible',
      addEventListener: (_type: string, callback: () => void) => { listener = callback; },
      removeEventListener: () => { listener = undefined; },
    };
    vi.stubGlobal('document', doc);
    const values: number[] = [];
    const unsubscribe = liveMinute.subscribe((value) => values.push(value));
    expect(values.at(-1)).toBe(100_000);
    vi.advanceTimersByTime(10_000);
    expect(values.at(-1)).toBe(100_000);
    vi.advanceTimersByTime(20_000);
    expect(values.at(-1)).toBe(130_000);
    doc.visibilityState = 'hidden';
    listener?.();
    vi.advanceTimersByTime(60_000);
    expect(values.at(-1)).toBe(130_000);
    expect(vi.getTimerCount()).toBe(0);
    unsubscribe();
    expect(vi.getTimerCount()).toBe(0);
  });
});
