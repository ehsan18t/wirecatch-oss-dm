// The download list's right-click menu: what can be done with the selection. Everything here is also in the details, the row's own buttons or the command palette (D3), so nothing lives only in a menu.

import AppWindow from '~icons/lucide/app-window';
import Copy from '~icons/lucide/copy';
import FolderOpen from '~icons/lucide/folder-open';
import FileCheck from '~icons/lucide/file-check';
import FolderInput from '~icons/lucide/folder-input';
import FolderPen from '~icons/lucide/folder-pen';
import Link from '~icons/lucide/link';
import ListOrdered from '~icons/lucide/list-ordered';
import Network from '~icons/lucide/network';
import PanelRight from '~icons/lucide/panel-right';
import Pause from '~icons/lucide/pause';
import Play from '~icons/lucide/play';
import RotateCw from '~icons/lucide/rotate-cw';
import SquareArrowOutUpRight from '~icons/lucide/square-arrow-out-up-right';
import Trash from '~icons/lucide/trash-2';
import type { FixAction } from '@wirecatch/protocol';
import type { IconComponent, MenuAction, MenuEntry } from '@wirecatch/ui';
import { FIX_LABEL } from '$lib/format';
import { shortcutParts } from '$lib/keymap';
import type { AppState } from '$lib/state/app.svelte';
import { applicable, canMoveTo, offersNewLink } from '$lib/state/row-actions';

const FIX_ICON: Record<FixAction, IconComponent> = {
  retry: RotateCw,
  refresh_link: Link,
  choose_folder: FolderPen,
  free_disk_space: FolderOpen,
  keep_original: FileCheck,
  proxy_settings: Network,
  change_queue: ListOrdered,
};

/** A failed download's own fix, which the row and the details offer and Enter runs, leads the menu. */
function fixEntry(app: AppState): MenuAction | false {
  const only = app.selection.rows.length === 1 ? app.selection.rows[0] : undefined;
  const fix = only?.status === 'failed' ? only.error?.fix : undefined;
  if (!only || !fix) return false;
  return {
    id: 'fix',
    label: FIX_LABEL[fix],
    icon: FIX_ICON[fix],
    shortcut: shortcutParts('open'),
    onselect: () => void app.actions.fix(only.id, fix),
  };
}

/** The actions on what is selected, as menu entries. */
function selectionEntries(app: AppState): MenuAction[] {
  const rows = app.selection.rows;
  const ids = rows.map((r) => r.id);
  const can = applicable(rows);
  const only = rows.length === 1 ? rows[0] : undefined;
  const space = shortcutParts('pause-resume');
  const fix = fixEntry(app);
  const entries: (MenuAction | false)[] = [
    fix,
    can.open &&
      only !== undefined && {
        id: 'open',
        label: 'Open',
        icon: SquareArrowOutUpRight,
        shortcut: shortcutParts('open'),
        onselect: () => void app.actions.openFile(only.id),
      },
    can.pause && {
      id: 'pause',
      label: 'Pause',
      icon: Pause,
      shortcut: space,
      onselect: () => void app.actions.pause(ids),
    },
    can.resume && {
      id: 'resume',
      label: 'Resume',
      icon: Play,
      shortcut: can.pause ? undefined : space,
      onselect: () => void app.actions.resume(ids),
    },
    can.retry &&
      only?.error?.fix !== 'retry' && {
        id: 'retry',
        label: 'Try again',
        icon: RotateCw,
        onselect: () => void app.actions.retry(ids),
      },
    {
      id: 'details',
      label: 'Show details',
      icon: PanelRight,
      shortcut: shortcutParts('details'),
      onselect: () => app.openDetails(),
    },
  ];
  return entries.filter((e) => e !== false);
}

/** What only one download offers: its folder, its window and its link. */
function oneEntries(app: AppState): MenuAction[] {
  const rows = app.selection.rows;
  const only = rows.length === 1 ? rows[0] : undefined;
  if (!only) return [];
  return [
    {
      id: 'folder',
      label: 'Show in folder',
      icon: FolderOpen,
      onselect: () => void app.actions.openFolder(only.id),
    },
    {
      id: 'window',
      label: 'Open in a separate window',
      icon: AppWindow,
      onselect: () => void app.showProgressWindow(only.id),
    },
    {
      id: 'copy',
      label: 'Copy link',
      icon: Copy,
      onselect: () => void app.actions.copyLink(only.id),
    },
    // A failure whose fix is a new link offers it first, as its fix.
    ...(offersNewLink(only) && only.error?.fix !== 'refresh_link'
      ? [
          {
            id: 'refresh',
            label: FIX_LABEL.refresh_link,
            icon: Link,
            onselect: () => app.openDialog({ kind: 'refresh', id: only.id }),
          },
        ]
      : []),
  ];
}

/** Moving to each queue that some of the selection is not in already. */
function moveEntries(app: AppState): MenuAction[] {
  const rows = app.selection.rows;
  const ids = rows.map((r) => r.id);
  return app.queues.list
    .filter((q) => canMoveTo(rows, q.id))
    .map((q) => ({
      id: `move-${String(q.id)}`,
      label: `Move to ${q.settings.name}`,
      icon: FolderInput,
      onselect: () => void app.actions.moveToQueue(ids, q.id),
    }));
}

export function rowMenu(app: AppState): MenuEntry[] {
  const count = app.selection.rows.length;
  if (count === 0) return [];
  const ids = app.selection.list;
  const moves = moveEntries(app);
  return [
    ...selectionEntries(app),
    ...oneEntries(app),
    ...(moves.length > 0 ? [{ kind: 'separator' as const, id: 'moves' }, ...moves] : []),
    { kind: 'separator', id: 'end' },
    {
      id: 'remove',
      label: count === 1 ? 'Remove' : `Remove ${String(count)} downloads`,
      icon: Trash,
      danger: true,
      shortcut: shortcutParts('remove'),
      onselect: () => app.openDialog({ kind: 'remove', ids }),
    },
  ];
}
