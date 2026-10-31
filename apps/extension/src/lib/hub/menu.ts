/** "Download with Wirecatch" from the context menu: what a click asks for, before anything is sent. */
import { fileNameFromUrl } from '../file-types';
import { nonEmpty } from '../text';
import type { MenuClick, MenuTab } from './deps';
import { MENU_NOT_WEB } from './alerts';
import { isWebUrl } from '../url';

export type MenuPlan =
  /** Nothing to download: the click had no link or source. */
  | { kind: 'failed' }
  /** "Work in private windows" is off (D82). */
  | { kind: 'private_window' }
  /** The browser downloads it itself, and the person is told why. */
  | { kind: 'browser'; url: string; message: string }
  /** A web link for Wirecatch, if the extension may read its cookies. */
  | { kind: 'wirecatch'; url: string; name: string };

export function planMenuClick(
  click: MenuClick,
  tab: MenuTab | undefined,
  privateWindows: boolean,
): MenuPlan {
  const url = nonEmpty(click.linkUrl ?? click.srcUrl);
  if (url === null) return { kind: 'failed' };
  if (tab?.incognito === true && !privateWindows) return { kind: 'private_window' };
  if (!isWebUrl(url)) {
    return { kind: 'browser', url, message: MENU_NOT_WEB };
  }
  return { kind: 'wirecatch', url, name: fileNameFromUrl(url) ?? 'this item' };
}
