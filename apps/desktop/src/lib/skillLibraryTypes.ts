import type { Id, Provider } from './agentCustomizationEntries';
import type { SkillSummary } from './agentSkillSourceTypes';
import { stripMarkdownToPlainText } from './skillSummaryMarkdown';

export type SkillSourceType = 'skills.sh' | 'git' | 'import' | 'local' | 'builtin' | string;

export type SkillDeployment = {
  id: Id;
  provider: Provider;
  root: string;
  path: string;
  enabled: boolean;
  linked: boolean;
  editable: boolean;
  builtin: boolean;
};

export type SkillSourceInfo = {
  source_type: SkillSourceType;
  source_group: string;
  source_ref: string | null;
  resolved_url: string | null;
  subpath: string | null;
  revision: string | null;
};

export type SkillLibraryItem = {
  key: string;
  name: string;
  primary_id: Id;
  path: string;
  exists: boolean;
  editable: boolean;
  builtin: boolean;
  linked: boolean;
  enabled: boolean;
  description: string;
  summary: SkillSummary | null;
  source: SkillSourceInfo;
  deployments: SkillDeployment[];
};

export type SkillViewMode = 'grid' | 'list';
export type SkillStatusFilter = 'all' | 'enabled' | 'disabled';
export type SkillAiFilter = 'all' | 'analyzed' | 'unanalyzed' | 'failed';
export type SkillSourceFilter = 'all' | 'local' | 'import' | 'git' | 'skills.sh' | 'builtin';

export type SkillFilterState = {
  query: string;
  status: SkillStatusFilter;
  source: SkillSourceFilter;
  ai: SkillAiFilter;
  provider: 'all' | Provider;
};

export type SkillCardBrief = {
  text: string;
  isAi: boolean;
};

export type SkillGroup = {
  group: string;
  items: SkillLibraryItem[];
};

export function isSkillAnalyzable(item: SkillLibraryItem): boolean {
  return item.exists && Boolean(item.path);
}

export function deriveCardBrief(item: SkillLibraryItem): SkillCardBrief {
  const explicitBrief = item.summary?.brief?.trim();
  if (explicitBrief) {
    return { text: explicitBrief, isAi: true };
  }

  const overview = item.summary?.overview?.trim();
  if (overview) {
    const plain = stripMarkdownToPlainText(overview);
    if (plain) {
      const sentenceMatch = plain.match(/^.+?(?:[。！？]|[.!?](?:\s|$))/);
      const firstSentence = (sentenceMatch ? sentenceMatch[0] : plain).trim();
      const clipped = firstSentence.length > 76 ? `${firstSentence.slice(0, 74).trim()}…` : firstSentence;
      return { text: clipped, isAi: true };
    }
  }

  const fallbackDesc = item.description?.trim();
  if (fallbackDesc) {
    return { text: fallbackDesc, isAi: false };
  }

  return { text: '', isAi: false };
}

export function sourceTypeLabel(sourceType: string): string {
  switch (sourceType) {
    case 'skills.sh':
      return 'skills.sh';
    case 'git':
      return 'Git';
    case 'import':
      return '导入';
    case 'builtin':
      return '内置';
    case 'local':
    default:
      return '本地';
  }
}

export function filterSkillItems(
  items: SkillLibraryItem[],
  filter: SkillFilterState,
  failedKeys: ReadonlySet<string> = new Set()
): SkillLibraryItem[] {
  const q = filter.query.trim().toLowerCase();

  return items.filter((item) => {
    if (filter.status === 'enabled' && !item.enabled) return false;
    if (filter.status === 'disabled' && item.enabled) return false;

    if (filter.source !== 'all' && item.source.source_type !== filter.source) return false;

    if (filter.provider !== 'all') {
      const hasProvider = item.deployments.some((d) => d.provider === filter.provider);
      if (!hasProvider) return false;
    }

    if (filter.ai === 'analyzed' && !item.summary) return false;
    if (filter.ai === 'unanalyzed' && (item.summary !== null || !isSkillAnalyzable(item))) return false;
    if (filter.ai === 'failed' && !failedKeys.has(item.key)) return false;

    if (q) {
      const haystack = [
        item.name,
        item.description,
        item.summary?.brief ?? '',
        item.summary?.overview ?? '',
        item.source.source_group,
        item.source.source_ref ?? '',
      ]
        .join(' ')
        .toLowerCase();
      if (!haystack.includes(q)) return false;
    }

    return true;
  });
}

export function groupSkillItemsBySource(items: SkillLibraryItem[]): SkillGroup[] {
  const map = new Map<string, SkillLibraryItem[]>();
  for (const item of items) {
    const groupName = item.source.source_group?.trim() || (item.builtin ? '内置技能' : '未分类');
    const bucket = map.get(groupName);
    if (bucket) {
      bucket.push(item);
    } else {
      map.set(groupName, [item]);
    }
  }

  const specialOrder = (name: string): number => {
    if (name === '未分类') return 1;
    if (name === '内置技能') return 2;
    return 0;
  };

  return Array.from(map.entries())
    .sort(([nameA, listA], [nameB, listB]) => {
      const orderDiff = specialOrder(nameA) - specialOrder(nameB);
      if (orderDiff !== 0) return orderDiff;
      if (listB.length !== listA.length) return listB.length - listA.length;
      return nameA.localeCompare(nameB);
    })
    .map(([group, groupItems]) => ({ group, items: groupItems }));
}
