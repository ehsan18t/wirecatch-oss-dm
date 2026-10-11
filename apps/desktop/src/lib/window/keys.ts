// Enter in a question window's fields means Download now (D23). Browsers only do this by
// themselves for a submit button inside the form; the window's buttons sit outside it, in the
// bar that stays in view, so the form handles Enter itself.

/** Runs `submit` for Enter pressed in a text field, a radio button or a checkbox. */
export function enterSubmits(e: KeyboardEvent, submit: () => void): void {
  if (e.key !== 'Enter' || e.defaultPrevented || e.isComposing) return;
  const target = e.target;
  if (!(target instanceof HTMLInputElement)) return;
  if (['button', 'submit', 'reset', 'file'].includes(target.type)) return;
  e.preventDefault();
  submit();
}
