import { get, writable } from 'svelte/store';
import { deleteSession, getSessionAnalysis } from './api';
import { formatNumber } from './format';
import type {
  DashboardRange,
  SessionAnalysisPage,
  SessionUsage,
} from './types';

export type SessionControllerState = {
  analysis: SessionAnalysisPage | null;
  search: string;
  toolFilter: string;
  modelFilter: string;
  projectFilter: string;
  pageSize: number;
  loading: boolean;
  error: string;
  offset: number;
  pendingDeleteKey: string;
  deletingKey: string;
  actionMessage: string;
  actionError: string;
};

type SessionControllerDependencies = {
  isRuntimeAvailable: () => boolean;
  isActive: () => boolean;
  range: () => DashboardRange;
  customStart: () => string;
  customEnd: () => string;
  refreshDashboard: (showSpinner?: boolean) => Promise<void>;
  loadPage?: typeof getSessionAnalysis;
  deleteUsage?: typeof deleteSession;
};

export const SESSION_PAGE_SIZES = [5, 6, 8, 10, 15, 20, 50, 100] as const;
const DEFAULT_PAGE_SIZE = 50;
const MAX_BATCH_SIZE = 100;
const CACHED_PAGES = 6;
const PAGE_CACHE_MS = 60_000;
const PAGE_SIZE_STORAGE_KEY = 'tokenscope.sessionPageSize';

function restorePageSize(): number {
  try {
    const raw = typeof localStorage !== 'undefined' ? Number(localStorage.getItem(PAGE_SIZE_STORAGE_KEY)) : NaN;
    return (SESSION_PAGE_SIZES as readonly number[]).includes(raw) ? raw : DEFAULT_PAGE_SIZE;
  } catch {
    return DEFAULT_PAGE_SIZE;
  }
}

const INITIAL_STATE: SessionControllerState = {
  analysis: null,
  search: '',
  toolFilter: '',
  modelFilter: '',
  projectFilter: '',
  pageSize: DEFAULT_PAGE_SIZE,
  loading: false,
  error: '',
  offset: 0,
  pendingDeleteKey: '',
  deletingKey: '',
  actionMessage: '',
  actionError: '',
};

