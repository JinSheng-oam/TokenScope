import { describe, expect, it } from 'vitest';
import {
  createSkillBatchController,
  emptyBatchProgress,
  type BatchProgress,
} from './skillBatchAnalyzer';
import type { SkillSummary } from './agentSkillSourceTypes';
import type { SkillLibraryItem } from './skillLibraryTypes';

function makeSkill(name: string, analyzed = false): SkillLibraryItem {
  return {
    key: `key:${name}`,
    name,
    primary_id: { provider: 'codex', root: 'codex-skills', name },
    path: `C:\\skills\\${name}\\SKILL.md`,
    exists: true,
    editable: true,
    builtin: false,
    linked: false,
    enabled: true,
    description: '',
    summary: analyzed
      ? {
          skill_name: name,
          brief: '已有摘要',
          overview: '已有详细摘要',
          usage_guide: '',
          raw_response: '',
          model: 'test-model',
          updated_at_ms: 1,
        }
      : null,
    source: {
      source_type: 'local',
      source_group: '未分类',
      source_ref: null,
      resolved_url: null,
      subpath: null,
      revision: null,
    },
    deployments: [],
  };
}

function makeSummary(name: string): SkillSummary {
  return {
    skill_name: name,
    brief: `${name} 简介`,
    overview: `${name} 详细`,
    scenarios: '- 场景1',
    usage_guide: '示例',
    raw_response: 'raw',
    model: 'deepseek-chat',
    updated_at_ms: Date.now(),
  };
}

describe('skillBatchAnalyzer', () => {
  it('analyzes only unanalyzed skills and enforces concurrency limit', async () => {
    const items = [makeSkill('a', true), makeSkill('b'), makeSkill('c'), makeSkill('d'), makeSkill('e')];
    let inFlight = 0;
    let maxInFlight = 0;
    const completed: string[] = [];
    let latest: BatchProgress = emptyBatchProgress();

    const controller = createSkillBatchController({
      analyze: async (item) => {
        inFlight += 1;
        maxInFlight = Math.max(maxInFlight, inFlight);
        await new Promise((r) => setTimeout(r, 15));
        inFlight -= 1;
        return makeSummary(item.name);
      },
      onItemSuccess: (item) => completed.push(item.name),
      onChange: (p) => {
        latest = p;
      },
      delayFn: async () => {},
    });

    await controller.start(items, { mode: 'unanalyzed', concurrency: 2, maxRetries: 1 });

    expect(completed.sort()).toEqual(['b', 'c', 'd', 'e']);
    expect(maxInFlight).toBeLessThanOrEqual(2);
    expect(latest.running).toBe(false);
    expect(latest.total).toBe(4);
    expect(latest.succeeded).toBe(4);
    expect(latest.failed).toBe(0);
  });

  it('retries transient failures automatically and supports manual retryFailed', async () => {
    const items = [makeSkill('flaky'), makeSkill('hard-fail'), makeSkill('recoverable')];
    const attempts: Record<string, number> = {};
    let allowRecoverable = false;
    let latest: BatchProgress = emptyBatchProgress();

    const controller = createSkillBatchController({
      analyze: async (item) => {
        attempts[item.name] = (attempts[item.name] ?? 0) + 1;
        if (item.name === 'flaky' && attempts[item.name] < 2) {
          throw new Error('AI 接口返回错误 (HTTP 429)：rate limit');
        }
        if (item.name === 'hard-fail') {
          throw new Error('AI 接口返回错误 (HTTP 401)：unauthorized');
        }
        if (item.name === 'recoverable' && !allowRecoverable) {
          throw new Error('请求 AI 接口失败：timeout');
        }
        return makeSummary(item.name);
      },
      onItemSuccess: () => {},
      onChange: (p) => {
        latest = p;
      },
      delayFn: async () => {},
    });

    await controller.start(items, { mode: 'all', concurrency: 3, maxRetries: 2 });

    expect(attempts['flaky']).toBe(2);
    expect(attempts['hard-fail']).toBe(1); // Non-retryable 401 fails immediately
    expect(attempts['recoverable']).toBe(3); // Initial + 2 retries
    expect(latest.succeeded).toBe(1);
    expect(latest.failed).toBe(2);

    // Now allow recoverable to succeed and trigger retryFailed
    allowRecoverable = true;
    await controller.retryFailed(items, { concurrency: 2, maxRetries: 0 });
    expect(latest.succeeded).toBe(2);
    expect(latest.failed).toBe(1);
    expect(latest.failedList.map((f) => f.name)).toEqual(['hard-fail']);
  });
});
