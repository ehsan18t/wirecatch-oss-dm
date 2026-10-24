/**
 * Keeps a controlled text field in step with its owner, as Checkbox does, without fighting the person typing. Svelte writes `value` into the field only when it changes, so on its own a field would keep text its owner refused or rewrote; written back too eagerly, it would erase keystrokes an owner answers later (a setting saved through the app and read back from its store).
 *
 * So while the field has focus, a value that is only an echo of text already typed there is left alone: the field already shows it, or something newer. Any other value (a rewrite, such as spaces stripped, or a change from outside) is shown at once, with the caret kept. When focus leaves, the field shows the owner's value, so text the owner refused goes then.
 */
export class TextSync {
  #field: HTMLInputElement | HTMLTextAreaElement | null = null;
  /** Texts reported to the owner that it has not answered yet, oldest first. */
  #pending: string[] = [];
  #value = '';

  /** A Svelte attachment: the field to keep in step. It listens for the field's blur itself: a handler in the component's markup would read the component's props, and Chromium sends a focused field a blur as it leaves the page, after the component is gone. */
  readonly attach = (field: HTMLInputElement | HTMLTextAreaElement): (() => void) => {
    this.#field = field;
    field.value = this.#value;
    field.addEventListener('blur', this.settle);
    return () => {
      field.removeEventListener('blur', this.settle);
      this.#field = null;
    };
  };

  /** The person typed `text`; it goes to the owner next. */
  typed(text: string): void {
    this.#pending.push(text);
  }

  /** The owner's value, now or after a change. */
  show(value: string): void {
    this.#value = value;
    const field = this.#field;
    if (!field || field.value === value) {
      this.#answered(value);
      return;
    }
    if (field === document.activeElement && this.#answered(value)) return;
    this.#write(field, value);
  }

  /** Focus left the field: it shows what the owner kept. */
  readonly settle = (): void => {
    this.#pending = [];
    const field = this.#field;
    if (field && field.value !== this.#value) this.#write(field, this.#value);
  };

  /** Drops what the owner has now answered; whether `value` was an echo of typed text. */
  #answered(value: string): boolean {
    const index = this.#pending.lastIndexOf(value);
    if (index === -1) return false;
    this.#pending = this.#pending.slice(index + 1);
    return true;
  }

  #write(field: HTMLInputElement | HTMLTextAreaElement, value: string): void {
    const caret = field.selectionStart;
    const shown = field.value;
    field.value = value;
    // What was typed before is superseded: none of it can be an echo any more.
    this.#pending = [];
    // Types without a caret (email, number) report null and take no selection.
    if (caret !== null && field === document.activeElement) {
      const moved = Math.min(value.length, Math.max(0, caret - (shown.length - value.length)));
      field.setSelectionRange(moved, moved);
    }
  }
}
