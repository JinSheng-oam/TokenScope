export interface AdaptiveFitRowOptions {
  /** Minimum comfortable width (px) for flexible title elements (.rank-title / .recent-session-main) before compacting numbers. */
  minTitleWidth?: number;
}

export interface AdaptiveFitTextOptions {
  /** Ordered candidates from full exact string to most compact string. */
  candidates: string[];
  /** Lower priority compacts first when a row has multiple adaptive numbers (e.g. tokens = 0, cost = 1). */
  priority?: number;
  /** Optional base tooltip text; when compacted, exact value is shown or appended. */
  title?: string;
}

interface SlotState {
  el: HTMLElement;
  candidates: string[];
  priority: number;
  baseTitle?: string;
  index: number;
}

interface RowController {
  fit: () => void;
  scheduleFit: () => void;
}

const ROW_ATTR = 'data-adaptive-row';
const SLOT_ATTR = 'data-adaptive-fit';

const slotRegistry = new WeakMap<HTMLElement, SlotState>();
const rowRegistry = new WeakMap<HTMLElement, RowController>();

function applySlotState(slot: SlotState, index: number) {
  const clamped = Math.max(0, Math.min(index, Math.max(0, slot.candidates.length - 1)));
  slot.index = clamped;
  const text = slot.candidates[clamped] ?? '';
  if (slot.el.textContent !== text) {
    slot.el.textContent = text;
  }

  const fullText = slot.candidates[0] ?? '';
  if (clamped > 0 && fullText) {
    slot.el.title = slot.baseTitle ? `${slot.baseTitle}（精确值：${fullText}）` : fullText;
  } else if (slot.baseTitle) {
    slot.el.title = slot.baseTitle;
  } else {
    slot.el.removeAttribute('title');
  }
}

function isFlexibleTitle(el: HTMLElement): boolean {
  return el.classList.contains('rank-title') || el.classList.contains('recent-session-main');
}

function isElementTextClipped(el: HTMLElement): boolean {
  if (el.scrollWidth > el.clientWidth + 1) return true;
  const strong = el.querySelector('strong');
  if (strong instanceof HTMLElement && strong.scrollWidth > strong.clientWidth + 1) return true;
  return false;
}

function isRowOverflowing(rowEl: HTMLElement, options: AdaptiveFitRowOptions): boolean {
  if (rowEl.scrollWidth > rowEl.clientWidth + 1) return true;

  const minTitleWidth = options.minTitleWidth ?? 0;
  for (const child of Array.from(rowEl.children)) {
    if (!(child instanceof HTMLElement)) continue;
    if (isFlexibleTitle(child)) {
      if (minTitleWidth > 0 && child.clientWidth < minTitleWidth && isElementTextClipped(child)) {
        return true;
      }
      continue;
    }

    if (child.scrollWidth > child.clientWidth + 1) return true;

    // Check immediate children of grouped right-side metadata (e.g. .rank-meta > .small-cost-tag)
    if (child.classList.contains('rank-meta') || child.classList.contains('recent-session-right')) {
      for (const grandChild of Array.from(child.children)) {
        if (grandChild instanceof HTMLElement && grandChild.scrollWidth > grandChild.clientWidth + 1) {
          return true;
        }
      }
    }
  }

  return false;
}

function findOwnerRow(el: HTMLElement): RowController | undefined {
  const rowEl = el.closest(`[${ROW_ATTR}]`);
  return rowEl instanceof HTMLElement ? rowRegistry.get(rowEl) : undefined;
}

/**
 * Svelte action placed on a container row (e.g. .metric-foot, .rank-row, .recent-session-row, .device-token-val).
 * Observes the container's width via ResizeObserver and progressively compacts descendant adaptive numbers
 * (full -> M -> B) whenever content would otherwise clip or squeeze titles.
 */
