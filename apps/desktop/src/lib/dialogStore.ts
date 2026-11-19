import { writable } from 'svelte/store';

export type ConfirmDialogOptions = {
  title?: string;
  message: string;
  confirmText?: string;
  cancelText?: string;
  tone?: 'default' | 'danger';
};

export type PromptDialogOptions = {
  title?: string;
  message: string;
  defaultValue?: string;
  placeholder?: string;
  confirmText?: string;
  cancelText?: string;
};

export type ActiveDialogRequest =
  | {
      id: number;
      kind: 'confirm';
      title: string;
      message: string;
      confirmText: string;
      cancelText: string;
      tone: 'default' | 'danger';
      resolve: (confirmed: boolean) => void;
    }
  | {
      id: number;
      kind: 'prompt';
      title: string;
      message: string;
      defaultValue: string;
      placeholder: string;
      confirmText: string;
      cancelText: string;
      resolve: (value: string | null) => void;
    };

let nextId = 1;
let queue: ActiveDialogRequest[] = [];
let hostMounted = 0;

const activeDialog = writable<ActiveDialogRequest | null>(null);

export const appDialog = {
  subscribe: activeDialog.subscribe,
};

export function registerDialogHost(): () => void {
  hostMounted += 1;
  return () => {
    hostMounted = Math.max(0, hostMounted - 1);
    if (hostMounted === 0 && queue.length > 0) {
      // Cancel pending requests cleanly when the host unmounts.
      const pending = queue;
      queue = [];
      activeDialog.set(null);
      for (const req of pending) {
        if (req.kind === 'confirm') req.resolve(false);
        else req.resolve(null);
      }
    }
  };
}

function enqueue(request: ActiveDialogRequest): void {
  queue = [...queue, request];
  if (queue.length === 1) {
    activeDialog.set(request);
  }
}

export function resolveActiveDialog(result: boolean | string | null): void {
  const [current, ...rest] = queue;
  if (!current) return;
  queue = rest;
  activeDialog.set(rest[0] ?? null);
  if (current.kind === 'confirm') {
    current.resolve(Boolean(result));
  } else {
    current.resolve(typeof result === 'string' ? result : null);
  }
}

export function confirmDialog(input: string | ConfirmDialogOptions): Promise<boolean> {
  const opts: ConfirmDialogOptions = typeof input === 'string' ? { message: input } : input;
  if (hostMounted === 0) {
    if (typeof window !== 'undefined' && typeof window.confirm === 'function') {
      return Promise.resolve(window.confirm(opts.message));
    }
    return Promise.resolve(false);
  }
  return new Promise<boolean>((resolve) => {
    enqueue({
      id: nextId++,
      kind: 'confirm',
      title: opts.title ?? (opts.tone === 'danger' ? '确认操作' : '请确认'),
      message: opts.message,
      confirmText: opts.confirmText ?? (opts.tone === 'danger' ? '确定删除' : '确定'),
      cancelText: opts.cancelText ?? '取消',
      tone: opts.tone ?? 'default',
      resolve,
    });
  });
}

export function promptDialog(opts: PromptDialogOptions): Promise<string | null> {
  if (hostMounted === 0) {
    if (typeof window !== 'undefined' && typeof window.prompt === 'function') {
      return Promise.resolve(window.prompt(opts.message, opts.defaultValue ?? ''));
    }
    return Promise.resolve(null);
  }
  return new Promise<string | null>((resolve) => {
    enqueue({
      id: nextId++,
      kind: 'prompt',
      title: opts.title ?? '输入信息',
      message: opts.message,
      defaultValue: opts.defaultValue ?? '',
      placeholder: opts.placeholder ?? '',
      confirmText: opts.confirmText ?? '确定',
      cancelText: opts.cancelText ?? '取消',
      resolve,
    });
  });
}