export function createSessionController(dependencies: SessionControllerDependencies) {
  const state = writable<SessionControllerState>({ ...INITIAL_STATE, pageSize: restorePageSize() });
  let debounceTimer: ReturnType<typeof setTimeout> | undefined;
  let requestSequence = 0;
  let pendingKey: string | undefined;
  let loadedKey: string | undefined;
  let knownDataRevision: number | undefined;
  const pages = new Map<string, { result: SessionAnalysisPage; expires: number }>();

  function remember(key: string, result: SessionAnalysisPage) {
    pages.delete(key);
    pages.set(key, { result, expires: Date.now() + PAGE_CACHE_MS });
    if (pages.size > CACHED_PAGES) {
      const first = pages.keys().next();
      if (!first.done) pages.delete(first.value);
    }
  }

  function requestKey(current: SessionControllerState, range: DashboardRange): string {
    return JSON.stringify({
      range,
      startDate: range === 'custom' ? dependencies.customStart() : '',
      endDate: range === 'custom' ? dependencies.customEnd() : '',
      offset: current.offset,
      pageSize: current.pageSize,
      search: current.search.trim(),
      toolId: current.toolFilter,
      modelId: current.modelFilter,
      projectId: current.projectFilter,
      dataRevision: knownDataRevision ?? null,
    });
  }

  function update(patch: Partial<SessionControllerState>) {
    state.update((current) => ({ ...current, ...patch }));
  }

  function clearDebounce() {
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    debounceTimer = undefined;
  }

  function resetForRangeChange() {
    requestSequence += 1;
    pendingKey = undefined;
    loadedKey = undefined;
    pages.clear();
    clearDebounce();
    update({ offset: 0, loading: false });
  }

  function schedule() {
    pages.clear();
    requestSequence += 1;
    pendingKey = undefined;
    update({ offset: 0, loading: false });
    clearDebounce();
    debounceTimer = setTimeout(() => {
      debounceTimer = undefined;
      void load();
    }, 220);
  }

  function clearFilters() {
    update({ search: '', toolFilter: '', modelFilter: '', projectFilter: '' });
    schedule();
  }

  async function setPage(nextOffset: number) {
    const current = get(state);
    if (nextOffset < 0 || nextOffset === current.offset) return;
    await load(false, undefined, nextOffset);
  }

  function setPageSize(size: number) {
    if (!(SESSION_PAGE_SIZES as readonly number[]).includes(size)) return;
    if (get(state).pageSize === size) return;
    try {
      localStorage.setItem(PAGE_SIZE_STORAGE_KEY, String(size));
    } catch {
      // Persisting the preference is best-effort.
    }
    requestSequence += 1;
    pendingKey = undefined;
    clearDebounce();
    pages.clear();
    update({ pageSize: size, offset: 0 });
    void load();
  }

  async function load(force = false, dataRevision?: number, requestedOffset?: number) {
    if (dataRevision !== undefined && dataRevision !== knownDataRevision) {
      knownDataRevision = dataRevision;
      pages.clear();
    }
    if (!dependencies.isRuntimeAvailable() || !dependencies.isActive()) return;
    if (debounceTimer !== undefined && !force) return;
    if (force) {
      clearDebounce();
      pages.clear();
    }

    const previous = get(state);
    const current = requestedOffset === undefined ? previous : { ...previous, offset: requestedOffset };
    const range = dependencies.range();
    const key = requestKey(current, range);
    if (!force && (key === pendingKey || key === loadedKey)) return;
    const sequence = ++requestSequence;
    const cached = pages.get(key);
    if (!force && cached && cached.expires > Date.now()) {
      pendingKey = undefined;
      loadedKey = key;
      update({ offset: current.offset, analysis: cached.result, loading: false, error: '' });
      return;
    }
    pendingKey = key;
    update({ offset: current.offset, loading: true, error: '' });
    const filters = {
      search: current.search.trim(),
      toolId: current.toolFilter,
      modelId: current.modelFilter,
      projectId: current.projectFilter,
      pageSize: current.pageSize * (current.pageSize * 2 <= MAX_BATCH_SIZE ? 2 : 1),
    };
    const fetchPage = dependencies.loadPage ?? getSessionAnalysis;
    const startDate = range === 'custom' ? dependencies.customStart() : undefined;
    const endDate = range === 'custom' ? dependencies.customEnd() : undefined;
    const loadBatch = (offset: number) => fetchPage(range, offset, filters, startDate, endDate);

    try {
      let offset = current.offset;
      let batch = await loadBatch(offset);
      if (sequence !== requestSequence) return;
      const lastValidOffset = batch.page.total === 0
        ? 0
        : Math.floor((batch.page.total - 1) / current.pageSize) * current.pageSize;
      if (offset > lastValidOffset) {
        offset = lastValidOffset;
        batch = await loadBatch(offset);
        if (sequence !== requestSequence) return;
      }
      const result = {
        ...batch,
        page: { ...batch.page, offset, limit: current.pageSize, sessions: batch.page.sessions.slice(0, current.pageSize) },
      };
      loadedKey = requestKey({ ...current, offset }, range);
      remember(loadedKey, result);
      if (batch.page.sessions.length > current.pageSize) {
        const nextOffset = offset + current.pageSize;
        remember(requestKey({ ...current, offset: nextOffset }, range), {
          ...batch,
          page: { ...result.page, offset: nextOffset, sessions: batch.page.sessions.slice(current.pageSize) },
        });
      }
      update({ offset, analysis: result, loading: false });
    } catch (error) {
      if (sequence === requestSequence) {
        const fallback = requestedOffset !== undefined ? previous.analysis?.page.offset ?? previous.offset : current.offset;
        update({ offset: fallback, error: error instanceof Error ? error.message : String(error) });
      }
    } finally {
      if (sequence === requestSequence) {
        pendingKey = undefined;
        if (get(state).loading) update({ loading: false });
      }
    }
  }

  function actionKey(session: Pick<SessionUsage, 'tool_id' | 'session_id'>): string {
    return `${session.tool_id}\u001f${session.session_id}`;
  }

  function projectLabel(projectId: string): string {
    const found = get(state).analysis?.filter_options.projects.find((option) => option.key === projectId)?.label;
    if (found && found !== '项目名称未提供' && !found.startsWith('prj_') && found.trim().length > 0) {
      return found;
    }
    const clean = projectId.replace(/[\/\\]+$/, '');
    const parts = clean.split(/[\/\\]/);
    const last = parts[parts.length - 1];
    if (last && last.length > 0 && last !== 'unknown' && !last.startsWith('prj_')) {
      return last;
    }
    return '';
  }

  function requestDelete(session: SessionUsage) {
    update({
      actionMessage: '',
      actionError: '',
      pendingDeleteKey: actionKey(session),
    });
  }

  function cancelDelete() {
    update({ pendingDeleteKey: '', actionError: '' });
  }

  async function confirmDelete(session: SessionUsage) {
    const key = actionKey(session);
    if (get(state).deletingKey) return;
    update({ deletingKey: key, actionError: '', actionMessage: '' });
    try {
      const deleteUsage = dependencies.deleteUsage ?? deleteSession;
      const deletedRecords = await deleteUsage(session.tool_id, session.session_id);
      update({
        pendingDeleteKey: '',
        actionMessage: `会话已从本机统计移除（${formatNumber(deletedRecords)} 条记录）；原应用聊天未改动。`,
      });
      loadedKey = undefined;
      pages.clear();
      await dependencies.refreshDashboard(false);
    } catch (error) {
      update({ actionError: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ deletingKey: '' });
    }
  }

  function dispose() {
    requestSequence += 1;
    pages.clear();
    clearDebounce();
  }

  return {
    state,
    resetForRangeChange,
    schedule,
    clearFilters,
    setPage,
    setPageSize,
    load,
    actionKey,
    projectLabel,
    requestDelete,
    cancelDelete,
    confirmDelete,
    dispose,
  };
}
