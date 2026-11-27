import { USD_DISPLAY, type DisplayCurrency } from './displayCurrency';
import { formatCost, formatNumber } from './format';

export type TokenUnitTier = 'full' | 'k' | 'm' | 'b' | 't';

export interface TokenTierCandidate {
  tier: TokenUnitTier;
  text: string;
}

const oneDecimalNoGroup = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 1,
  useGrouping: false,
});

const twoDecimalNoGroup = new Intl.NumberFormat('en-US', {
  maximumFractionDigits: 2,
  useGrouping: false,
});

function formatScaled(value: number, divisor: number, suffix: 'K' | 'M' | 'B' | 'T', preferTwoDecimalsBelowTen: boolean): string {
  const scaled = value / divisor;
  const absScaled = Math.abs(scaled);
  const formatter = preferTwoDecimalsBelowTen && absScaled < 10 ? twoDecimalNoGroup : oneDecimalNoGroup;
  return `${formatter.format(scaled)}${suffix}`;
}

export function formatTokenAtTier(value: number | null | undefined, tier: TokenUnitTier): string {
  const num = value ?? 0;
  switch (tier) {
    case 'full':
      return formatNumber(num);
    case 'k':
      return formatScaled(num, 1_000, 'K', false);
    case 'm':
      return formatScaled(num, 1_000_000, 'M', false);
    case 'b':
      return formatScaled(num, 1_000_000_000, 'B', true);
    case 't':
      return formatScaled(num, 1_000_000_000_000, 'T', true);
  }
}

/**
 * Progressive compacting ladder for token counts:
 * full -> M (when >= 1M) -> B (when >= 1B) -> T (when >= 1T),
 * or full -> K (when 1K <= abs < 1M).
 */
export function buildTokenTiers(value: number | null | undefined): TokenTierCandidate[] {
  const num = value ?? 0;
  const abs = Math.abs(num);
  const tiers: TokenTierCandidate[] = [{ tier: 'full', text: formatTokenAtTier(num, 'full') }];

  const pushUnique = (tier: TokenUnitTier) => {
    const text = formatTokenAtTier(num, tier);
    if (tiers[tiers.length - 1]?.text !== text) {
      tiers.push({ tier, text });
    }
  };

  if (abs >= 1_000_000) {
    pushUnique('m');
    if (abs >= 1_000_000_000) {
      pushUnique('b');
    }
    if (abs >= 1_000_000_000_000) {
      pushUnique('t');
    }
  } else if (abs >= 1_000) {
    pushUnique('k');
  }

  return tiers;
}

export function buildTokenCandidates(value: number | null | undefined): string[] {
  return buildTokenTiers(value).map((item) => item.text);
}

/**
 * Progressive compacting ladder for currency costs:
 * exact formatCost -> compact K / M / B when space is tight.
 */
export function buildCostCandidates(
  microUsd: number | null | undefined,
  display: DisplayCurrency = USD_DISPLAY,
): string[] {
  const full = formatCost(microUsd, display);
  if (microUsd === null || microUsd === undefined) return [full];

  const amount = (microUsd / 1_000_000) * display.multiplier;
  const abs = Math.abs(amount);
  const sign = amount < 0 ? '-' : '';
  const symbol = display.code === 'CNY' ? '¥' : '$';
  const candidates: string[] = [full];

  const pushUnique = (text: string) => {
    if (candidates[candidates.length - 1] !== text) {
      candidates.push(text);
    }
  };

  if (abs >= 1_000_000) {
    pushUnique(`${sign}${symbol}${formatScaled(abs, 1_000_000, 'M', true)}`);
    if (abs >= 1_000_000_000) {
      pushUnique(`${sign}${symbol}${formatScaled(abs, 1_000_000_000, 'B', true)}`);
    }
  } else if (abs >= 1_000) {
    pushUnique(`${sign}${symbol}${formatScaled(abs, 1_000, 'K', true)}`);
  }

  return candidates;
}
