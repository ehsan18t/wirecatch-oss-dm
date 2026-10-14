// File types as the desktop reads them. Whether a file may open without asking is decided in Rust (`is_program` in `src-tauri/src/api.rs`) from `SAFE_EXTENSIONS`, generated from `crates/protocol`, and asked there in a dialog of the shell's own; this is the same list and the same reading, for the sample backend.

import { SAFE_EXTENSIONS } from '@wirecatch/protocol';

const SAFE = new Set(SAFE_EXTENSIONS);

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
