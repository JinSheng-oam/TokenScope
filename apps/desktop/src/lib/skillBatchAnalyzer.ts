import type { SkillSummary } from './agentSkillSourceTypes';
import { isSkillAnalyzable, type SkillLibraryItem } from './skillLibraryTypes';

export type BatchAnalysisMode = 'unanalyzed' | 'selected' | 'all';

export type BatchAnalysisPrefs = {
  concurrency: number;
  maxRetries: number;
  skipAnalyzedOnSelected: boolean;
};

export type BatchItemStatus = 'queued' | 'running' | 'retrying' | 'done' | 'failed';

export type BatchItemState = {
  key: string;
  name: string;
  status: BatchItemStatus;
  attempt: number;
  maxRetries: number;
  error?: string;
};

export type BatchProgress = {
  running: boolean;
  cancelled: boolean;
  total: number;
  completed: number;
  succeeded: number;
  failed: number;
  runningCount: number;
  retryingCount: number;
  activeNames: string[];
  items: Record<string, BatchItemState>;
  failedList: BatchItemState[];
};

const STORAGE_KEY = 'tokenscope:skill-batch-ai-prefs';

export const DEFAULT_BATCH_PREFS: BatchAnalysisPrefs = {
  concurrency: 3,
  maxRetries: 2,
  skipAnalyzedOnSelected: true,
};

export function clampConcurrency(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_BATCH_PREFS.concurrency;
  return Math.max(1, Math.min(8, Math.round(value)));
}

export function clampRetries(value: number): number {
  if (!Number.isFinite(value)) return DEFAULT_BATCH_PREFS.maxRetries;
  return Math.max(0, Math.min(3, Math.round(value)));
}

export function loadBatchAnalysisPrefs(): BatchAnalysisPrefs {
  try {
    const raw = globalThis.localStorage?.getItem(STORAGE_KEY);
    if (!raw) return { ...DEFAULT_BATCH_PREFS };
    const parsed = JSON.parse(raw) as Partial<BatchAnalysisPrefs>;
    return {
      concurrency: clampConcurrency(Number(parsed.concurrency ?? DEFAULT_BATCH_PREFS.concurrency)),
      maxRetries: clampRetries(Number(parsed.maxRetries ?? DEFAULT_BATCH_PREFS.maxRetries)),
      skipAnalyzedOnSelected:
        typeof parsed.skipAnalyzedOnSelected === 'boolean'
          ? parsed.skipAnalyzedOnSelected
          : DEFAULT_BATCH_PREFS.skipAnalyzedOnSelected,
    };
  } catch {
    return { ...DEFAULT_BATCH_PREFS };
  }
}

export function saveBatchAnalysisPrefs(prefs: BatchAnalysisPrefs): BatchAnalysisPrefs {
  const normalized: BatchAnalysisPrefs = {
    concurrency: clampConcurrency(prefs.concurrency),
    maxRetries: clampRetries(prefs.maxRetries),
    skipAnalyzedOnSelected: Boolean(prefs.skipAnalyzedOnSelected),
  };
  try {
    globalThis.localStorage?.setItem(STORAGE_KEY, JSON.stringify(normalized));
  } catch {
    // Ignore storage quota or SSR errors
  }
  return normalized;
}

export function isRetryableAnalysisError(errorMessage: string): boolean {
  const msg = errorMessage.toLowerCase();
  if (
    msg.includes('未配置 ai api') ||
    msg.includes('未配置 ai 模型') ||
    msg.includes('找不到 skill.md') ||
    msg.includes('http 401') ||
    msg.includes('http 403') ||
    msg.includes('http 404') ||
    msg.includes('invalid api key') ||
    msg.includes('unauthorized')
  ) {
    return false;
  }
  return true;
}

export function selectBatchTargets(
  allItems: SkillLibraryItem[],
  mode: BatchAnalysisMode,
  selectedKeys: ReadonlySet<string>,
  skipAnalyzedOnSelected = true
): SkillLibraryItem[] {
  const analyzable = allItems.filter(isSkillAnalyzable);
  if (mode === 'unanalyzed') {
    return analyzable.filter((item) => item.summary === null);
  }
  if (mode === 'selected') {
    return analyzable.filter(
      (item) => selectedKeys.has(item.key) && (!skipAnalyzedOnSelected || item.summary === null)
    );
  }
  return analyzable;
}

export function emptyBatchProgress(): BatchProgress {
  return {
    running: false,
    cancelled: false,
    total: 0,
    completed: 0,
    succeeded: 0,
    failed: 0,
    runningCount: 0,
    retryingCount: 0,
    activeNames: [],
    items: {},
    failedList: [],
  };
}

export type BatchControllerOptions = {
  analyze: (item: SkillLibraryItem) => Promise<SkillSummary>;
  onItemSuccess: (item: SkillLibraryItem, summary: SkillSummary) => void;
  onChange: (progress: BatchProgress) => void;
  delayFn?: (ms: number, signal: AbortSignal) => Promise<void>;
};

function defaultDelay(ms: number, signal: AbortSignal): Promise<void> {
  if (signal.aborted) return Promise.resolve();
  return new Promise((resolve) => {
    const timer = setTimeout(() => {
      signal.removeEventListener('abort', onAbort);
      resolve();
    }, ms);
    const onAbort = () => {
      clearTimeout(timer);
      resolve();
    };
    signal.addEventListener('abort', onAbort, { once: true });
  });
}

