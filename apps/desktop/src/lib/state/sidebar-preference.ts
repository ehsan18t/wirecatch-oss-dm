// Whether the person collapsed the main window's sidebar to its rail: a display preference of this window, kept in its page storage like the hints.

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
