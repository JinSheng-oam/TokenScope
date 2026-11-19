// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { fireEvent, render } from '@testing-library/svelte';
import { tick } from 'svelte';
import AppDialogHost from './AppDialogHost.svelte';
import { confirmDialog, promptDialog } from './dialogStore';

describe('AppDialogHost.svelte', () => {
  it('resolves confirmDialog true on confirm and false on cancel', async () => {
    const { getByRole, getByText, queryByRole } = render(AppDialogHost);

    const p1 = confirmDialog({
      title: '删除文件',
      message: '确定要删除吗？',
      confirmText: '确认删除',
      cancelText: '先不删',
      tone: 'danger',
    });
    await tick();

    expect(getByRole('dialog')).toBeTruthy();
    expect(getByText('确定要删除吗？')).toBeTruthy();

    await fireEvent.click(getByRole('button', { name: '确认删除' }));
    await expect(p1).resolves.toBe(true);
    await tick();
    expect(queryByRole('dialog')).toBeNull();

    const p2 = confirmDialog('放弃更改？');
    await tick();
    await fireEvent.click(getByRole('button', { name: '取消' }));
    await expect(p2).resolves.toBe(false);
  });

  it('queues sequential dialogs and resolves promptDialog input values', async () => {
    const { getByRole, container } = render(AppDialogHost);

    const promptPromise = promptDialog({
      title: '重命名 Skill',
      message: '请输入新名称：',
      defaultValue: 'old-skill',
      confirmText: '保存名称',
    });
    const confirmPromise = confirmDialog({ message: '第二个弹窗' });
    await tick();

    const input = container.querySelector('input[type="text"]') as HTMLInputElement;
    expect(input).toBeTruthy();
    expect(input.value).toBe('old-skill');

    await fireEvent.input(input, { target: { value: 'new-skill' } });
    await fireEvent.click(getByRole('button', { name: '保存名称' }));
    await expect(promptPromise).resolves.toBe('new-skill');

    await tick();
    // Second queued dialog opens automatically after the first closes
    expect(getByRole('dialog')).toBeTruthy();
    await fireEvent.click(getByRole('button', { name: '取消' }));
    await expect(confirmPromise).resolves.toBe(false);
  });
});
