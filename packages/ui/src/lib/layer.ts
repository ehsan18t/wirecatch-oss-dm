// Flyouts (menus, listboxes, popovers, tooltips) are shown in the browser's top layer through the popover API. There they sit above modal dialogs, are never clipped by an `overflow` ancestor, and are placed against the viewport even inside a panel whose backdrop filter would otherwise become their containing block. Where the API is missing (unit tests), the element stays where it is, positioned `fixed`.

/** Shows `element` (which carries `popover="manual"`) in the top layer. */
export function raise(element: HTMLElement): void {
  if ('showPopover' in element && !element.matches(':popover-open')) element.showPopover();
}

/** Takes `element` out of the top layer again; it is removed from the page right after. */
export function lower(element: HTMLElement): void {
  if ('hidePopover' in element && element.isConnected && element.matches(':popover-open')) {
    element.hidePopover();
  }
}

/** A Svelte attachment that raises the element while it is on the page. */
export function topLayer(element: HTMLElement): () => void {
  raise(element);
  return () => {
    lower(element);
  };
}
