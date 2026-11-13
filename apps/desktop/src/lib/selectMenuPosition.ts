type Anchor = Pick<DOMRectReadOnly, 'left' | 'right' | 'top' | 'bottom' | 'width'>;

export function selectMenuPosition(
  anchor: Anchor,
  viewportWidth: number,
  viewportHeight: number,
  contentHeight: number,
  align: 'left' | 'right',
  desiredWidth = 156,
) {
  const edge = 8;
  const gap = 4;
  const width = Math.max(0, Math.min(viewportWidth - 2 * edge, Math.max(anchor.width, desiredWidth)));
  const preferredLeft = align === 'right' ? anchor.right - width : anchor.left;
  const left = Math.max(edge, Math.min(viewportWidth - edge - width, preferredLeft));
  const below = viewportHeight - anchor.bottom - gap - edge;
  const above = anchor.top - gap - edge;
  const desiredHeight = Math.min(242, contentHeight);
  const up = below < desiredHeight && above > below;

  return {
    width,
    left,
    top: up ? anchor.top - gap : anchor.bottom + gap,
    maxHeight: Math.max(0, Math.min(242, up ? above : below)),
    up,
  };
}
