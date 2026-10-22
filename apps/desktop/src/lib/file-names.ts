// What a file's name says: its kind, and an extension as a person types it.

/** What kind of file a name is, by its extension (`ISO file`), or by its category where it has none. */
export function fileKind(name: string, category: string): string {
  const dot = name.lastIndexOf('.');
  const ext = dot > 0 ? name.slice(dot + 1) : '';
  return /^[a-z0-9]{1,8}$/i.test(ext) ? `${ext.toUpperCase()} file` : category;
}

/** How many of a path's last folders a summary shows. */
const FOLDERS_SHOWN = 2;

/** The last two folders of a path, as a one-line summary shows where a file goes: `Downloads\Video`. */
export function folderTail(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  const sep = path.includes('\\') ? '\\' : '/';
  return parts.slice(-FOLDERS_SHOWN).join(sep) || path;
}

/** A file extension as typed, with or without its dot, the way settings keep it; null when nothing is left. */
export function typedExtension(typed: string): string | null {
  return typed.trim().toLowerCase().replace(/^\./, '') || null;
}
