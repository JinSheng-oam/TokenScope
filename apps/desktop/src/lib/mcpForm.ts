import type { Provider } from './agentCustomizationEntries';

export type McpTransport = 'stdio' | 'http' | 'sse' | 'streamable-http';

export interface McpServerEntry {
  name: string;
  transport: McpTransport | string;
  command: string | null;
  args: string[];
  url: string | null;
  env: Record<string, string>;
  headers: Record<string, string>;
  cwd: string | null;
  enabled: boolean;
  hasExtraFields?: boolean;
}

export interface McpView {
  path: string;
  format: 'TOML' | 'JSON';
  draft: string;
  revision: string;
  servers: McpServerEntry[];
}

export interface KvRow {
  key: string;
  value: string;
  reveal?: boolean;
}

export interface McpFormDraft {
  name: string;
  transport: 'stdio' | 'http' | 'sse';
  command: string;
  args: string[];
  url: string;
  cwd: string;
  env: KvRow[];
  headers: KvRow[];
  enabled: boolean;
}

const DEEPSEEK_NAME_RE = /^[A-Za-z0-9_-]{1,32}$/;

export function createEmptyFormDraft(transport: 'stdio' | 'http' | 'sse' = 'stdio'): McpFormDraft {
  return {
    name: '',
    transport,
    command: '',
    args: [],
    url: '',
    cwd: '',
    env: [],
    headers: [],
    enabled: true,
  };
}

export function entryToFormDraft(entry: McpServerEntry): McpFormDraft {
  const transport: 'stdio' | 'http' | 'sse' =
    entry.transport === 'sse'
      ? 'sse'
      : entry.transport === 'http' || entry.transport === 'streamable-http'
        ? 'http'
        : 'stdio';
  return {
    name: entry.name,
    transport,
    command: entry.command ?? '',
    args: [...entry.args],
    url: entry.url ?? '',
    cwd: entry.cwd ?? '',
    env: Object.entries(entry.env ?? {}).map(([key, value]) => ({ key, value, reveal: false })),
    headers: Object.entries(entry.headers ?? {}).map(([key, value]) => ({ key, value, reveal: false })),
    enabled: entry.enabled !== false,
  };
}

export function formDraftToEntry(draft: McpFormDraft, provider: Provider): McpServerEntry {
  const isRemote = draft.transport !== 'stdio';
  const transport: McpTransport = isRemote
    ? provider === 'deepseek'
      ? 'streamable-http'
      : draft.transport
    : 'stdio';
  const env: Record<string, string> = {};
  for (const row of draft.env) {
    const key = row.key.trim();
    if (key) env[key] = row.value;
  }
  const headers: Record<string, string> = {};
  for (const row of draft.headers) {
    const key = row.key.trim();
    if (key) headers[key] = row.value;
  }
  return {
    name: draft.name.trim(),
    transport,
    command: isRemote ? null : draft.command.trim() || null,
    args: isRemote ? [] : draft.args.map((a) => a.trim()).filter(Boolean),
    url: isRemote ? draft.url.trim() || null : null,
    env: isRemote ? {} : env,
    headers: isRemote ? headers : {},
    cwd: isRemote ? null : draft.cwd.trim() || null,
    enabled: draft.enabled,
  };
}

export function validateFormDraft(
  draft: McpFormDraft,
  provider: Provider,
  existingNames: string[],
  originalName: string | null,
): string | null {
  const name = draft.name.trim();
  if (!name) return '请填写服务器名称';
  if (name.length > 128 || /\s/.test(name)) {
    return '服务器名称不能包含空白字符，且长度不超过 128';
  }
  if (provider === 'deepseek' && !DEEPSEEK_NAME_RE.test(name)) {
    return 'DeepSeek 服务器名称仅允许 1–32 个英文字母、数字、下划线或连字符';
  }
  if (name !== originalName && existingNames.includes(name)) {
    return `已存在同名 MCP 服务器：${name}`;
  }
  if (draft.transport === 'stdio') {
    if (!draft.command.trim()) return '请填写启动命令（如 npx、uvx、node）';
    for (const row of draft.env) {
      const k = row.key.trim();
      if (!k) continue;
      if (k.includes('=') || /\s/.test(k)) return `环境变量名无效：${k}`;
    }
  } else {
    if (provider === 'deepseek' && draft.transport === 'sse') {
      return 'DeepSeek 仅支持 stdio 或 HTTP (streamable-http)';
    }
    const url = draft.url.trim();
    if (!url) return '请填写远端服务地址（URL）';
    if (!/^https?:\/\/\S+$/.test(url)) {
      return '服务地址必须是以 http:// 或 https:// 开头的有效 URL';
    }
    for (const row of draft.headers) {
      const k = row.key.trim();
      if (!k) continue;
      if (/\s/.test(k)) return `请求头名称无效：${k}`;
    }
  }
  return null;
}

