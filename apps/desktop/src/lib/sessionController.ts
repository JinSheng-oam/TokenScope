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

  function update(patch: Partial<SessionControllerState>) {
    state.update((current) => ({ ...current, ...patch }));
  }

  function clearDebounce() {
    if (debounceTimer !== undefined) clearTimeout(debounceTimer);
    debounceTimer = undefined;
  }

  function resetForRangeChange() {
    requestSequence += 1;
    clearDebounce();
    update({ offset: 0 });
  }

  function schedule() {
    requestSequence += 1;
    update({ offset: 0 });
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

  function setPage(nextOffset: number) {
    const current = get(state);
    if (nextOffset < 0 || nextOffset === current.offset) return;
    update({ offset: nextOffset });
    void load();
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
    clearDebounce();
    update({ pageSize: size, offset: 0 });
    void load();
  }

  async function load() {
    if (!dependencies.isRuntimeAvailable() || !dependencies.isActive()) return;
    const sequence = ++requestSequence;
    const current = get(state);
    update({ loading: true, error: '' });
    const filters = {
      search: current.search,
      toolId: current.toolFilter,
      modelId: current.modelFilter,
      projectId: current.projectFilter,
      pageSize: current.pageSize,
    };
    const range = dependencies.range();
    const fetchPage = dependencies.loadPage ?? getSessionAnalysis;
    const loadPage = (offset: number) => fetchPage(
      range,
      offset,
      filters,
      range === 'custom' ? dependencies.customStart() : undefined,
      range === 'custom' ? dependencies.customEnd() : undefined,
    );

    try {
      let result = await loadPage(current.offset);
      if (sequence !== requestSequence) return;
      const lastValidOffset = result.page.total === 0
        ? 0
        : Math.floor((result.page.total - 1) / result.page.limit) * result.page.limit;
      if (get(state).offset > lastValidOffset) {
        update({ offset: lastValidOffset });
        result = await loadPage(lastValidOffset);
        if (sequence !== requestSequence) return;
      }
      if (sequence !== requestSequence) return;
      update({ analysis: result });
    } catch (error) {
      if (sequence === requestSequence) {
        update({ error: error instanceof Error ? error.message : String(error) });
      }
    } finally {
      if (sequence === requestSequence) update({ loading: false });
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
      await dependencies.refreshDashboard(false);
    } catch (error) {
      update({ actionError: error instanceof Error ? error.message : String(error) });
    } finally {
      update({ deletingKey: '' });
    }
  }

  function dispose() {
    requestSequence += 1;
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
