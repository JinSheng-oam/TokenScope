// @vitest-environment happy-dom
import { describe, expect, it } from 'vitest';
import { render } from '@testing-library/svelte';
import LoadingStatus from './LoadingStatus.svelte';

describe('LoadingStatus.svelte', () => {
  it('renders loading message and optional stage pill', () => {
    const { getByText, queryByText, rerender } = render(LoadingStatus, {
      props: { message: '正在加载概览数据' },
    });

    expect(getByText('正在加载概览数据')).toBeTruthy();
    expect(queryByText('缓存命中')).toBeNull();

    rerender({ message: '正在同步', stage: '缓存命中' });
    expect(getByText('正在同步')).toBeTruthy();
    expect(getByText('缓存命中')).toBeTruthy();
  });
});
