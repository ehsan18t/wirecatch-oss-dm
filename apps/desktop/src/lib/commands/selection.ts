// Commands for the selected downloads. They exist only while something is selected, and the
// ones about one file only while exactly one download is.

import type { DownloadId, DownloadSummary, Queue } from '@wirecatch/protocol';
import type { AppState } from '../state/app.svelte';
import {
  applicable,
  canChangeFolder,
  canMoveTo,
  offersNewLink,
  type Applicable,
} from '../state/row-actions';
import { commandsFrom, type Command, type CommandSpec } from './table';

/** The selection the commands act on. */
interface Selected {
  app: AppState;
  ids: DownloadId[];
  /** How the titles name the selection: the file's name, or how many there are. */
  n: string;
  /** The one selected download, when exactly one is. */
  only: DownloadSummary | undefined;
  /** What the selection offers: an action shows when at least one of its downloads takes it. */
  can: Applicable;
  rows: readonly DownloadSummary[];
}

const GROUP = 'Selected downloads';

const ACTIONS: readonly CommandSpec<Selected>[] = [
  {
    id: 'sel.pause',
    title: (s) => `Pause ${s.n}`,
    keywords: 'stop hold',
    when: (s) => s.can.pause,
    run: (s) => s.app.actions.pause(s.ids),
  },
  {
    id: 'sel.resume',
    title: (s) => `Resume ${s.n}`,
    keywords: 'start continue',
    when: (s) => s.can.resume,
    run: (s) => s.app.actions.resume(s.ids),
  },
  {
    id: 'sel.retry',
    title: (s) => `Try ${s.n} again`,
    keywords: 'retry restart',
    when: (s) => s.can.retry,
    run: (s) => s.app.actions.retry(s.ids),
  },
  {
    id: 'sel.cancel',
    title: (s) => `Cancel ${s.n} and discard the partial file`,
    danger: true,
    keywords: 'stop abort',
    when: (s) => s.can.cancel,
    run: (s) => s.app.openDialog({ kind: 'cancel', ids: s.ids }),
  },
  {
    id: 'sel.remove',
    title: (s) => `Remove ${s.n} from the list`,
    danger: true,
    keywords: 'delete clear',
    key: 'remove',
    run: (s) => s.app.openDialog({ kind: 'remove', ids: s.ids }),
  },
];

const MOVE: CommandSpec<Selected & { queue: Queue }> = {
  id: (s) => `sel.move.${s.queue.id}`,
  title: (s) => `Move ${s.n} to ${s.queue.name}`,
  keywords: 'queue transfer',
  when: (s) => canMoveTo(s.rows, s.queue.id),
  run: (s) => s.app.actions.moveToQueue(s.ids, s.queue.id),
};

/** Commands about one file. `only` is checked by `when`, so `run` may rely on it. */
const ONE: readonly CommandSpec<Selected & { only: DownloadSummary }>[] = [
  {
    id: 'sel.open',
    title: (s) => `Open ${s.n}`,
    keywords: 'launch run file',
    key: 'open',
    when: (s) => s.only.status === 'completed',
    run: (s) => s.app.actions.openFile(s.only.id),
  },
  {
    id: 'sel.folder',
    title: (s) => `Show ${s.n} in its folder`,
    keywords: 'open folder explorer',
    run: (s) => s.app.actions.openFolder(s.only.id),
  },
  {
    id: 'sel.progress',
    title: (s) => `Open ${s.n} in its own window`,
    keywords: 'window progress speed open',
    run: (s) => s.app.showProgressWindow(s.only.id),
  },
  {
    id: 'sel.refresh',
    title: (s) => `Get a new link for ${s.n}`,
    keywords: 'refresh fresh link expired url',
    when: (s) => offersNewLink(s.only),
    run: (s) => s.app.openDialog({ kind: 'refresh', id: s.only.id }),
  },
  {
    id: 'sel.choosefolder',
    title: (s) => `Save ${s.n} to a different folder`,
    keywords: 'change folder location',
    when: (s) => canChangeFolder(s.only),
    run: (s) => s.app.actions.chooseFolderFor(s.only.id),
  },
];

const ORDER: readonly CommandSpec<Selected>[] = [
  {
    id: 'sel.up',
    title: 'Move the selection up in its queue',
    key: 'nudge-up',
    keywords: 'reorder priority',
    run: (s) => s.app.actions.nudge(-1),
  },
  {
    id: 'sel.down',
    title: 'Move the selection down in its queue',
    key: 'nudge-down',
    keywords: 'reorder priority',
    run: (s) => s.app.actions.nudge(1),
  },
];

const DETAILS: CommandSpec<Selected> = {
  id: 'sel.details',
  title: (s) => `Show the details of ${s.n}`,
  keywords: 'info properties panel',
  key: 'details',
  run: (s) => s.app.openDetails(),
};

/** Escape closes the details before it clears the selection they show. */
const CLOSE_DETAILS: CommandSpec<AppState> = {
  id: 'sel.closedetails',
  title: 'Close the details',
  key: 'escape',
  when: (app) => app.screen === 'downloads' && app.detailsOpen,
  run: (app) => app.closeDetails(),
};

const CLEAR: CommandSpec<AppState> = {
  id: 'sel.clear',
  title: 'Clear the selection',
  key: 'escape',
  when: (app) =>
    app.screen === 'downloads' && (app.selection.size > 0 || app.selection.active !== null),
  run: (app) => app.selection.clear(),
};

export function selectionCommands(app: AppState): Command[] {
  const clear = commandsFrom(GROUP, [CLOSE_DETAILS, CLEAR], app);
  const rows = app.selection.rows;
  const first = rows[0];
  if (!first) return clear;
  const only = rows.length === 1 ? first : undefined;
  const s: Selected = {
    app,
    ids: rows.map((r) => r.id),
    n: only ? `"${only.file_name}"` : `${rows.length} downloads`,
    only,
    can: applicable(rows),
    rows,
  };
  return [
    ...commandsFrom(GROUP, ACTIONS, s),
    ...app.queues.list.flatMap((queue) => commandsFrom(GROUP, [MOVE], { ...s, queue })),
    ...commandsFrom(GROUP, [DETAILS], s),
    ...(only ? commandsFrom(GROUP, ONE, { ...s, only }) : []),
    ...commandsFrom(GROUP, ORDER, s),
    ...clear,
  ];
}
