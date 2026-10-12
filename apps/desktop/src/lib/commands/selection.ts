// Commands for the selected downloads. They exist only while something is selected, and the
// ones about one file only while exactly one download is.

import type { DownloadId, DownloadSummary, Queue } from '@wirecatch/protocol';
import type { AppState } from '../state/app.svelte';
import { commandsFrom, type Command, type CommandSpec } from './table';

/** The selection the commands act on. */
interface Selected {
  app: AppState;
  ids: DownloadId[];
  /** How the titles name the selection: the file's name, or how many there are. */
  n: string;
  /** The one selected download, when exactly one is. */
  only: DownloadSummary | undefined;
}

const GROUP = 'Selected downloads';

const ACTIONS: readonly CommandSpec<Selected>[] = [
  {
    id: 'sel.pause',
    title: (s) => `Pause ${s.n}`,
    keywords: 'stop hold',
    run: (s) => s.app.actions.pause(s.ids),
  },
  {
    id: 'sel.resume',
    title: (s) => `Resume ${s.n}`,
    keywords: 'start continue',
    run: (s) => s.app.actions.resume(s.ids),
  },
  {
    id: 'sel.retry',
    title: (s) => `Try ${s.n} again`,
    keywords: 'retry restart',
    run: (s) => s.app.actions.retry(s.ids),
  },
  {
    id: 'sel.cancel',
    title: (s) => `Cancel ${s.n} and discard the partial file`,
    keywords: 'stop abort',
    run: (s) => s.app.openDialog({ kind: 'cancel', ids: s.ids }),
  },
  {
    id: 'sel.remove',
    title: (s) => `Remove ${s.n} from the list`,
    keywords: 'delete clear',
    key: 'remove',
    run: (s) => s.app.openDialog({ kind: 'remove', ids: s.ids }),
  },
];

const MOVE: CommandSpec<Selected & { queue: Queue }> = {
  id: (s) => `sel.move.${s.queue.id}`,
  title: (s) => `Move ${s.n} to ${s.queue.name}`,
  keywords: 'queue transfer',
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
    title: (s) => `Show the progress window of ${s.n}`,
    keywords: 'window progress speed open',
    run: (s) => s.app.showProgressWindow(s.only.id),
  },
  {
    id: 'sel.refresh',
    title: (s) => `Give ${s.n} a fresh link`,
    keywords: 'refresh link expired url',
    run: (s) => s.app.openDialog({ kind: 'refresh', id: s.only.id }),
  },
  {
    id: 'sel.choosefolder',
    title: (s) => `Save ${s.n} to a different folder`,
    keywords: 'change folder location',
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

const CLEAR: CommandSpec<AppState> = {
  id: 'sel.clear',
  title: 'Clear the selection',
  key: 'escape',
  when: (app) =>
    app.screen === 'downloads' && (app.selection.size > 0 || app.selection.active !== null),
  run: (app) => app.selection.clear(),
};

export function selectionCommands(app: AppState): Command[] {
  const clear = commandsFrom(GROUP, [CLEAR], app);
  const rows = app.selection.rows;
  const first = rows[0];
  if (!first) return clear;
  const only = rows.length === 1 ? first : undefined;
  const s: Selected = {
    app,
    ids: rows.map((r) => r.id),
    n: only ? `"${only.file_name}"` : `${rows.length} downloads`,
    only,
  };
  return [
    ...commandsFrom(GROUP, ACTIONS, s),
    ...app.queues.list.flatMap((queue) => commandsFrom(GROUP, [MOVE], { ...s, queue })),
    ...(only ? commandsFrom(GROUP, ONE, { ...s, only }) : []),
    ...commandsFrom(GROUP, ORDER, s),
    ...clear,
  ];
}
