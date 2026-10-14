// Whether the person collapsed the main window's sidebar to its rail, and whether they folded its File types away: display preferences of this window, kept in its page storage like the hints.

import { readStored, writeStored } from './page-storage';

const KEY = 'wirecatch.sidebar.collapsed';
const COLLAPSED = 'true';

/** Whether the sidebar was left collapsed; expanded when nothing was kept. */
export function sidebarCollapsed(): boolean {
  return readStored(KEY) === COLLAPSED;
}

export function keepSidebarCollapsed(collapsed: boolean): void {
  writeStored(KEY, String(collapsed));
}

const FILE_TYPES_KEY = 'wirecatch.sidebar.file-types';
const SHOWN = 'true';

/** Whether File types shows its places: as the person left it, or, when they never chose, whether the window is tall enough for every section open. */
export function fileTypesShown(tallWindow: boolean): boolean {
  const kept = readStored(FILE_TYPES_KEY);
  return kept === null ? tallWindow : kept === SHOWN;
}

export function keepFileTypesShown(shown: boolean): void {
  writeStored(FILE_TYPES_KEY, String(shown));
}
