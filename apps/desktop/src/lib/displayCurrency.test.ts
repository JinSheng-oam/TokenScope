import { get } from 'svelte/store';
import { expect, it } from 'vitest';
import { displayCurrency, setDisplayCurrency, setDisplayExchangeRate } from './displayCurrency';
import { formatCost } from './format';

it('uses the same live currency and rate for all cost displays', () => {
  try {
    setDisplayCurrency('CNY');
    setDisplayExchangeRate(7.2);
    const cny = get(displayCurrency);
    expect(cny).toEqual({ code: 'CNY', multiplier: 7.2 });
    expect(formatCost(1_250_000, cny)).toMatch(/[¥￥]9\.00/);
    expect(formatCost(0, cny)).toMatch(/[¥￥]0/);
    expect(formatCost(null, cny)).toBe('暂无价格数据');

    setDisplayExchangeRate(7.4);
    expect(formatCost(1_250_000, get(displayCurrency))).toMatch(/[¥￥]9\.25/);
    setDisplayCurrency('USD');
    expect(formatCost(1_250_000, get(displayCurrency))).toBe('$1.25');
  } finally {
    setDisplayCurrency('USD');
    setDisplayExchangeRate(7.25);
  }
});
