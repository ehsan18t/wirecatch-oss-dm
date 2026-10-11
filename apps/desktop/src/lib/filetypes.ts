// File types as the desktop reads them, for wording. The decision whether a file may open
// without asking is made in Rust (`is_program` in `src-tauri/src/api.rs`); this is the same
// reading, so the interface can name the type it asks about.

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
