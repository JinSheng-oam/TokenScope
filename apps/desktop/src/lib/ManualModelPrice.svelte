<script lang="ts">
  import { rateColumns, type PriceEntry, type RateKey } from './modelPricing';

  export let entry: PriceEntry | null = null;
  export let busy: boolean;
  export let onSave: (entry: PriceEntry) => void;
  export let onCancel: () => void;

  let provider = entry?.provider_id ?? '';
  let model = entry?.model_id ?? '';
  const fieldFormat = new Intl.NumberFormat('en-US', { useGrouping: false, maximumFractionDigits: 9 });
  let fields = Object.fromEntries(rateColumns.map(({ key }) => [key, entry?.[key] == null ? '' : fieldFormat.format(entry[key]! / 1e9)])) as Record<RateKey, string>;
  let error = '';

  function submit(event: SubmitEvent) {
    event.preventDefault();
    const providerId = provider.trim().toLowerCase();
    const modelId = model.trim();
    if (!/^[a-z0-9._-]{1,128}$/.test(providerId) || !modelId) {
      error = '请输入有效的厂商标识和完整模型名。';
      return;
    }
    const rates = {} as Record<RateKey, number | null>;
    for (const { key } of rateColumns) {
      const raw = fields[key].trim();
      const nanos = Math.round(Number(raw) * 1e9);
      if (raw && (!/^\d+(?:\.\d{1,9})?$/.test(raw) || !Number.isSafeInteger(nanos))) {
        error = '费率须为非负数字，最多保留 9 位小数。';
        return;
      }
      rates[key] = raw ? nanos : null;
    }
    if (rates.input_rate_nanos === null || rates.output_rate_nanos === null) {
      error = '请输入输入和输出费率。';
      return;
    }
    error = '';
    onSave({ provider_id: providerId, model_id: modelId, ...rates });
  }
</script>

<form class="price-form" onsubmit={submit}>
  <div class="form-heading"><strong>{entry ? '编辑手动报价' : '添加模型报价'}</strong><span>USD / 百万 Token</span></div>
  <div class="fields">
    <label>厂商标识<input bind:value={provider} maxlength="128" placeholder="例如 openai" readonly={!!entry} required /></label>
    <label>完整模型名<input bind:value={model} maxlength="256" placeholder="例如 gpt-5.6-luna" readonly={!!entry} required /></label>
    {#each rateColumns as { key, label }}
      <label>{label}<input class="tabular" inputmode="decimal" bind:value={fields[key]} placeholder={key.startsWith('cache_') ? '默认按输入价' : key === 'reasoning_rate_nanos' ? '默认按输出价' : '0.00'} /></label>
    {/each}
  </div>
  {#if error}<div class="notice-box" role="alert">{error}</div>{/if}
  <div class="form-actions">
    <button type="submit" class="btn-primary" disabled={busy}>{busy ? '保存中…' : '保存报价'}</button>
    <button type="button" class="btn-secondary" disabled={busy} onclick={onCancel}>取消</button>
  </div>
</form>

<style>
  .price-form {
    display: grid;
    gap: 12px;
    margin-bottom: 12px;
    padding: 14px 16px;
    background: color-mix(in srgb, var(--bg-secondary) 65%, var(--bg-card));
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
  }
  .form-heading, .form-actions {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .form-heading {
    justify-content: space-between;
  }
  .form-heading strong {
    font-size: 13px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .form-heading span {
    color: var(--text-muted);
    font-size: 11.5px;
  }
  .fields {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(140px, 1fr));
    gap: 10px;
  }
  label {
    display: grid;
    gap: 5px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
  }
  input {
    width: 100%;
    min-width: 0;
    height: 34px;
    padding: 0 10px;
    font: inherit;
    font-size: 12.5px;
    color: var(--text-primary);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    transition: border-color var(--transition-fast), background-color var(--transition-fast), box-shadow var(--transition-fast);
  }
  input:focus {
    border-color: var(--brand-primary);
    background: var(--bg-card);
    box-shadow: 0 0 0 2px var(--brand-subtle);
    outline: none;
  }
  input:read-only {
    background: var(--bg-secondary);
    color: var(--text-muted);
    cursor: default;
  }
</style>

