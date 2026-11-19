import { describe, expect, it } from 'vitest';
import {
  createEmptyFormDraft,
  entryToFormDraft,
  formDraftToEntry,
  parseMcpJsonSnippet,
  validateFormDraft,
} from './mcpForm';

describe('mcpForm helpers', () => {
  it('validates stdio, remote, duplicate, and DeepSeek naming rules', () => {
    const draft = createEmptyFormDraft('stdio');
    expect(validateFormDraft(draft, 'codex', [], null)).toContain('服务器名称');

    draft.name = 'github';
    expect(validateFormDraft(draft, 'codex', ['github'], null)).toContain('已存在同名');
    expect(validateFormDraft(draft, 'codex', ['github'], 'github')).toContain('启动命令');

    draft.command = 'npx';
    expect(validateFormDraft(draft, 'codex', ['github'], 'github')).toBeNull();

    const dshInvalid = { ...draft, name: 'bad name!' };
    expect(validateFormDraft(dshInvalid, 'deepseek', [], null)).toContain('不能包含空白');
    const dshSymbol = { ...draft, name: 'bad.name' };
    expect(validateFormDraft(dshSymbol, 'deepseek', [], null)).toContain('DeepSeek');

    const remote = createEmptyFormDraft('http');
    remote.name = 'docs';
    remote.url = 'ftp://example.com';
    expect(validateFormDraft(remote, 'claude', [], null)).toContain('http://');
    remote.url = 'https://mcp.example.com/mcp';
    expect(validateFormDraft(remote, 'claude', [], null)).toBeNull();
  });

  it('converts form draft to provider-appropriate entry and round-trips', () => {
    const draft = createEmptyFormDraft('http');
    draft.name = 'context7';
    draft.url = 'https://mcp.example.com/http';
    draft.headers = [{ key: 'Authorization', value: 'Bearer secret' }];
    draft.enabled = false;

    const dshEntry = formDraftToEntry(draft, 'deepseek');
    expect(dshEntry.transport).toBe('streamable-http');
    expect(dshEntry.headers.Authorization).toBe('Bearer secret');
    expect(dshEntry.enabled).toBe(false);

    const roundTripped = entryToFormDraft(dshEntry);
    expect(roundTripped.transport).toBe('http');
    expect(roundTripped.url).toBe('https://mcp.example.com/http');
    expect(roundTripped.enabled).toBe(false);
  });

  it('parses mcpServers and direct JSON snippets into graphical form draft', () => {
    const wrapped = parseMcpJsonSnippet(
      JSON.stringify({
        mcpServers: {
          filesystem: {
            command: 'npx',
            args: ['-y', '@modelcontextprotocol/server-filesystem', '/tmp'],
            env: { NODE_ENV: 'production' },
          },
          second: { url: 'https://example.com/mcp' },
        },
      }),
    );
    expect('draft' in wrapped).toBe(true);
    if ('draft' in wrapped) {
      expect(wrapped.draft.name).toBe('filesystem');
      expect(wrapped.draft.command).toBe('npx');
      expect(wrapped.draft.args).toEqual(['-y', '@modelcontextprotocol/server-filesystem', '/tmp']);
      expect(wrapped.draft.env).toEqual([{ key: 'NODE_ENV', value: 'production', reveal: false }]);
      expect(wrapped.extraCount).toBe(1);
    }

    const directRemote = parseMcpJsonSnippet(
      JSON.stringify({
        serverUrl: 'https://mcp.example.com/sse',
        type: 'sse',
        headers: { 'X-Token': 'abc' },
      }),
    );
    expect('draft' in directRemote).toBe(true);
    if ('draft' in directRemote) {
      expect(directRemote.draft.transport).toBe('sse');
      expect(directRemote.draft.url).toBe('https://mcp.example.com/sse');
      expect(directRemote.draft.headers).toEqual([{ key: 'X-Token', value: 'abc', reveal: false }]);
    }
  });
});
