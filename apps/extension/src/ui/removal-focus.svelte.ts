/**
 * Keeps keyboard focus in a list whose item the person just removed with the item's own button.
 * The button goes with its item, and focus would fall to the page: once the list without the item
 * arrives, focus goes to the button now in that place (or the last one), or to the fallback when
 * none is left. Focus the person moved elsewhere in the meantime stays there.
 *
 * Construct it while a component initialises: it watches the list with an effect.
 */
export class RemovalFocus {
  /** The removed item and its place, until the list without it arrives and focus can move. */
  #removed = $state<{ key: string; index: number } | null>(null);

  constructor(options: {
    /** Whether the list still holds the item. */
    has: (key: string) => boolean;
    /** The buttons of the items left, in order. */
    buttons: () => readonly HTMLElement[];
    /** Where focus goes when the list is empty. */
    fallback: () => HTMLElement | null;
  }) {
    $effect(() => {
      if (this.#removed === null) return;
      const { key, index } = this.#removed;
      if (options.has(key)) return;
      this.#removed = null;
      const active = document.activeElement;
      if (active !== null && active !== document.body) return;
      const buttons = options.buttons();
      (buttons[Math.min(index, buttons.length - 1)] ?? options.fallback())?.focus();
    });
  }

  /** Call as the item at `index` is removed. */
  removing(key: string, index: number): void {
    this.#removed = { key, index };
  }
}

/** The buttons inside `root`, for `RemovalFocus.buttons`. */
export function buttonsIn(root: HTMLElement | null): HTMLElement[] {
  return root === null ? [] : [...root.querySelectorAll('button')];
}
