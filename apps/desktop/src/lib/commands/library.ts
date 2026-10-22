// Commands for the download list as a whole: adding, acting on every download, what the list
// shows, its queues, help, and the downloads that wait for an answer.

import type { Category, Queue } from '@wirecatch/protocol';
import { CATEGORIES, CATEGORY_LABEL } from '../format';
import { SEARCH_ID } from '../dom';
import type { AppState } from '../state/app.svelte';
import {
  GROUP_ORDER,
  QUEUE_ORDER,
  sameSort,
  STATUS_GROUPS,
  STATUS_GROUP_LABEL,
  type Sort,
  type SortColumn,
  type StatusGroup,
} from '../state/view';
import { keysOf } from '../keys';
import { commandsFrom, type Command, type CommandSpec } from './table';

/** Puts the keyboard in the title bar's search (the list's, or the settings' while they show), with what is typed there selected. */
export function focusSearch(): void {
  const el = document.getElementById(SEARCH_ID);
  if (el instanceof HTMLInputElement) {
    el.focus();
    el.select();
  }
}

const DOWNLOADS: readonly CommandSpec<AppState>[] = [
  {
    id: 'dl.add',
    title: 'New download',
    keywords: 'add new link url paste',
    key: 'add',
    run: (app) => app.openDialog({ kind: 'add', urls: [] }),
  },
  {
    id: 'dl.pauseall',
    title: 'Pause all downloads',
    keywords: 'stop everything',
    run: (app) => app.actions.pauseAll(),
  },
  {
    id: 'dl.resumeall',
    title: 'Resume all downloads',
    keywords: 'start everything continue',
    run: (app) => app.actions.resumeAll(),
  },
  {
    id: 'dl.retryfailed',
    title: 'Retry all failed downloads',
    keywords: 'try again errors needs attention',
    run: (app) => app.actions.retryAllFailed(),
  },
  {
    id: 'dl.selectall',
    title: 'Select every download in the list',
    key: 'select-all',
    run: (app) => app.selection.all(),
  },
  {
    id: 'dl.search',
    title: 'Search the list',
    key: 'search',
    keywords: 'find filter',
    run: () => focusSearch(),
  },
];

const BACK: CommandSpec<AppState> = {
  id: 'show.back',
  title: 'Back to downloads',
  key: 'escape',
  when: (app) => app.screen === 'settings',
  run: (app) => (app.screen = 'downloads'),
};

const SHOW_ALL: CommandSpec<AppState> = {
  id: 'show.all',
  title: 'Show all downloads',
  run: (app) => app.show({ kind: 'all' }),
};

const SHOW_STATUS: CommandSpec<{ app: AppState; group: StatusGroup }> = {
  id: ({ group }) => `show.status.${group}`,
  title: ({ group }) => `Show ${STATUS_GROUP_LABEL[group].toLowerCase()}`,
  keywords: 'filter status',
  run: ({ app, group }) => app.show({ kind: 'status', group }),
};

const SHOW_CATEGORY: CommandSpec<{ app: AppState; category: Category }> = {
  id: ({ category }) => `show.category.${category}`,
  title: ({ category }) => `Show ${CATEGORY_LABEL[category].toLowerCase()}`,
  keywords: 'filter category type',
  run: ({ app, category }) => app.show({ kind: 'category', category }),
};

/** A group of the list on screen, folded away or opened: what the click on its heading does. */
const FOLD_GROUP: CommandSpec<{ app: AppState; group: StatusGroup }> = {
  id: ({ group }) => `list.fold.${group}`,
  title: ({ app, group }) =>
    `${app.folded.has(group) ? 'Open' : 'Fold'} the ${STATUS_GROUP_LABEL[group]} group`,
  keywords: 'fold unfold collapse expand hide show group heading',
  when: ({ app, group }) =>
    app.screen === 'downloads' && app.layout.sections.some((s) => s.group === group),
  run: ({ app, group }) => app.toggleGroup(group),
};

/** How each sort reads, ascending and descending, in words. */
const SORT_WORDS: Record<Exclude<SortColumn, 'position'>, [string, string]> = {
  name: ['by name, A to Z', 'by name, Z to A'],
  progress: ['by progress, least done first', 'by progress, most done first'],
  size: ['by size, smallest first', 'by size, largest first'],
  speed: ['by speed, slowest first', 'by speed, fastest first'],
  left: ['by time left, shortest first', 'by time left, longest first'],
};

