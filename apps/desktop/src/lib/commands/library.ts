// Commands for the download list as a whole: adding, acting on every download, what the list
// shows, its queues, help, and the downloads that wait for an answer.

import type { Category, Queue } from '@wirecatch/protocol';
import { CATEGORIES, CATEGORY_LABEL } from '../format';
import type { AppState } from '../state/app.svelte';
import { STATUS_GROUPS, STATUS_GROUP_LABEL, type StatusGroup } from '../state/view';
import { commandsFrom, type Command, type CommandSpec } from './table';

/** Puts the keyboard in the list's search box, with what is typed there selected. */
export function focusSearch(): void {
  const el = document.getElementById('list-search');
  if (el instanceof HTMLInputElement) {
    el.focus();
    el.select();
  }
}

const DOWNLOADS: readonly CommandSpec<AppState>[] = [
  {
    id: 'dl.add',
    title: 'Add a download',
    keywords: 'new link url paste',
    key: 'add',
    run: (app) => app.openDialog({ kind: 'add', urls: [] }),
  },
  {
    id: 'dl.pauseall',
    title: 'Pause all downloads',
    keywords: 'stop everything',
    run: (app) => app.pauseAll(),
  },
  {
    id: 'dl.resumeall',
    title: 'Resume all downloads',
    keywords: 'start everything continue',
    run: (app) => app.resumeAll(),
  },
  {
    id: 'dl.retryfailed',
    title: 'Retry all failed downloads',
    keywords: 'try again errors needs attention',
    run: (app) => app.retryAllFailed(),
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
    keywords: 'remove',
    when: ({ q }) => !q.is_default,
    run: ({ app, q }) => app.openDialog({ kind: 'queue', id: q.id }),
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
    ...commandsFrom('Show', [SHOW_ALL], app),
    ...STATUS_GROUPS.flatMap((group) => commandsFrom('Show', [SHOW_STATUS], { app, group })),
    ...CATEGORIES.flatMap((category) => commandsFrom('Show', [SHOW_CATEGORY], { app, category })),
    ...commandsFrom('Queues', [NEW_QUEUE], app),
    ...app.queues.list.flatMap((q) => commandsFrom('Queues', QUEUE, { app, q })),
  ];
}

export function helpCommands(app: AppState): Command[] {
  return [...commandsFrom('Help', HELP, app), ...commandsFrom('Downloads', QUESTIONS, app)];
}
