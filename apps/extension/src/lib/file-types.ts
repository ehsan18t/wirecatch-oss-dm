/** Helpers for the "file types to capture" setting (lower case, no dot, empty list means all). */

/**
 * Splits what a person typed into a clean list of file types.
 * `".ZIP, exe  mp4,zip"` becomes `["zip", "exe", "mp4"]`.
 */
export function parseFileTypes(input: string): string[] {
  const seen = new Set<string>();
  for (const raw of input.split(/[\s,;]+/)) {
    const type = normalizeFileType(raw);
    if (type !== null) seen.add(type);
  }
  return [...seen];
}

/** `".Tar.GZ"` becomes `"tar.gz"`; returns null when nothing is left. */
export function normalizeFileType(raw: string): string | null {
  const type = raw
    .trim()
    .toLowerCase()
    .replace(/^\*?\.+/, '');
  return type === '' ? null : type;
}

/** Shows a list of file types the way a person would type it. */
export function formatFileTypes(types: readonly string[]): string {
  return types.join(', ');
}

/** The last path segment of a URL, decoded, or null if there is none. */
export function fileNameFromUrl(url: string): string | null {
  try {
    const { pathname } = new URL(url);
    const last = pathname.split('/').pop() ?? '';
    if (last === '') return null;
    try {
      return decodeURIComponent(last);
    } catch {
      return last;
    }
  } catch {
    return null;
  }
}

/** The file name at the end of a local path (works with both `/` and `\` separators). */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] ?? path;
}

/** True when `fileName` ends with one of `types` (each without the dot); an empty list matches everything. */
export function matchesFileTypes(fileName: string | null, types: readonly string[]): boolean {
  const wanted = types.map(normalizeFileType).filter((t): t is string => t !== null);
  if (wanted.length === 0) return true;
  if (fileName === null) return false;
  const name = fileName.toLowerCase();
  return wanted.some((type) => name.endsWith(`.${type}`));
}
