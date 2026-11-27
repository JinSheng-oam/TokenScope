import { describe, expect, it } from 'vitest';
import {
  createSkillBatchUpdater,
  emptyBatchUpdateProgress,
  type BatchUpdateProgress,
} from './skillBatchUpdater';
import type { SkillLibraryItem } from './skillLibraryTypes';

function makeSkill(name: string): SkillLibraryItem {
  return {
    key: `skill:${name}`,
    name,
    primary_id: { provider: 'library', root: 'tokenscope-skills', name },
    path: `C:\\skills\\${name}\\SKILL.md`,
    exists: true,
    editable: true,
    builtin: false,
    linked: false,
    enabled: true,
    description: '',
    summary: null,
    source: {
      source_type: 'git',
      source_group: 'owner/repo',
      source_ref: 'https://github.com/owner/repo',
      resolved_url: 'https://github.com/owner/repo.git',
      subpath: null,
      revision: null,
    },
    deployments: [],
  };
}

describe('skillBatchUpdater', () => {
  it('checks all skills and updates only those with available updates', async () => {
    const items = [makeSkill('a'), makeSkill('b'), makeSkill('c')];
    let latest: BatchUpdateProgress = emptyBatchUpdateProgress();
    const updatedNames: string[] = [];

    const updater = createSkillBatchUpdater({
      checkOne: async (item) => ({
        kind: 'git',
        source: item.source.source_ref,
        status: item.name === 'b' ? 'available' : 'current',
        detail: item.name === 'b' ? '检测到更新' : '已是最新',
      }),
      updateOne: async (item) => {
        updatedNames.push(item.name);
        return '已更新';
      },
      onChange: (p) => {
        latest = p;
      },
    });

    await updater.checkBatch(items, 2);
    expect(latest.availableCount).toBe(1);
    expect(latest.items['skill:b'].status).toBe('available');
    expect(latest.items['skill:a'].status).toBe('current');

    await updater.updateAvailable(items, 2);
    expect(updatedNames).toEqual(['b']);
    expect(latest.updatedCount).toBe(1);
    expect(latest.availableCount).toBe(0);
  });
});
