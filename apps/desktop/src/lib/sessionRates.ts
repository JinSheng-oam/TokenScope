import { formatCompactTokens } from './dashboardView';
import type { SessionActivity, SessionUsage } from './types';

type SessionKey = Pick<SessionUsage, 'tool_id' | 'session_id'>;
type OutputSpeed = NonNullable<SessionActivity['output_speed']>;
const sessionKey = (session: SessionKey) => `${session.tool_id}:${session.session_id}`;

export type SessionRates = ReadonlyMap<string, OutputSpeed>;

export const sessionRateTitle = 'Codex 最近一轮的估算输出速度：模型输出 Token（含推理）÷ 首个 Token 到回合结束的时间。';

export function buildSessionRates(sessions: SessionActivity[]): SessionRates {
  const rates = new Map<string, OutputSpeed>();
  for (const session of sessions) {
    if (session.output_speed) rates.set(sessionKey(session), session.output_speed);
  }
  return rates;
}

export function sessionRateLabel(rates: SessionRates | null, session: SessionKey, nowMs: number): string | null {
  const speed = rates?.get(sessionKey(session));
  if (!speed || nowMs - speed.completed_at_ms > 5 * 60_000) return null;
  const perSecond = speed.output_tokens * 1000 / speed.generation_ms;
  const display = perSecond < 1 ? '<1' : perSecond < 100 ? perSecond.toFixed(1) : formatCompactTokens(perSecond);
  return `上次输出 ≈ ${display} Token/s`;
}
