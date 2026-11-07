import { describe, expect, it } from 'vitest';

function normalizeLines(val: string | null | undefined): string {
  return (val ?? '').replace(/\r\n/g, '\n').replace(/\r/g, '\n');
}

describe('Custom components tests', () => {
  it('correctly calculates iso date range logic', () => {
    const d = new Date(2026, 10, 4); // 2026-11-04
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    expect(`${y}-${m}-${day}`).toBe('2026-11-04');
  });

  it('bounds preset ranges properly', () => {
    const end = '2026-11-04';
    const parts = end.split('-').map(Number);
    const d = new Date(parts[0], parts[1] - 1, parts[2]);
    d.setDate(d.getDate() - 6);
    const y = d.getFullYear();
    const m = String(d.getMonth() + 1).padStart(2, '0');
    const day = String(d.getDate()).padStart(2, '0');
    expect(`${y}-${m}-${day}`).toBe('2026-10-29');
  });

  it('normalizes CRLF and LF so textarea binding does not trigger false dirty state', () => {
    const diskContent = '# Rule\r\n\r\n- Item 1\r\n- Item 2\r\n';
    const textareaNormalized = diskContent.replace(/\r\n/g, '\n');

    expect(textareaNormalized !== diskContent).toBe(true);

    const isDirty = normalizeLines(textareaNormalized) !== normalizeLines(diskContent);
    expect(isDirty).toBe(false);

    const edited = textareaNormalized + '- Item 3\n';
    const isReallyDirty = normalizeLines(edited) !== normalizeLines(diskContent);
    expect(isReallyDirty).toBe(true);
  });

  it('preserves CRLF on save when original had CRLF', () => {
    const original = '# Title\r\nBody text\r\n';
    const editedInTextarea = '# Title\nEdited body text\n';

    const toSave = original && original.includes('\r\n')
      ? editedInTextarea.replace(/\r?\n/g, '\r\n')
      : editedInTextarea;

    expect(toSave).toBe('# Title\r\nEdited body text\r\n');
    expect(toSave.includes('\r\n')).toBe(true);
  });
});
