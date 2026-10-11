/**
 * Reads its arguments so the surrounding effect or derived runs again when they change.
 *
 * Svelte subscribes to state when it is read. Some code depends on a value without using it
 * (scroll a new view to the top, move the focus once a button goes away); passing the value here
 * says that the read is the point, where a bare expression statement would look like a mistake.
 */
export function track(..._values: unknown[]): void {
  // Nothing to do: the caller read the values to pass them, which is what subscribes.
}
