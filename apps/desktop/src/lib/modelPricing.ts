export type RateKey = 'input_rate_nanos' | 'output_rate_nanos' | 'cache_read_rate_nanos' | 'cache_write_rate_nanos' | 'reasoning_rate_nanos';
export type PriceEntry = { provider_id: string; model_id: string } & Record<RateKey, number | null>;
export type PricingPreferences = { keyword_matching: boolean; manual_entries: PriceEntry[] };
export type Catalog = { updated_at_ms: number | null; entries: PriceEntry[]; preferences: PricingPreferences };

export const rateColumns: { key: RateKey; label: string }[] = [
  { key: 'input_rate_nanos', label: '输入' },
  { key: 'output_rate_nanos', label: '输出' },
  { key: 'cache_read_rate_nanos', label: '缓存读取' },
  { key: 'cache_write_rate_nanos', label: '缓存写入' },
  { key: 'reasoning_rate_nanos', label: '推理' },
];

export const priceKey = (entry: PriceEntry) => entry.provider_id + '/' + entry.model_id;

export type PriceSortField = 'model' | 'provider' | RateKey;
export type IndexedPrice = { entry: PriceEntry; searchText: string; manual: boolean };

export function indexPrices(entries: PriceEntry[], manualKeys: Set<string>): IndexedPrice[] {
  return entries.map((entry) => {
    const key = priceKey(entry);
    return { entry, searchText: key.toLowerCase(), manual: manualKeys.has(key) };
  });
}

export function sortPrices(entries: IndexedPrice[], field: PriceSortField, direction: 'asc' | 'desc'): IndexedPrice[] {
  const dir = direction === 'asc' ? 1 : -1;
  return [...entries].sort((left, right) => {
    if (left.manual !== right.manual && field === 'model' && direction === 'asc') {
      return Number(right.manual) - Number(left.manual);
    }
    const a = left.entry;
    const b = right.entry;
    if (field === 'model') return a.model_id.localeCompare(b.model_id) * dir;
    if (field === 'provider') {
      const cmp = a.provider_id.localeCompare(b.provider_id);
      return (cmp !== 0 ? cmp : a.model_id.localeCompare(b.model_id)) * dir;
    }
    const av = a[field];
    const bv = b[field];
    if (av === null && bv === null) return a.model_id.localeCompare(b.model_id);
    if (av === null) return 1;
    if (bv === null) return -1;
    return (av - bv) * dir;
  });
}

export function filterPrices(entries: IndexedPrice[], terms: string[], provider: string, manualOnly: boolean): PriceEntry[] {
  return entries.filter(({ entry, searchText, manual }) =>
    (!provider || provider === entry.provider_id) &&
    (!manualOnly || manual) &&
    terms.every((term) => searchText.includes(term))
  ).map(({ entry }) => entry);
}

