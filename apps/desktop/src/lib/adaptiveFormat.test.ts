import { describe, expect, it } from 'vitest';
import {
  buildCostCandidates,
  buildTokenCandidates,
  buildTokenTiers,
  formatTokenAtTier,
} from './adaptiveFormat';

describe('adaptive number formatters', () => {
  it('builds full -> M -> B ladder for billion-scale tokens', () => {
    expect(buildTokenCandidates(23_367_576_743)).toEqual([
      '23,367,576,743',
      '23367.6M',
      '23.4B',
    ]);
    expect(buildTokenCandidates(1_262_834_372)).toEqual([
      '1,262,834,372',
      '1262.8M',
      '1.26B',
    ]);
  });

  it('builds full -> M ladder for million-scale tokens', () => {
    expect(buildTokenCandidates(94_312_884)).toEqual(['94,312,884', '94.3M']);
    expect(buildTokenCandidates(41_235_731)).toEqual(['41,235,731', '41.2M']);
  });

  it('builds full -> K ladder for thousand-scale tokens and keeps small values intact', () => {
    expect(buildTokenCandidates(189_166)).toEqual(['189,166', '189.2K']);
    expect(buildTokenCandidates(512)).toEqual(['512']);
    expect(buildTokenCandidates(0)).toEqual(['0']);
  });

  it('formats any value at a chosen unit tier for rolling animations', () => {
    expect(formatTokenAtTier(23_367_576_743, 'full')).toBe('23,367,576,743');
    expect(formatTokenAtTier(23_367_576_743, 'm')).toBe('23367.6M');
    expect(formatTokenAtTier(23_367_576_743, 'b')).toBe('23.4B');
    expect(buildTokenTiers(1_500_000_000_000).map((t) => t.tier)).toEqual(['full', 'm', 'b', 't']);
  });

  it('builds compact currency candidates for large costs', () => {
    const cny = { code: 'CNY' as const, multiplier: 7.25, costDecimalPlaces: 0 as const };
    // 4,000 USD * 7.25 = ¥29,000
    expect(buildCostCandidates(4_000_000_000, cny)).toEqual(['¥29,000', '¥29K']);
    // 200,000 USD * 7.25 = ¥1,450,000
    expect(buildCostCandidates(200_000_000_000, cny)).toEqual(['¥1,450,000', '¥1.45M']);
    expect(buildCostCandidates(null, cny)).toEqual(['暂无价格数据']);
  });
});