export function formatServerSummary(entry: McpServerEntry): string {
  if (entry.transport !== 'stdio' && entry.url) {
    return entry.url;
  }
  const parts = [entry.command ?? '', ...entry.args].filter(Boolean);
  return parts.join(' ') || '未配置命令或地址';
}

export function transportBadgeLabel(transport: string): string {
  if (transport === 'streamable-http' || transport === 'http') return 'HTTP';
  if (transport === 'sse') return 'SSE';
  return 'STDIO';
}

export function parseMcpJsonSnippet(
  raw: string,
): { draft: McpFormDraft; extraCount: number } | { error: string } {
  const trimmed = raw.trim();
  if (!trimmed) return { error: '请先粘贴 JSON 配置片段' };
  let parsed: unknown;
  try {
    parsed = JSON.parse(trimmed);
  } catch {
    return { error: 'JSON 格式无效，请检查引号或逗号' };
  }
  if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
    return { error: '请粘贴 JSON 对象（如 mcpServers 或单个服务器配置）' };
  }

  const root = parsed as Record<string, unknown>;
  let name = '';
  let config: Record<string, unknown> | null = null;
  let extraCount = 0;

  const isServerConfig = (obj: Record<string, unknown>) =>
    typeof obj.command === 'string' ||
    typeof obj.url === 'string' ||
    typeof obj.serverUrl === 'string' ||
    Array.isArray(obj.args);

  if (root.mcpServers && typeof root.mcpServers === 'object' && !Array.isArray(root.mcpServers)) {
    const entries = Object.entries(root.mcpServers as Record<string, unknown>).filter(
      ([, v]) => v && typeof v === 'object' && !Array.isArray(v),
    );
    if (entries.length === 0) return { error: 'mcpServers 中未找到有效的服务器对象' };
    name = entries[0][0];
    config = entries[0][1] as Record<string, unknown>;
    extraCount = Math.max(0, entries.length - 1);
  } else if (isServerConfig(root)) {
    config = root;
  } else {
    const entries = Object.entries(root).filter(
      ([, v]) => v && typeof v === 'object' && !Array.isArray(v),
    );
    if (entries.length > 0 && isServerConfig(entries[0][1] as Record<string, unknown>)) {
      name = entries[0][0];
      config = entries[0][1] as Record<string, unknown>;
      extraCount = Math.max(0, entries.length - 1);
    }
  }

  if (!config) {
    return { error: '未识别到 command 或 url 字段，请检查 JSON 结构' };
  }

  const command = typeof config.command === 'string' ? config.command.trim() : '';
  const url =
    typeof config.url === 'string'
      ? config.url.trim()
      : typeof config.serverUrl === 'string'
        ? config.serverUrl.trim()
        : '';
  const rawTransport =
    typeof config.transport === 'string'
      ? config.transport
      : typeof config.type === 'string'
        ? config.type
        : '';
  const transport: 'stdio' | 'http' | 'sse' =
    rawTransport === 'sse'
      ? 'sse'
      : rawTransport === 'http' || rawTransport === 'streamable-http' || (url && !command)
        ? 'http'
        : 'stdio';

  const args = Array.isArray(config.args)
    ? config.args.filter((item): item is string => typeof item === 'string')
    : [];
  const cwd = typeof config.cwd === 'string' ? config.cwd.trim() : '';
  const env: KvRow[] =
    config.env && typeof config.env === 'object' && !Array.isArray(config.env)
      ? Object.entries(config.env as Record<string, unknown>)
          .filter(([, v]) => typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean')
          .map(([key, v]) => ({ key, value: String(v), reveal: false }))
      : [];
  const headers: KvRow[] =
    config.headers && typeof config.headers === 'object' && !Array.isArray(config.headers)
      ? Object.entries(config.headers as Record<string, unknown>)
          .filter(([, v]) => typeof v === 'string' || typeof v === 'number' || typeof v === 'boolean')
          .map(([key, v]) => ({ key, value: String(v), reveal: false }))
      : [];
  const enabled =
    typeof config.enabled === 'boolean'
      ? config.enabled
      : typeof config.disabled === 'boolean'
        ? !config.disabled
        : true;

  return {
    draft: {
      name,
      transport,
      command,
      args,
      url,
      cwd,
      env,
      headers,
      enabled,
    },
    extraCount,
  };
}
