/** Helpers for the "file types to capture" setting (lower case, no dot, empty list means all). */
import { normalizeFileType } from '@wirecatch/protocol';

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

/**
 * File types that run code when opened: Windows programs, installers and scripts,
 * Java archives, and Linux, macOS and Android packages. These stay in the browser
 * by default, so its own protection (Safe Browsing, SmartScreen, download
 * warnings) checks them before they reach the disk.
 */
export const PROGRAM_TYPES: ReadonlySet<string> = new Set([
  // Windows programs and installers
  'exe',
  'msi',
  'msix',
  'msixbundle',
  'appx',
  'appxbundle',
  'msp',
  'com',
  'scr',
  'pif',
  'cpl',
  'msc',
  'application',
  'appref-ms',
  'gadget',
  'inf',
  'lnk',
  'reg',
  // Windows scripts
  'bat',
  'cmd',
  'ps1',
  'psm1',
  'vbs',
  'vbe',
  'js',
  'jse',
  'wsf',
  'wsh',
  'hta',
  // Java
  'jar',
  'jnlp',
  // Linux
  'appimage',
  'deb',
  'rpm',
  'sh',
  'bash',
  'run',
  'snap',
  'flatpak',
  'flatpakref',
  // macOS
  'dmg',
  'pkg',
  'mpkg',
  'command',
  // Android
  'apk',
  'apks',
  'xapk',
]);

/** Media types servers use for programs, for files whose name says nothing. */
const PROGRAM_MIMES: ReadonlySet<string> = new Set([
  'application/x-msdownload',
  'application/x-msdos-program',
  'application/x-ms-installer',
  'application/x-msi',
  'application/vnd.microsoft.portable-executable',
  'application/x-dosexec',
  'application/x-executable',
  'application/x-sh',
  'application/x-shellscript',
  'application/java-archive',
  'application/x-java-archive',
  'application/vnd.android.package-archive',
  'application/x-apple-diskimage',
  'application/x-debian-package',
  'application/vnd.debian.binary-package',
  'application/x-rpm',
  'application/x-redhat-package-manager',
  'application/x-hta',
]);

/**
 * The type a file opens as: its last extension in lower case, after dropping the
 * trailing dots and spaces Windows ignores (`setup.EXE. ` opens as `exe`). Null
 * when there is none.
 */
export function lastExtension(fileName: string): string | null {
  const name = baseName(fileName).replace(/[. ]+$/, '');
  const dot = name.lastIndexOf('.');
  if (dot < 0 || dot === name.length - 1) return null;
  return name.slice(dot + 1).toLowerCase();
}

/** True when the file is a program, an installer or a script (see `PROGRAM_TYPES`), by name or by media type. */
export function isProgramFile(fileName: string | null, mime: string | null): boolean {
  const ext = fileName === null ? null : lastExtension(fileName);
  if (ext !== null && PROGRAM_TYPES.has(ext)) return true;
  const type = mime?.split(';', 1)[0]?.trim().toLowerCase() ?? '';
  return PROGRAM_MIMES.has(type);
}
