// File types as the desktop reads them. The decision whether a file may open without asking is
// made in Rust (`is_program` in `src-tauri/src/api.rs`); this is the same list and the same
// reading, for wording and for the sample backend. `filetypes.test.ts` checks the list against
// the Rust one, so they cannot drift apart.

/** Copy of `SAFE_EXTENSIONS`: the only types that open without asking. */
export const SAFE_FILE_TYPES: readonly string[] = [
  ...['mp4', 'm4v', 'mkv', 'webm', 'mov', 'avi', 'wmv', 'flv', 'mpg', 'mpeg', 'ts', 'm2ts'],
  ...['3gp', 'ogv', 'mp3', 'm4a', 'aac', 'flac', 'wav', 'ogg', 'oga', 'opus', 'wma', 'aiff'],
  ...['mka', 'srt', 'vtt', 'jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'tif', 'tiff', 'heic'],
  ...['heif', 'avif', 'pdf', 'txt', 'md', 'docx', 'xlsx', 'pptx', 'epub', 'zip', '7z', 'rar'],
  ...['tar', 'gz', 'tgz', 'bz2', 'xz', 'zst'],
];

const SAFE = new Set(SAFE_FILE_TYPES);

/**
 * The type of `name` the way Windows reads it: trailing dots and spaces dropped, lower case,
 * the last extension only (so `report.pdf.exe` is `exe`). `null` when the name has none.
 */
export function fileType(name: string): string | null {
  const trimmed = name.replace(/[. ]+$/, '');
  const dot = trimmed.lastIndexOf('.');
  if (dot < 0 || dot === trimmed.length - 1) return null;
  return trimmed.slice(dot + 1).toLowerCase();
}

/** Whether opening a file of this name might run code, so the person is asked first. */
export function isProgramName(name: string): boolean {
  const type = fileType(name);
  return type === null || !SAFE.has(type);
}
