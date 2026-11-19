// @vitest-environment happy-dom
import { describe, expect, it, vi } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import CustomSelect from './CustomSelect.svelte';

describe('CustomSelect.svelte', () => {
  const options = [
    { key: 'all', label: '全部工具', count: 12 },
    { key: 'codex', label: 'Codex', subtext: 'CLI' },
    { key: 'claude', label: 'Claude Code', count: 5 },
  ];

  it('displays placeholder when no option matches and updates selection on click', async () => {
    const onChange = vi.fn();
    const { getByRole, getByText } = render(CustomSelect, {
      props: {
        options,
        value: '',
        placeholder: '请选择工具',
        ariaLabel: '工具筛选',
        onChange,
      },
    });

    const trigger = getByRole('combobox', { name: '工具筛选' });
    expect(trigger.textContent).toContain('请选择工具');
    expect(trigger.getAttribute('aria-expanded')).toBe('false');

    await fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('true');

    const codexOption = getByText('Codex');
    await fireEvent.click(codexOption);

    expect(onChange).toHaveBeenCalledWith('codex');
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
  });

  it('does not open menu when disabled', async () => {
    const { getByRole } = render(CustomSelect, {
      props: {
        options,
        value: 'all',
        ariaLabel: '禁用选择器',
        disabled: true,
      },
    });

    const trigger = getByRole('combobox', { name: '禁用选择器' });
    await fireEvent.click(trigger);
    expect(trigger.getAttribute('aria-expanded')).toBe('false');
  });
});