export function adaptiveFitRow(rowEl: HTMLElement, initialOptions: AdaptiveFitRowOptions = {}) {
  let options = initialOptions;
  let rafId: number | null = null;

  rowEl.setAttribute(ROW_ATTR, '');

  const collectSlots = (): SlotState[] => {
    const result: SlotState[] = [];
    for (const node of Array.from(rowEl.querySelectorAll(`[${SLOT_ATTR}]`))) {
      if (!(node instanceof HTMLElement)) continue;
      // Ensure nested adaptive rows do not steal each other's slots
      if (node.closest(`[${ROW_ATTR}]`) !== rowEl) continue;
      const slot = slotRegistry.get(node);
      if (slot) result.push(slot);
    }
    return result;
  };

  const fit = () => {
    if (rowEl.clientWidth <= 0) return;
    const slots = collectSlots();
    if (slots.length === 0) return;

    // 1. Start from full exact values so widening the window restores full precision
    for (const slot of slots) {
      applySlotState(slot, 0);
    }

    // 2. Step down one candidate at a time until nothing overflows
    let guard = slots.reduce((sum, s) => sum + s.candidates.length, 0) + 2;
    while (guard-- > 0 && isRowOverflowing(rowEl, options)) {
      let best: SlotState | null = null;
      for (const slot of slots) {
        if (slot.index + 1 >= slot.candidates.length) continue;
        if (!best) {
          best = slot;
          continue;
        }
        if (slot.priority !== best.priority) {
          if (slot.priority < best.priority) best = slot;
          continue;
        }
        const curLen = (slot.candidates[slot.index] ?? '').length;
        const bestLen = (best.candidates[best.index] ?? '').length;
        if (curLen > bestLen) best = slot;
      }

      if (!best) break;
      applySlotState(best, best.index + 1);
    }
  };

  const scheduleFit = () => {
    if (rafId !== null) cancelAnimationFrame(rafId);
    rafId = requestAnimationFrame(() => {
      rafId = null;
      fit();
    });
  };

  const controller: RowController = { fit, scheduleFit };
  rowRegistry.set(rowEl, controller);

  let resizeObserver: ResizeObserver | null = null;
  if (typeof ResizeObserver !== 'undefined') {
    resizeObserver = new ResizeObserver(() => {
      fit();
    });
    resizeObserver.observe(rowEl);
  }

  // Run immediately and once after layout/fonts settle
  fit();
  scheduleFit();

  return {
    update(nextOptions: AdaptiveFitRowOptions = {}) {
      options = nextOptions;
      fit();
    },
    destroy() {
      if (rafId !== null) cancelAnimationFrame(rafId);
      resizeObserver?.disconnect();
      rowRegistry.delete(rowEl);
      rowEl.removeAttribute(ROW_ATTR);
    },
  };
}

/**
 * Svelte action placed on a numeric value element inside an `adaptiveFitRow` container.
 * Do not put Svelte text interpolation inside the element; this action manages `textContent` directly.
 */
export function adaptiveFitText(el: HTMLElement, initialOptions: AdaptiveFitTextOptions) {
  const slot: SlotState = {
    el,
    candidates: initialOptions.candidates.length > 0 ? initialOptions.candidates : [''],
    priority: initialOptions.priority ?? 0,
    baseTitle: initialOptions.title,
    index: 0,
  };

  el.setAttribute(SLOT_ATTR, '');
  slotRegistry.set(el, slot);
  applySlotState(slot, 0);

  const notifyRow = () => {
    const controller = findOwnerRow(el);
    if (controller) {
      controller.fit();
    }
  };

  notifyRow();

  return {
    update(nextOptions: AdaptiveFitTextOptions) {
      slot.candidates = nextOptions.candidates.length > 0 ? nextOptions.candidates : [''];
      slot.priority = nextOptions.priority ?? 0;
      slot.baseTitle = nextOptions.title;
      applySlotState(slot, 0);
      notifyRow();
    },
    destroy() {
      slotRegistry.delete(el);
      el.removeAttribute(SLOT_ATTR);
    },
  };
}
