import { describe, expect, it } from 'vitest';
import { BOOTSTRAP_STAGE, describeAgentStatus } from './status';

describe('desktop milestone status', () => {
  it('labels the current milestone', () => {
    expect(BOOTSTRAP_STAGE).toBe('M0-12');
  });
  it('does not claim connectivity before Agent health is confirmed', () => {
    expect(describeAgentStatus('not-connected')).toBe('尚未连接采集器');
  });
});
