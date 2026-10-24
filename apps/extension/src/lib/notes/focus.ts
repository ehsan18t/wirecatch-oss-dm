/**
 * Which tab may show a note (D65): the one the person is looking at, while the browser is the
 * program in front. A note in a browser behind another program, minimized, or in a private window,
 * or on a page that is not a website (where no page script runs), would go unseen or cannot be
 * shown; the app then tells it in a system notification.
 */
import { originPattern } from '../site-access';

/** The part of `windows` and `tabs` this needs. */
export interface FocusApi {
  /** `windows.getLastFocused()`. */
  lastFocusedWindow: () => Promise<{
    id?: number;
    focused: boolean;
    state?: string;
    incognito: boolean;
  }>;
  /** The active tab of the window `windowId`: `tabs.query({ active: true, windowId })`. */
  activeTab: (windowId: number) => Promise<{ id?: number; url?: string } | undefined>;
}

/** The tab the person is looking at, or null when a note there would go unseen or cannot show. */
export async function focusedWebTab(api: FocusApi): Promise<number | null> {
  const window = await api.lastFocusedWindow();
  if (!window.focused || window.state === 'minimized' || window.incognito) return null;
  if (window.id === undefined) return null;
  const tab = await api.activeTab(window.id);
  if (tab?.id === undefined || tab.url === undefined || originPattern(tab.url) === null)
    return null;
  return tab.id;
}
