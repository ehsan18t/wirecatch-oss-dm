// A `<select>` hands back its value as text, whatever its options were made from. These read that text back into the value of the option it names, so a value the menu did not offer never reaches the core.

/** The option of `offered` whose text is `value`, or `undefined` when none is. */
export function offeredValue<T extends string | number>(
  offered: readonly T[],
  value: string,
): T | undefined {
  return offered.find((option) => String(option) === value);
}

/**
 * The audio quality a menu of `offered` bit rates names: `null` for its empty option, which keeps the usual quality, when `usual` says the menu offers it; `undefined` when `value` is none of these.
 */
export function offeredKbps(
  offered: readonly number[],
  value: string,
  { usual }: { usual: boolean },
): number | null | undefined {
  if (value === '') return usual ? null : undefined;
  return offeredValue(offered, value);
}
