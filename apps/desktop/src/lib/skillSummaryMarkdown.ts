export type InlineToken =
  | { type: 'text'; text: string }
  | { type: 'strong'; text: string }
  | { type: 'em'; text: string }
  | { type: 'code'; text: string };

export type SummaryBlock =
  | { type: 'heading'; level: number; tokens: InlineToken[] }
  | { type: 'paragraph'; tokens: InlineToken[] }
  | { type: 'list'; ordered: boolean; items: InlineToken[][] }
  | { type: 'code'; lang: string; code: string }
  | { type: 'blockquote'; tokens: InlineToken[] };

const HIDDEN_CODE_LANGS = new Set(['text', 'plaintext', 'txt', 'plain']);

export function parseInlineMarkdown(input: string): InlineToken[] {
  const tokens: InlineToken[] = [];
  const pattern = /`([^`\n]+)`|\*\*([^*\n]+(?:\*(?!\*)[^*\n]*)*)\*\*|__([^_\n]+(?:_(?!_)[^_\n]*)*)__|\*([^*\n]+)\*/g;
  let lastIndex = 0;

  for (const match of input.matchAll(pattern)) {
    const index = match.index ?? 0;
    if (index > lastIndex) {
      tokens.push({ type: 'text', text: input.slice(lastIndex, index) });
    }
    if (match[1] !== undefined) {
      tokens.push({ type: 'code', text: match[1] });
    } else if (match[2] !== undefined || match[3] !== undefined) {
      tokens.push({ type: 'strong', text: match[2] ?? match[3] });
    } else if (match[4] !== undefined) {
      tokens.push({ type: 'em', text: match[4] });
    }
    lastIndex = index + match[0].length;
  }

  if (lastIndex < input.length) {
    tokens.push({ type: 'text', text: input.slice(lastIndex) });
  }
  return tokens.length > 0 ? tokens : [{ type: 'text', text: input }];
}

function normalizeCodeLang(raw: string): string {
  const cleaned = raw.trim().toLowerCase();
  if (!cleaned || HIDDEN_CODE_LANGS.has(cleaned)) return '';
  return cleaned;
}

export function parseSummaryMarkdown(source: string): SummaryBlock[] {
  const normalized = source.replace(/\r\n?/g, '\n').trim();
  if (!normalized) return [];

  const lines = normalized.split('\n');
  const blocks: SummaryBlock[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];
    const trimmed = line.trim();

    if (!trimmed) {
      i += 1;
      continue;
    }

    // Fenced code block: ```lang ... ```
    const fenceMatch = trimmed.match(/^(`{3,}|~{3,})\s*([a-zA-Z0-9_+-]*)\s*$/);
    if (fenceMatch) {
      const fence = fenceMatch[1];
      const lang = normalizeCodeLang(fenceMatch[2] ?? '');
      const codeLines: string[] = [];
      i += 1;
      while (i < lines.length && !lines[i].trim().startsWith(fence)) {
        codeLines.push(lines[i]);
        i += 1;
      }
      if (i < lines.length) i += 1;
      const code = codeLines.join('\n').replace(/^\n+|\n+$/g, '');
      if (code) blocks.push({ type: 'code', lang, code });
      continue;
    }

    // Heading: # ... ####
    const headingMatch = trimmed.match(/^(#{1,4})\s+(.+)$/);
    if (headingMatch) {
      blocks.push({
        type: 'heading',
        level: headingMatch[1].length,
        tokens: parseInlineMarkdown(headingMatch[2].trim()),
      });
      i += 1;
      continue;
    }

    // Blockquote: > ...
    if (trimmed.startsWith('>')) {
      const quoteLines: string[] = [];
      while (i < lines.length && lines[i].trim().startsWith('>')) {
        quoteLines.push(lines[i].trim().replace(/^>\s?/, ''));
        i += 1;
      }
      blocks.push({
        type: 'blockquote',
        tokens: parseInlineMarkdown(quoteLines.join(' ').trim()),
      });
      continue;
    }

    // Unordered list: - / * / •
    if (/^[-*•]\s+/.test(trimmed)) {
      const items: InlineToken[][] = [];
      while (i < lines.length && /^[-*•]\s+/.test(lines[i].trim())) {
        items.push(parseInlineMarkdown(lines[i].trim().replace(/^[-*•]\s+/, '')));
        i += 1;
      }
      blocks.push({ type: 'list', ordered: false, items });
      continue;
    }

    // Ordered list: 1. ...
    if (/^\d+[.)]\s+/.test(trimmed)) {
      const items: InlineToken[][] = [];
      while (i < lines.length && /^\d+[.)]\s+/.test(lines[i].trim())) {
        items.push(parseInlineMarkdown(lines[i].trim().replace(/^\d+[.)]\s+/, '')));
        i += 1;
      }
      blocks.push({ type: 'list', ordered: true, items });
      continue;
    }

    // Paragraph (stop before blank lines or next block opener)
    const paraLines: string[] = [];
    while (i < lines.length) {
      const next = lines[i].trim();
      if (
        !next ||
        /^(`{3,}|~{3,})/.test(next) ||
        /^#{1,4}\s+/.test(next) ||
        next.startsWith('>') ||
        /^[-*•]\s+/.test(next) ||
        /^\d+[.)]\s+/.test(next)
      ) {
        break;
      }
      paraLines.push(next);
      i += 1;
    }
    if (paraLines.length > 0) {
      blocks.push({
        type: 'paragraph',
        tokens: parseInlineMarkdown(paraLines.join('\n')),
      });
    }
  }

  return blocks;
}

export function stripMarkdownToPlainText(source: string): string {
  return source
    .replace(/\r\n?/g, '\n')
    .replace(/```[\s\S]*?```/g, ' ')
    .replace(/^#{1,6}\s+/gm, '')
    .replace(/^>\s?/gm, '')
    .replace(/^[-*•]\s+/gm, '')
    .replace(/^\d+[.)]\s+/gm, '')
    .replace(/\*\*([^*]+)\*\*/g, '$1')
    .replace(/__([^_]+)__/g, '$1')
    .replace(/`([^`]+)`/g, '$1')
    .replace(/\*([^*]+)\*/g, '$1')
    .replace(/\s+/g, ' ')
    .trim();
}

