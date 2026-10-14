export interface Box {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface Placement {
  /** Preferred side; flips to the other when it overflows there and the other side fits. */
  side: 'above' | 'below';
  /** `start` lines the flyout up with the anchor's left edge, `end` with its right edge. */
  align: 'start' | 'center' | 'end';
  /** Space between anchor and flyout, px. */
  gap: number;
  /** Closest the flyout comes to the viewport's edges, px. */
  inset: number;
}

export interface Position {
  top: number;
  left: number;
  above: boolean;
}

function preferredLeft(anchor: Box, width: number, align: Placement['align']): number {
  if (align === 'start') return anchor.left;
  if (align === 'end') return anchor.right - width;
  return anchor.left + (anchor.right - anchor.left - width) / 2;
}

/** Where a fixed flyout of `size` goes next to `anchor`, kept inside `viewport`. */
export function placeFlyout(
  anchor: Box,
  size: Size,
  viewport: Size,
  placement: Placement,
): Position {
  const { side, align, gap, inset } = placement;
  const aboveTop = anchor.top - size.height - gap;
  const belowTop = anchor.bottom + gap;
  const fitsAbove = aboveTop >= inset;
  const fitsBelow = belowTop + size.height <= viewport.height - inset;
  const above = side === 'above' ? fitsAbove || !fitsBelow : !fitsBelow && fitsAbove;
  const maxLeft = viewport.width - inset - size.width;
  const left = Math.max(inset, Math.min(preferredLeft(anchor, size.width, align), maxLeft));
  const top = above ? aboveTop : belowTop;
  const maxTop = viewport.height - inset - size.height;
  return { top: Math.max(inset, Math.min(top, maxTop)), left, above };
}

/** A zero-size anchor at a point, for a menu opened where the pointer is. */
export function pointBox(x: number, y: number): Box {
  return { top: y, bottom: y, left: x, right: x };
}

/** The viewport the page sees. */
export function viewportSize(): Size {
  return { width: window.innerWidth, height: window.innerHeight };
}

/** An element's laid-out size, not its bounding box: an entrance animation may be scaling it. */
export function layoutSize(element: HTMLElement): Size {
  return { width: element.offsetWidth, height: element.offsetHeight };
}
