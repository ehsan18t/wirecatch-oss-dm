import { findElement, findElements } from './dom';

const FOCUSABLE = [
  'a[href]',
  'button:not([disabled])',
  'input:not([disabled]):not([type="hidden"])',
  'select:not([disabled])',
  'textarea:not([disabled])',
  'summary',
  '[tabindex]:not([tabindex="-1"])',
  '[contenteditable="true"]',
].join(',');

/** What Tab can reach inside `root`, in order. Controls disabled with a reason (aria-disabled) stay in: they are focusable so they can say why. A control taken out of the tab order (`tabindex="-1"`, as a roving group's other members are) is not. */
export function focusables(root: HTMLElement): HTMLElement[] {
  return findElements(root, FOCUSABLE).filter(
    (el) =>
      !el.closest('[inert]') && !el.closest('[hidden]') && el.getAttribute('tabindex') !== '-1',
  );
}

/** Where focus starts in a layer: the first control marked `data-autofocus`, else the first that is not aria-disabled, else the first at all, else the layer itself. */
export function initialFocus(root: HTMLElement): HTMLElement {
  const marked = findElement(root, '[data-autofocus]');
  if (marked) return marked;
  const all = focusables(root);
  return all.find((el) => el.getAttribute('aria-disabled') !== 'true') ?? all[0] ?? root;
}

/** Keeps Tab and Shift+Tab inside `root`, wrapping at either end. Returns whether it moved focus. */
export function containTab(event: KeyboardEvent, root: HTMLElement): boolean {
  if (event.key !== 'Tab') return false;
  const all = focusables(root);
  const first = all[0];
  const last = all.at(-1);
  const active = document.activeElement;
  if (!first || !last) {
    event.preventDefault();
    root.focus();
    return true;
  }
  const inside = active instanceof Node && root.contains(active);
  const edge = event.shiftKey ? first : last;
  if (inside && active !== edge && active !== root) return false;
  event.preventDefault();
  (event.shiftKey ? last : first).focus();
  return true;
}

/**
 * Makes everything on the page outside `keep` inert, as the browser does around a modal `<dialog>`, for a modal layer that cannot be one (the sidebar drawer, whose items also render docked). Returns what undoes it; elements that were inert already stay so.
 */
export function inertOutside(keep: readonly Element[]): () => void {
  const changed: Element[] = [];
  const holdsKept = (element: Element) => keep.some((kept) => element.contains(kept));
  for (const start of keep) {
    let node: Element = start;
    while (node !== document.body && node.parentElement) {
      for (const sibling of node.parentElement.children) {
        if (holdsKept(sibling) || sibling.hasAttribute('inert')) continue;
        sibling.setAttribute('inert', '');
        changed.push(sibling);
      }
      node = node.parentElement;
    }
  }
  return () => {
    for (const element of changed) element.removeAttribute('inert');
  };
}

/** Whether a modal layer (a modal dialog, the drawer) is open, so the page behind it takes no keys. */
export const modalLayerOpen = (): boolean => document.querySelector('[aria-modal="true"]') !== null;

/** The element that has focus now, if it is an element that can take it back later. */
export function focusedElement(): HTMLElement | null {
  const active = document.activeElement;
  return active instanceof HTMLElement && active !== document.body ? active : null;
}

/** Gives focus back to `target` when focus was lost (on the body, or inside `leaving`) and `target` is still on the page. */
export function restoreFocus(target: HTMLElement | null, leaving: HTMLElement | null): void {
  const active = document.activeElement;
  const lost =
    active === null ||
    active === document.body ||
    (active instanceof Node && leaving?.contains(active) === true);
  if (lost && target?.isConnected) target.focus({ preventScroll: true });
}
