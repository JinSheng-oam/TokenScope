import { derived, writable } from 'svelte/store';

export type Currency = 'USD' | 'CNY';
export type DisplayCurrency = { code: Currency; multiplier: number };
type Preference = { currency: Currency; exchangeRate?: number };
const key = 'tokenscope.priceCurrency';
export const DEFAULT_EXCHANGE_RATE = 7.25;
export const USD_DISPLAY: DisplayCurrency = { code: 'USD', multiplier: 1 };

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
    ? { code: 'CNY', multiplier: preference.exchangeRate } : USD_DISPLAY;
}

const saved = readStored();
const preferences = writable<Preference>({
  currency: saved.currency === 'CNY' ? 'CNY' : 'USD',
  exchangeRate: typeof saved.exchangeRate === 'number' && validRate(saved.exchangeRate) ? saved.exchangeRate : DEFAULT_EXCHANGE_RATE,
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
  update((current) => ({ currency, exchangeRate: validRate(current.exchangeRate) ? current.exchangeRate : DEFAULT_EXCHANGE_RATE }));
}

export function setDisplayExchangeRate(exchangeRate: number | undefined) {
  update((current) => ({ ...current, exchangeRate }));
}
