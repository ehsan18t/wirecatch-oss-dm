// Finding HTML elements by what they are, checked, rather than by what a selector is assumed to match.

/** The first HTML element under `root` that matches `selector`, or null. */
export function findElement(
  root: ParentNode | null | undefined,
  selector: string,
): HTMLElement | null {
  return findElements(root, selector)[0] ?? null;
}

/** Every HTML element under `root` that matches `selector`, in document order. */
export function findElements(root: ParentNode | null | undefined, selector: string): HTMLElement[] {
  if (!root) return [];
  return [...root.querySelectorAll(selector)].filter((e) => e instanceof HTMLElement);
}

/** `start` or the nearest element around it that matches `selector`, when that is an HTML element. */
export function closestElement(start: Element, selector: string): HTMLElement | null {
  const found = start.closest(selector);
  return found instanceof HTMLElement ? found : null;
}
