import { describe, expect, it } from 'vitest';
import { filterPrices, indexPrices, priceKey, sortPrices, type PriceEntry } from './modelPricing';

function entry(provider_id: string, model_id: string, input_rate_nanos: number | null): PriceEntry {
  return { provider_id, model_id, input_rate_nanos, output_rate_nanos: null, cache_read_rate_nanos: null, cache_write_rate_nanos: null, reasoning_rate_nanos: null };
}

describe('model price filtering and sorting', () => {
  const alpha = entry('openai', 'alpha-gpt', 3);
  const manual = entry('google', 'manual-gemini', 7);
  const unknown = entry('openai', 'unknown', null);
  const indexed = indexPrices([alpha, manual, unknown], new Set([priceKey(manual)]));

  it('keeps manual prices first for the default sort and filters multiple terms', () => {
    const ordered = sortPrices(indexed, 'model', 'asc');
    expect(ordered.map(({ entry }) => entry.model_id)).toEqual(['manual-gemini', 'alpha-gpt', 'unknown']);
    expect(filterPrices(ordered, ['openai', 'gpt'], '', false)).toEqual([alpha]);
    expect(filterPrices(ordered, [], 'google', true)).toEqual([manual]);
  });

  it('preserves numeric null placement and sorting when filters change', () => {
    const ordered = sortPrices(indexed, 'input_rate_nanos', 'desc');
    expect(filterPrices(ordered, [], '', false)).toEqual([manual, alpha, unknown]);
    expect(filterPrices(ordered, [], 'openai', false)).toEqual([alpha, unknown]);
    expect(filterPrices(ordered, [], '', true)).toEqual([manual]);
  });
});
