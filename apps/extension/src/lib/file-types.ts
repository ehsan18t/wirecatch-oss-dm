/** File names, and the "file types to capture" setting (lower case, no dot, empty list means all). */
import { normalizeFileType } from '@wirecatch/protocol';
import { parseUrl } from './url';

/** The last path segment of a URL, decoded, or null if there is none. */
export function fileNameFromUrl(url: string): string | null {
  const last = parseUrl(url)?.pathname.split('/').pop() ?? '';
  if (last === '') return null;
  try {
    return decodeURIComponent(last);
  } catch {
    // A `%` that starts no escape: the name is shown as the address spells it.
    return last;
  }
}

/** The file name at the end of a local path (works with both `/` and `\` separators). */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts.at(-1) ?? path;
}

/** True when `fileName` ends with one of `types` (each without the dot); an empty list matches everything. */
export function matchesFileTypes(fileName: string | null, types: readonly string[]): boolean {
  const wanted = types.map(normalizeFileType).filter((t): t is string => t !== null);
  if (wanted.length === 0) return true;
  if (fileName === null) return false;
  const name = fileName.toLowerCase();
  return wanted.some((type) => name.endsWith(`.${type}`));
}
