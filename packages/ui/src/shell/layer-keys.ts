/**
 * Whether a key event belongs to the layer `layer` and not to one opened above it. A layer opened from inside another sits inside it in the page, so its keys pass through the outer one on their way up: the inner layer acts first and marks the event handled, and the outer one leaves it alone.
 */
export function ownsKey(event: KeyboardEvent, layer: HTMLElement): boolean {
  if (event.defaultPrevented) return false;
  const target = event.target;
  if (!(target instanceof Element)) return true;
  const nearest = target.closest('dialog, [data-layer]');
  return nearest === null || nearest === layer;
}