/** A sort of the list inside its groups: what a click on a column heading does. */
const SORT_LIST: CommandSpec<{ app: AppState; sort: Sort }> = {
  id: ({ sort }) => `list.sort.${sort.column}${sort.descending ? '.descending' : ''}`,
  title: ({ sort }) =>
    sort.column === 'position'
      ? 'Sort the list in queue order'
      : `Sort the list ${SORT_WORDS[sort.column][sort.descending ? 1 : 0]}`,
  keywords: 'sort order column ascending descending',
  when: ({ app, sort }) => app.screen === 'downloads' && !sameSort(app.sort, sort),
  run: ({ app, sort }) => (app.sort = sort),
};

const SORTS: readonly Sort[] = [
  QUEUE_ORDER,
  ...keysOf(SORT_WORDS).flatMap((column) => [
    { column, descending: false },
    { column, descending: true },
  ]),
];

const NEW_QUEUE: CommandSpec<AppState> = {
  id: 'queue.new',
  title: 'Create a queue',
  keywords: 'new add',
  run: (app) => app.openDialog({ kind: 'queue', id: null }),
};

/** One queue's commands; `when` picks Pause or Resume, and what only some queues offer. */
const QUEUE: readonly CommandSpec<{ app: AppState; q: Queue }>[] = [
  {
    id: ({ q }) => `queue.show.${q.id}`,
    title: ({ q }) => `Show the ${q.name} queue`,
    keywords: 'open view',
    run: ({ app, q }) => app.show({ kind: 'queue', id: q.id }),
  },
  {
    id: ({ q }) => `queue.edit.${q.id}`,
    title: ({ q }) => `Edit ${q.name}: name, schedule and limits`,
    keywords: 'rename schedule speed limit concurrent settings',
    run: ({ app, q }) => app.openDialog({ kind: 'queue', id: q.id }),
  },
  {
    id: ({ q }) => `queue.resume.${q.id}`,
    title: ({ q }) => `Resume the ${q.name} queue`,
    keywords: 'start run continue unpause',
    when: ({ q }) => q.paused,
    run: ({ app, q }) => app.queues.setPaused(q.id, false),
  },
  {
    id: ({ q }) => `queue.pause.${q.id}`,
    title: ({ q }) => `Pause the ${q.name} queue`,
    keywords: 'stop hold',
    when: ({ q }) => !q.paused,
    run: ({ app, q }) => app.queues.setPaused(q.id, true),
  },
  {
    id: ({ q }) => `queue.startnow.${q.id}`,
    title: ({ q }) => `Start ${q.name} now, outside its hours`,
    keywords: 'run schedule override immediately',
    when: ({ app, q }) => app.queues.isOutsideSchedule(q),
    run: ({ app, q }) => app.queues.startNow(q.id),
  },
  {
    id: ({ q }) => `queue.delete.${q.id}`,
    title: ({ q }) => `Delete the ${q.name} queue`,
    danger: true,
    keywords: 'remove',
    when: ({ q }) => !q.is_default,
    run: ({ app, q }) => app.openDialog({ kind: 'queue', id: q.id, deleting: true }),
  },
];

const HELP: readonly CommandSpec<AppState>[] = [
  {
    id: 'help.shortcuts',
    title: 'Show keyboard shortcuts',
    key: 'help',
    keywords: 'keys hotkeys help',
    run: (app) => app.openDialog({ kind: 'shortcuts' }),
  },
];

const QUESTIONS: readonly CommandSpec<AppState>[] = [
  {
    id: 'questions.show',
    title: (app) =>
      app.questions.waitingCount === 1
        ? 'Show the download that is waiting for you'
        : `Show the ${app.questions.waitingCount} downloads that are waiting for you`,
    keywords: 'waiting answer ask confirm pending capture video quality window add-on',
    when: (app) => app.questions.waitingCount > 0,
    run: (app) => app.showQuestionWindows(),
  },
];

export function downloadCommands(app: AppState): Command[] {
  return commandsFrom('Downloads', DOWNLOADS, app);
}

export function viewCommands(app: AppState): Command[] {
  return [
    ...commandsFrom('Show', [BACK, SHOW_ALL], app),
    ...STATUS_GROUPS.flatMap((group) => commandsFrom('Show', [SHOW_STATUS], { app, group })),
    ...CATEGORIES.flatMap((category) => commandsFrom('Show', [SHOW_CATEGORY], { app, category })),
    ...GROUP_ORDER.flatMap((group) => commandsFrom('Show', [FOLD_GROUP], { app, group })),
    ...SORTS.flatMap((sort) => commandsFrom('Show', [SORT_LIST], { app, sort })),
    ...commandsFrom('Queues', [NEW_QUEUE], app),
    ...app.queues.list.flatMap((q) => commandsFrom('Queues', QUEUE, { app, q })),
  ];
}

export function helpCommands(app: AppState): Command[] {
  return [...commandsFrom('Help', HELP, app), ...commandsFrom('Downloads', QUESTIONS, app)];
}
