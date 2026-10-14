/** A setting as a search sees it: the words it is shown with, and others people may look for it by. */
export interface Searchable {
  label: string;
  description?: string | undefined;
  /** Words that are not on screen but that people may search for, such as "intercept" for "Capture downloads". */
  keywords?: readonly string[] | undefined;
}

/** The words of a search, in lower case. */
export function searchWords(query: string): string[] {
  return query
    .toLocaleLowerCase()
    .split(/\s+/)
    .filter((word) => word !== '');
}

/** Whether a setting answers a search: every word of the search appears in its label, its description or its keywords, in any order. An empty search matches every setting. */
export function matchesSearch(item: Searchable, query: string): boolean {
  const words = searchWords(query);
  if (words.length === 0) return true;
  const text = [item.label, item.description ?? '', ...(item.keywords ?? [])]
    .join(' ')
    .toLocaleLowerCase();
  return words.every((word) => text.includes(word));
}