export function createSkillBatchController(options: BatchControllerOptions) {
  const delayFn = options.delayFn ?? defaultDelay;
  let running = false;
  let cancelled = false;
  let abortController: AbortController | null = null;
  let itemStates: Record<string, BatchItemState> = {};

  function emit() {
    const values = Object.values(itemStates);
    let succeeded = 0;
    let failed = 0;
    let runningCount = 0;
    let retryingCount = 0;
    const activeNames: string[] = [];
    const failedList: BatchItemState[] = [];

    for (const st of values) {
      if (st.status === 'done') succeeded += 1;
      else if (st.status === 'failed') {
        failed += 1;
        failedList.push(st);
      } else if (st.status === 'running') {
        runningCount += 1;
        activeNames.push(st.name);
      } else if (st.status === 'retrying') {
        retryingCount += 1;
        activeNames.push(`${st.name} (重试 ${st.attempt}/${st.maxRetries})`);
      }
    }

    options.onChange({
      running,
      cancelled,
      total: values.length,
      completed: succeeded + failed,
      succeeded,
      failed,
      runningCount,
      retryingCount,
      activeNames,
      items: { ...itemStates },
      failedList,
    });
  }

  async function runQueue(
    targets: SkillLibraryItem[],
    concurrency: number,
    maxRetries: number,
    mergeIntoExisting = false
  ): Promise<void> {
    if (running || targets.length === 0) return;

    const safeConcurrency = clampConcurrency(concurrency);
    const safeRetries = clampRetries(maxRetries);
    running = true;
    cancelled = false;
    abortController = new AbortController();
    const signal = abortController.signal;

    if (!mergeIntoExisting) {
      itemStates = {};
    }
    for (const item of targets) {
      itemStates[item.key] = {
        key: item.key,
        name: item.name,
        status: 'queued',
        attempt: 0,
        maxRetries: safeRetries,
      };
    }
    emit();

    let cursor = 0;

    async function worker() {
      while (!signal.aborted) {
        const index = cursor;
        cursor += 1;
        if (index >= targets.length) break;

        const item = targets[index];
        let attempt = 0;

        while (!signal.aborted) {
          itemStates[item.key] = {
            key: item.key,
            name: item.name,
            status: attempt === 0 ? 'running' : 'retrying',
            attempt,
            maxRetries: safeRetries,
          };
          emit();

          try {
            const summary = await options.analyze(item);
            itemStates[item.key] = {
              key: item.key,
              name: item.name,
              status: 'done',
              attempt,
              maxRetries: safeRetries,
            };
            options.onItemSuccess(item, summary);
            emit();
            break;
          } catch (err) {
            const message = err instanceof Error ? err.message : String(err);
            const canRetry = !signal.aborted && attempt < safeRetries && isRetryableAnalysisError(message);
            if (!canRetry) {
              itemStates[item.key] = {
                key: item.key,
                name: item.name,
                status: 'failed',
                attempt,
                maxRetries: safeRetries,
                error: message,
              };
              emit();
              break;
            }
            attempt += 1;
            itemStates[item.key] = {
              key: item.key,
              name: item.name,
              status: 'retrying',
              attempt,
              maxRetries: safeRetries,
              error: message,
            };
            emit();
            const backoffMs = 1100 * Math.pow(2, attempt - 1) + Math.floor(Math.random() * 250);
            await delayFn(backoffMs, signal);
          }
        }
      }
    }

    const workers = Array.from({ length: Math.min(safeConcurrency, targets.length) }, () => worker());
    await Promise.all(workers);

    if (signal.aborted) {
      for (const key of Object.keys(itemStates)) {
        if (itemStates[key].status === 'queued') {
          delete itemStates[key];
        }
      }
    }

    running = false;
    abortController = null;
    emit();
  }

  return {
    async start(
      allItems: SkillLibraryItem[],
      config: {
        mode: BatchAnalysisMode;
        selectedKeys?: ReadonlySet<string>;
        skipAnalyzedOnSelected?: boolean;
        concurrency: number;
        maxRetries: number;
      }
    ) {
      const targets = selectBatchTargets(
        allItems,
        config.mode,
        config.selectedKeys ?? new Set(),
        config.skipAnalyzedOnSelected ?? true
      );
      await runQueue(targets, config.concurrency, config.maxRetries, false);
    },

    async retryFailed(
      allItems: SkillLibraryItem[],
      config: { concurrency: number; maxRetries: number }
    ) {
      const failedKeys = new Set(
        Object.values(itemStates)
          .filter((s) => s.status === 'failed')
          .map((s) => s.key)
      );
      const targets = allItems.filter((item) => failedKeys.has(item.key) && isSkillAnalyzable(item));
      await runQueue(targets, config.concurrency, config.maxRetries, true);
    },

    async retrySingle(item: SkillLibraryItem, maxRetries: number) {
      if (!isSkillAnalyzable(item)) return;
      await runQueue([item], 1, maxRetries, true);
    },

    stop() {
      if (!running) return;
      cancelled = true;
      abortController?.abort();
      emit();
    },

    clear() {
      if (running) return;
      cancelled = false;
      itemStates = {};
      emit();
    },
  };
}
