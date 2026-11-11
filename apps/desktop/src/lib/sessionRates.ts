import { formatCompactTokens } from './dashboardView';
import type { RecentSessionActivity, SessionUsage } from './types';

type SessionKey = Pick<SessionUsage, 'tool_id' | 'session_id'>;
const sessionKey = (session: SessionKey) => `${session.tool_id}:${session.session_id}`;

export type SessionRates = { sampledAtMs: number; tokens: ReadonlyMap<string, number> };

export const sessionRateTitle = '本机最近 60 秒已采集的平均用量（含输入、输出和缓存）';

export function buildSessionRates(update: RecentSessionActivity): SessionRates {
  return {
    sampledAtMs: update.sampled_at_ms,
    tokens: new Map(update.rates.map((rate) => [sessionKey(rate), rate.tokens])),
  };
}

export function sessionRateLabel(rates: SessionRates | null, session: SessionKey, nowMs: number): string | null {
  if (!rates || nowMs - rates.sampledAtMs > 20_000) return null;
  const tokens = rates.tokens.get(sessionKey(session));
  if (!tokens) return null;
  const perSecond = tokens / 60;
  return `${perSecond < 1 ? '<1' : formatCompactTokens(perSecond)} Token/s`;
}
