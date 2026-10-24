import { layoutSize, placeFlyout, viewportSize, type Box, type Placement } from './flyout';

/**
 * Where a fixed flyout sits next to its anchor: placed after it renders, since only then is its size known.
 */
export class Anchored {
  top = $state(0);
  left = $state(0);
  above = $state(false);
  /** The anchor's width, for a listbox that should be at least as wide as its trigger. */
  anchorWidth = $state(0);

  place(anchor: Box, flyout: HTMLElement, placement: Placement): void {
    this.anchorWidth = anchor.right - anchor.left;
    const position = placeFlyout(anchor, layoutSize(flyout), viewportSize(), placement);
    this.top = position.top;
    this.left = position.left;
    this.above = position.above;
  }

  /** Inline position for the flyout: computed at runtime, so it cannot be a utility. */
  get style(): string {
    return `top: ${String(this.top)}px; left: ${String(this.left)}px;`;
  }
}

/**
 * Closes a flyout, as native menus close, on a press outside `inside`, on a resize, or when `anchor` moves under it (a scroll), until the returned function is called.
 */
export function dismissOnOutside(
  inside: () => (HTMLElement | null | undefined)[],
  anchor: HTMLElement | null,
  close: () => void,
): () => void {
  const start = anchor?.getBoundingClientRect();
  const onPointerDown = (event: PointerEvent) => {
    const target = event.target;
    if (!(target instanceof Node)) return;
    if (inside().every((element) => !element?.contains(target))) close();
  };
  // Compared, not closed outright: a scroll queued as the flyout opened moved nothing since.
  const onScroll = () => {
    const now = anchor?.getBoundingClientRect();
    if (now?.top !== start?.top || now?.left !== start?.left) close();
  };
  window.addEventListener('pointerdown', onPointerDown, true);
  window.addEventListener('resize', close);
  window.addEventListener('scroll', onScroll, { capture: true, passive: true });
  return () => {
    window.removeEventListener('pointerdown', onPointerDown, true);
    window.removeEventListener('resize', close);
    window.removeEventListener('scroll', onScroll, { capture: true });
  };
}
