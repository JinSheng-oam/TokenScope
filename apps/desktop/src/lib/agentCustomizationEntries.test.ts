import { describe, expect, it } from 'vitest';
import {
  isReferenceLine,
  isReferenced,
  addReference,
  removeReference,
  getMainEntry,
  instructionInfo,
  type Entry,
} from './agentCustomizationEntries';

describe('agentCustomizationEntries tests', () => {
  const dummyAuxiliary: Entry = {
    id: { provider: 'codex', root: 'codex-instructions', name: 'my-rules.md' },
    path: 'C:\\Users\\Jin\\.codex\\my-rules.md',
    kind: 'instruction',
    exists: true,
    editable: true,
    linked: false,
    enabled: true,
    instructionEntry: false,
  };

  const dummyMainAgents: Entry = {
    id: { provider: 'codex', root: 'codex-instructions', name: 'AGENTS.md' },
    path: 'C:\\Users\\Jin\\.codex\\AGENTS.md',
    kind: 'instruction',
    exists: true,
    editable: true,
    linked: false,
    enabled: true,
    instructionEntry: true,
  };

  const dummyMainOverride: Entry = {
    id: { provider: 'codex', root: 'codex-instructions', name: 'AGENTS.override.md' },
    path: 'C:\\Users\\Jin\\.codex\\AGENTS.override.md',
    kind: 'instruction',
    exists: false,
    editable: true,
    linked: false,
    enabled: true,
    instructionEntry: true,
  };

  describe('instructionInfo', () => {
    it('identifies auxiliary files properly with guidance', () => {
      const info = instructionInfo(dummyAuxiliary, [dummyMainAgents]);
      expect(info.tone).toBe('auxiliary');
      expect(info.label).toBe('辅助文件');
      expect(info.detail).toContain('@');
    });
  });

  describe('isReferenceLine', () => {
    const targetPath = 'C:\\Users\\Jin\\.codex\\my-rules.md';
    const targetName = 'my-rules.md';

    it('matches exact Windows path', () => {
      expect(isReferenceLine('@C:\\Users\\Jin\\.codex\\my-rules.md', targetPath, targetName)).toBe(true);
    });

    it('matches forward-slash normalized path', () => {
      expect(isReferenceLine('@C:/Users/Jin/.codex/my-rules.md', targetPath, targetName)).toBe(true);
    });

    it('matches quoted path', () => {
      expect(isReferenceLine('@"C:\\Users\\Jin\\.codex\\my-rules.md"', targetPath, targetName)).toBe(true);
      expect(isReferenceLine("@'C:/Users/Jin/.codex/my-rules.md'", targetPath, targetName)).toBe(true);
    });

    it('matches file name and relative path', () => {
      expect(isReferenceLine('@my-rules.md', targetPath, targetName)).toBe(true);
      expect(isReferenceLine('@./my-rules.md', targetPath, targetName)).toBe(true);
    });

    it('matches markdown link syntax', () => {
      expect(isReferenceLine('@[rules](C:/Users/Jin/.codex/my-rules.md)', targetPath, targetName)).toBe(true);
      expect(isReferenceLine('@[rules](my-rules.md)', targetPath, targetName)).toBe(true);
    });

    it('rejects other files and non-reference lines', () => {
      expect(isReferenceLine('@other-file.md', targetPath, targetName)).toBe(false);
      expect(isReferenceLine('Contact user@example.com for help', targetPath, targetName)).toBe(false);
      expect(isReferenceLine('# Just a header', targetPath, targetName)).toBe(false);
    });
  });

  describe('isReferenced', () => {
    it('detects reference in multiline markdown content', () => {
      const content = '# Title\n\nSome guidelines.\n\n@C:\\Users\\Jin\\.codex\\my-rules.md\n\nMore content.';
      expect(isReferenced(content, dummyAuxiliary)).toBe(true);
    });

    it('returns false when reference is absent', () => {
      const content = '# Title\n\nSome guidelines.\n\n@C:\\Users\\Jin\\.codex\\other.md\n';
      expect(isReferenced(content, dummyAuxiliary)).toBe(false);
    });
  });

  describe('addReference and removeReference', () => {
    it('adds reference to empty content', () => {
      const result = addReference('', '@C:\\Users\\Jin\\.codex\\my-rules.md');
      expect(result).toBe('@C:\\Users\\Jin\\.codex\\my-rules.md\n');
    });

    it('adds reference to existing content with proper spacing', () => {
      const initial = '# AGENTS.md\n\nExisting rule.';
      const added = addReference(initial, '@C:\\Users\\Jin\\.codex\\my-rules.md');
      expect(added).toBe('# AGENTS.md\n\nExisting rule.\n\n@C:\\Users\\Jin\\.codex\\my-rules.md\n');
    });

    it('removes target reference and leaves other references intact', () => {
      const initial = '# AGENTS.md\n\n@C:\\other.md\n@C:\\Users\\Jin\\.codex\\my-rules.md\n@C:\\keep-this.md\n';
      const removed = removeReference(initial, dummyAuxiliary);
      expect(removed).toBe('# AGENTS.md\n\n@C:\\other.md\n@C:\\keep-this.md\n');
    });
  });

  describe('getMainEntry', () => {
    it('prefers AGENTS.md when override does not exist', () => {
      const main = getMainEntry(dummyAuxiliary, [dummyMainOverride, dummyMainAgents]);
      expect(main?.id.name).toBe('AGENTS.md');
    });

    it('prefers AGENTS.override.md when it exists', () => {
      const existingOverride: Entry = { ...dummyMainOverride, exists: true };
      const main = getMainEntry(dummyAuxiliary, [existingOverride, dummyMainAgents]);
      expect(main?.id.name).toBe('AGENTS.override.md');
    });

    it('falls back gracefully to default preset entry if none exist yet', () => {
      const nonExistingAgents: Entry = { ...dummyMainAgents, exists: false };
      const main = getMainEntry(dummyAuxiliary, [dummyMainOverride, nonExistingAgents]);
      expect(main?.id.name).toBe('AGENTS.md');
    });
  });
});
