/** Sets `key` in `map`, then drops the oldest entry while it holds more than `limit`. */
export function remember<K, V>(map: Map<K, V>, key: K, value: V, limit: number): void {
  map.set(key, value);
  while (map.size > limit) {
    const oldest = map.keys().next();
    if (oldest.done === true) return;
    map.delete(oldest.value);
  }
}
