import { describe, expect, it } from 'vitest';
import { selectMenuPosition } from './selectMenuPosition';

describe('select menu placement', () => {
  it('opens above a footer control and expands beyond a narrow trigger', () => {
    const anchor = { left: 850, right: 940, top: 650, bottom: 684, width: 90 };
    expect(selectMenuPosition(anchor, 1000, 720, 135, 'right')).toEqual({
      width: 156, left: 784, top: 646, maxHeight: 242, up: true,
    });
  });

  it('keeps the dropdown in the viewport and makes long menus scroll', () => {
    const anchor = { left: 345, right: 388, top: 110, bottom: 144, width: 43 };
    expect(selectMenuPosition(anchor, 400, 300, 900, 'left')).toEqual({
      width: 156, left: 236, top: 148, maxHeight: 144, up: false,
    });
  });
});
