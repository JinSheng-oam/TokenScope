/** Current desktop milestone label and user-facing Agent status. */
export const BOOTSTRAP_STAGE = 'M0-12' as const;

export type AgentStatusLabel = 'not-connected' | 'connected';

export function describeAgentStatus(status: AgentStatusLabel): string {
  return status === 'connected' ? '采集器已连接' : '尚未连接采集器';
}
