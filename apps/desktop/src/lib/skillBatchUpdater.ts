import type { UpdateInfo } from './agentSkillSourceTypes';
import type { SkillLibraryItem } from './skillLibraryTypes';

export type SkillUpdateCheckStatus =
  | 'idle'
  | 'checking'
  | 'available'
  | 'current'
  | 'local_changes'
  | 'unsupported'
  | 'updating'
  | 'updated'
  | 'error';

export type SkillUpdateItemState = {
  key: string;
  name: string;
  status: SkillUpdateCheckStatus;
  detail: string;
  source?: string | null;
};

export type BatchUpdateProgress = {
  phase: 'idle' | 'checking' | 'updating';
  total: number;
  completed: number;
  availableCount: number;
  updatedCount: number;
  errorCount: number;
  items: Record<string, SkillUpdateItemState>;
};

export function emptyBatchUpdateProgress(): BatchUpdateProgress {
  return {
    phase: 'idle',
    total: 0,
    completed: 0,
    availableCount: 0,
    updatedCount: 0,
    errorCount: 0,
    items: {},
  };
}

export function normalizeCheckStatus(raw: string): SkillUpdateCheckStatus {
  if (raw === 'available' || raw === 'update_available') return 'available';
  if (raw === 'current' || raw === 'up_to_date') return 'current';
  if (raw === 'local_changes') return 'local_changes';
  if (raw === 'unsupported' || raw === 'source_missing') return 'unsupported';
  return 'current';
}

export function createSkillBatchUpdater(options: {
  checkOne: (item: SkillLibraryItem) => Promise<UpdateInfo>;
  updateOne: (item: SkillLibraryItem) => Promise<string>;
  onChange: (progress: BatchUpdateProgress) => void;
  onFinishedUpdate?: () => Promise<void> | void;
}) {
  let phase: 'idle' | 'checking' | 'updating' = 'idle';
  let total = 0;
  let completed = 0;
  let abortController: AbortController | null = null;
  let itemStates: Record<string, SkillUpdateItemState> = {};

  function emit() {
    const values = Object.values(itemStates);
    let availableCount = 0;
    let updatedCount = 0;
    let errorCount = 0;
    for (const s of values) {
      if (s.status === 'available') availableCount += 1;
      else if (s.status === 'updated') updatedCount += 1;
      else if (s.status === 'error') errorCount += 1;
    }
    options.onChange({
      phase,
      total,
      completed,
      availableCount,
      updatedCount,
      errorCount,
      items: { ...itemStates },
    });
  }

  async function runWorkers<T>(
    list: T[],
    concurrency: number,
    signal: AbortSignal,
    fn: (entry: T) => Promise<void>
  ) {
    let cursor = 0;
    const workers = Array.from({ length: Math.min(Math.max(1, concurrency), list.length) }, async () => {
      while (!signal.aborted) {
        const idx = cursor;
        cursor += 1;
        if (idx >= list.length) break;
        await fn(list[idx]);
      }
    });
    await Promise.all(workers);
  }

  return {
    async checkBatch(items: SkillLibraryItem[], concurrency = 3) {
      if (phase !== 'idle') return;
      const targets = items.filter((i) => !i.builtin && i.exists);
      if (targets.length === 0) return;

      phase = 'checking';
      total = targets.length;
      completed = 0;
      abortController = new AbortController();
      const signal = abortController.signal;

      for (const item of targets) {
        itemStates[item.key] = {
          key: item.key,
          name: item.name,
          status: 'checking',
          detail: '检查更新中…',
        };
      }
      emit();

      await runWorkers(targets, concurrency, signal, async (item) => {
        try {
          const info = await options.checkOne(item);
          itemStates[item.key] = {
            key: item.key,
            name: item.name,
            status: normalizeCheckStatus(info.status),
            detail: info.detail,
            source: info.source,
          };
        } catch (err) {
          itemStates[item.key] = {
            key: item.key,
            name: item.name,
            status: 'error',
            detail: err instanceof Error ? err.message : String(err),
          };
        } finally {
          completed += 1;
          emit();
        }
      });

      phase = 'idle';
      abortController = null;
      emit();
    },

    async updateAvailable(items: SkillLibraryItem[], concurrency = 2, onlyKeys?: ReadonlySet<string>) {
      if (phase !== 'idle') return;
      const targets = items.filter((item) => {
        if (item.builtin || !item.exists) return false;
        if (onlyKeys && !onlyKeys.has(item.key)) return false;
        const st = itemStates[item.key];
        return st ? st.status === 'available' : Boolean(onlyKeys);
      });
      if (targets.length === 0) return;

      phase = 'updating';
      total = targets.length;
      completed = 0;
      abortController = new AbortController();
      const signal = abortController.signal;

      for (const item of targets) {
        itemStates[item.key] = {
          key: item.key,
          name: item.name,
          status: 'updating',
          detail: '正在更新…',
        };
      }
      emit();

      await runWorkers(targets, concurrency, signal, async (item) => {
        try {
          const msg = await options.updateOne(item);
          itemStates[item.key] = {
            key: item.key,
            name: item.name,
            status: 'updated',
            detail: msg || '已更新至最新版本',
          };
        } catch (err) {
          itemStates[item.key] = {
            key: item.key,
            name: item.name,
            status: 'error',
            detail: err instanceof Error ? err.message : String(err),
          };
        } finally {
          completed += 1;
          emit();
        }
      });

      phase = 'idle';
      abortController = null;
      emit();
      await options.onFinishedUpdate?.();
    },

    async updateSingle(item: SkillLibraryItem) {
      if (phase !== 'idle' || item.builtin) return;
      await this.updateAvailable([item], 1, new Set([item.key]));
    },

    stop() {
      abortController?.abort();
    },
  };
}
