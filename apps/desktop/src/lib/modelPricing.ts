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

