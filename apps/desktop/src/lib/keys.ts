/** The keys of `record`, as the type of its keys: `Object.keys` types them as any string, since an object may hold more than its type names, so a record whose keys are a fixed set is read with this instead of an assertion. Only own keys are given, as `Object.keys` gives them. */
export function keysOf<K extends string>(record: Readonly<Record<K, unknown>>): K[] {
  const keys: K[] = [];
  for (const key in record) if (Object.hasOwn(record, key)) keys.push(key);
  return keys;
}
