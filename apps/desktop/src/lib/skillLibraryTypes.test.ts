import { describe, expect, it } from 'vitest';
import {
  deriveCardBrief,
  filterSkillItems,
  groupSkillItemsBySource,
  type SkillLibraryItem,
} from './skillLibraryTypes';

function makeItem(overrides: Partial<SkillLibraryItem> = {}): SkillLibraryItem {
  return {
    key: 'c:/skills/demo',
    name: 'demo-skill',
    primary_id: { provider: 'codex', root: 'codex-skills', name: 'demo-skill' },
    path: 'C:\\skills\\demo\\SKILL.md',
    exists: true,
    editable: true,
    builtin: false,
    linked: false,
    enabled: true,
    description: 'Frontmatter fallback description.',
    summary: null,
    source: {
      source_type: 'local',
      source_group: '未分类',
      source_ref: null,
      resolved_url: null,
      subpath: null,
      revision: null,
    },
    deployments: [
      {
        id: { provider: 'codex', root: 'codex-skills', name: 'demo-skill' },
        provider: 'codex',
        root: 'codex-skills',
        path: 'C:\\skills\\demo\\SKILL.md',
        enabled: true,
        linked: false,
        editable: true,
        builtin: false,
      },
    ],
    ...overrides,
  };
}

describe('skillLibraryTypes', () => {
  it('derives brief from explicit brief, legacy overview first sentence, or frontmatter description', () => {
    const withBrief = makeItem({
      summary: {
        skill_name: 'demo-skill',
        brief: '用深模块视角评审代码边界。',
        overview: '详细说明。第二句。',
        usage_guide: '',
        raw_response: '',
        model: 'deepseek-chat',
        updated_at_ms: 1,
      },
    });
    expect(deriveCardBrief(withBrief)).toEqual({
      text: '用深模块视角评审代码边界。',
      isAi: true,
    });

    const legacySummary = makeItem({
      summary: {
        skill_name: 'demo-skill',
        overview: '这是一份**共享词汇表**，用于架构评审。第二句补充信息。',
        usage_guide: '',
        raw_response: '',
        model: 'deepseek-chat',
        updated_at_ms: 1,
      },
    });
    expect(deriveCardBrief(legacySummary)).toEqual({
      text: '这是一份共享词汇表，用于架构评审。',
      isAi: true,
    });

    const unanalyzed = makeItem();
    expect(deriveCardBrief(unanalyzed)).toEqual({
      text: 'Frontmatter fallback description.',
      isAi: false,
    });
  });

  it('filters by status, source, ai analysis state, provider, and search query', () => {
    const analyzed = makeItem({
      key: 'k1',
      name: 'codebase-design',
      enabled: true,
      source: {
        source_type: 'skills.sh',
        source_group: 'mattpcock/skills',
        source_ref: 'mattpcock/skills/codebase-design',
        resolved_url: 'https://github.com/mattpcock/skills.git',
        subpath: null,
        revision: null,
      },
      summary: {
        skill_name: 'codebase-design',
        brief: '深模块设计评审词汇表',
        overview: '详细定位',
        usage_guide: '',
        raw_response: '',
        model: 'gpt-4o-mini',
        updated_at_ms: 1,
      },
    });
    const unanalyzedDisabled = makeItem({
      key: 'k2',
      name: 'tdd',
      enabled: false,
      summary: null,
      deployments: [
        {
          id: { provider: 'claude', root: 'claude-skills', name: 'tdd' },
          provider: 'claude',
          root: 'claude-skills',
          path: 'C:\\skills\\tdd\\SKILL.md',
          enabled: false,
          linked: false,
          editable: true,
          builtin: false,
        },
      ],
    });

    expect(
      filterSkillItems([analyzed, unanalyzedDisabled], {
        query: '深模块',
        status: 'enabled',
        source: 'skills.sh',
        ai: 'analyzed',
        provider: 'codex',
      }).map((x) => x.key)
    ).toEqual(['k1']);

    expect(
      filterSkillItems([analyzed, unanalyzedDisabled], {
        query: '',
        status: 'all',
        source: 'all',
        ai: 'unanalyzed',
        provider: 'claude',
      }).map((x) => x.key)
    ).toEqual(['k2']);
  });

  it('groups by source with named repositories first and uncategorized/builtin last', () => {
    const items = [
      makeItem({ key: '1', source: { ...makeItem().source, source_group: '未分类' } }),
      makeItem({ key: '2', source: { ...makeItem().source, source_group: 'vercel-labs/agent-skills' } }),
      makeItem({ key: '3', source: { ...makeItem().source, source_group: 'vercel-labs/agent-skills' } }),
      makeItem({ key: '4', builtin: true, source: { ...makeItem().source, source_group: '内置技能' } }),
      makeItem({ key: '5', source: { ...makeItem().source, source_group: 'mattpcock/skills' } }),
    ];
    const groups = groupSkillItemsBySource(items);
    expect(groups.map((g) => `${g.group}:${g.items.length}`)).toEqual([
      'vercel-labs/agent-skills:2',
      'mattpcock/skills:1',
      '未分类:1',
      '内置技能:1',
    ]);
  });
});
