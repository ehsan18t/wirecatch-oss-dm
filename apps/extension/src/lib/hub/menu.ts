/** "Download with Wirecatch" from the context menu: what a click asks for, before anything is sent. */
import { fileNameFromUrl } from '../file-types';
import { nonEmpty } from '../text';
import type { MenuClick, MenuTab } from './deps';

export type MenuPlan =
  /** Nothing to download: the click had no link or source. */
  | { kind: 'failed' }
  /** Like browser downloads from private windows, these stay out of Wirecatch's list. */
  | { kind: 'private_window' }
  /** The browser downloads it itself, and the person is told why. */
  | { kind: 'browser'; url: string; message: string }
  /** A web link for Wirecatch, if the extension may read its cookies. */
  | { kind: 'wirecatch'; url: string; name: string };

export function planMenuClick(click: MenuClick, tab: MenuTab | undefined): MenuPlan {
  const url = nonEmpty(click.linkUrl ?? click.srcUrl);
  if (url === null) return { kind: 'failed' };
  if (tab?.incognito === true) return { kind: 'private_window' };
  if (!/^https?:/i.test(url)) {
    const message =
      'Wirecatch can only take web links, so the browser is downloading this item instead.';
    return { kind: 'browser', url, message };
  }
  return { kind: 'wirecatch', url, name: fileNameFromUrl(url) ?? 'this item' };
}
