import { derived, writable } from 'svelte/store';

export type Currency = 'USD' | 'CNY';
export const COST_DECIMAL_PLACES = [0, 1, 2, 3, 4, 5, 6] as const;
type CostDecimalPlaces = (typeof COST_DECIMAL_PLACES)[number];
const DEFAULT_COST_DECIMAL_PLACES: CostDecimalPlaces = 2;
export type DisplayCurrency = { code: Currency; multiplier: number; costDecimalPlaces: CostDecimalPlaces };
type Preference = { currency: Currency; exchangeRate?: number; costDecimalPlaces: CostDecimalPlaces };
const key = 'tokenscope.priceCurrency';
export const DEFAULT_EXCHANGE_RATE = 7.25;
export const USD_DISPLAY: DisplayCurrency = { code: 'USD', multiplier: 1, costDecimalPlaces: DEFAULT_COST_DECIMAL_PLACES };

function validRate(rate: number | undefined): rate is number {
  return rate !== undefined && Number.isFinite(rate) && rate >= 0.000001 && rate <= 1000;
}

function readStored(): Record<string, unknown> {
  try {
    const value = JSON.parse(localStorage.getItem(key) ?? '{}');
    return value && typeof value === 'object' && !Array.isArray(value) ? value : {};
  } catch { return {}; }
}

export function resolveDisplayCurrency(preference: Preference): DisplayCurrency {
  return preference.currency === 'CNY' && validRate(preference.exchangeRate)
    ? { code: 'CNY', multiplier: preference.exchangeRate, costDecimalPlaces: preference.costDecimalPlaces }
    : { ...USD_DISPLAY, costDecimalPlaces: preference.costDecimalPlaces };
}

const saved = readStored();
const preferences = writable<Preference>({
  currency: saved.currency === 'CNY' ? 'CNY' : 'USD',
  exchangeRate: typeof saved.exchangeRate === 'number' && validRate(saved.exchangeRate) ? saved.exchangeRate : DEFAULT_EXCHANGE_RATE,
  costDecimalPlaces: COST_DECIMAL_PLACES.find((places) => places === saved.costDecimalPlaces) ?? DEFAULT_COST_DECIMAL_PLACES,
});

function update(change: (current: Preference) => Preference) {
  preferences.update((current) => {
    const next = change(current);
    try { localStorage.setItem(key, JSON.stringify({ ...readStored(), ...next })); } catch { /* Keep the in-memory setting. */ }
    return next;
  });
}

export const currencyPreference = { subscribe: preferences.subscribe };
export const displayCurrency = derived(preferences, resolveDisplayCurrency);

export function setDisplayCurrency(currency: Currency) {
  update((current) => ({ ...current, currency, exchangeRate: validRate(current.exchangeRate) ? current.exchangeRate : DEFAULT_EXCHANGE_RATE }));
}

export function setDisplayExchangeRate(exchangeRate: number | undefined) {
  update((current) => ({ ...current, exchangeRate }));
}

export function setCostDecimalPlaces(value: number) {
  const costDecimalPlaces = COST_DECIMAL_PLACES.find((places) => places === value);
  if (costDecimalPlaces !== undefined) update((current) => ({ ...current, costDecimalPlaces }));
}
