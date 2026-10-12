// Every action in the app, in one list. The command palette runs these; buttons and keyboard
// shortcuts call the same `AppState` methods, so there is one behavior per action.

import type { AppState } from './state/app.svelte';
import { addonAction, isInstalled } from './addons';
import { CATEGORIES, CATEGORY_LABEL } from './format';
import { SETTINGS, SECTIONS } from './settings-schema';
import { STATUS_GROUPS, STATUS_GROUP_LABEL } from './state/view';

// How the palette ranks a match. Each typed word scores against its best word in the text.
/** The typed word is the whole word. */
const SCORE_WHOLE_WORD = 12;
/**
 * The typed word starts the word. Up to one more for how much of the word is typed, so "pau"
 * ranks "Pause" above "Paused" but never above a whole word.
 */
const SCORE_PREFIX = 8;
/** The typed word is a short form of the word ("pse" for "Pause"). */
const SCORE_SHORT_FORM = 3;
/** Added when the words typed appear together and in order in the text. */
const SCORE_PHRASE = 5;
/** A match in a command's keywords counts for this share of one in its title. */
const KEYWORD_WEIGHT = 0.5;

export type CommandGroup =
  'Selected downloads' | 'Downloads' | 'Show' | 'Queues' | 'Settings' | 'Help' | 'Sample data';

export interface Command {
  id: string;
  title: string;
  group: CommandGroup;
  keywords?: string;
  shortcut?: string;
  run: () => unknown;
}

export const SHORTCUTS: readonly { keys: string; what: string }[] = [
  { keys: 'Ctrl+K', what: 'Find any action or setting' },
  { keys: 'Ctrl+V', what: 'Download the link you copied' },
  { keys: 'Ctrl+N', what: 'Add a download' },
  { keys: 'Ctrl+F', what: 'Search the list' },
  { keys: 'Ctrl+,', what: 'Open settings' },
  { keys: 'Up, Down', what: 'Move through the list' },
  { keys: 'Shift+Up, Shift+Down', what: 'Select several downloads' },
  { keys: 'Ctrl+A', what: 'Select every download in the list' },
  { keys: 'Space', what: 'Pause or resume the selected downloads' },
  { keys: 'Enter', what: 'Open the selected file when it is finished' },
  { keys: 'Delete', what: 'Remove the selected downloads' },
  { keys: 'Alt+Up, Alt+Down', what: 'Move the selection up or down in its queue' },
  { keys: 'Escape', what: 'Close a dialog or clear the selection' },
  { keys: 'F1', what: 'Show these shortcuts' },
];

export function buildCommands(app: AppState): Command[] {
  const out: Command[] = [];
  const selected = app.selectedRows;
  const ids = selected.map((r) => r.id);

  if (selected.length > 0) {
    const n =
      selected.length === 1 ? `"${selected[0]?.file_name}"` : `${selected.length} downloads`;
    const g: CommandGroup = 'Selected downloads';
    out.push(
      {
        id: 'sel.pause',
        group: g,
        title: `Pause ${n}`,
        keywords: 'stop hold',
        run: () => app.pause(ids),
      },
      {
        id: 'sel.resume',
        group: g,
        title: `Resume ${n}`,
        keywords: 'start continue',
        run: () => app.resume(ids),
      },
      {
        id: 'sel.retry',
        group: g,
        title: `Try ${n} again`,
        keywords: 'retry restart',
        run: () => app.retry(ids),
      },
      {
        id: 'sel.cancel',
        group: g,
        title: `Cancel ${n} and discard the partial file`,
        keywords: 'stop abort',
        run: () => app.openDialog({ kind: 'cancel', ids }),
      },
      {
        id: 'sel.remove',
        group: g,
        title: `Remove ${n} from the list`,
        keywords: 'delete clear',
        shortcut: 'Delete',
        run: () => app.openDialog({ kind: 'remove', ids }),
      },
    );
    for (const q of app.queues) {
      out.push({
        id: `sel.move.${q.id}`,
        group: g,
        title: `Move ${n} to ${q.name}`,
        keywords: 'queue transfer',
        run: () => app.moveToQueue(ids, q.id),
      });
    }
    const first = selected[0];
    if (selected.length === 1 && first) {
      if (first.status === 'completed') {
        out.push({
          id: 'sel.open',
          group: g,
          title: `Open ${n}`,
          keywords: 'launch run file',
          shortcut: 'Enter',
          run: () => app.openFile(first.id),
        });
      }
      out.push(
        {
          id: 'sel.folder',
          group: g,
          title: `Show ${n} in its folder`,
          keywords: 'open folder explorer',
          run: () => app.openFolder(first.id),
        },
        {
          id: 'sel.progress',
          group: g,
          title: `Show the progress window of ${n}`,
          keywords: 'window progress speed open',
          run: () => app.showProgressWindow(first.id),
        },
        {
          id: 'sel.refresh',
          group: g,
          title: `Give ${n} a fresh link`,
          keywords: 'refresh link expired url',
          run: () => app.openDialog({ kind: 'refresh', id: first.id }),
        },
        {
          id: 'sel.choosefolder',
          group: g,
          title: `Save ${n} to a different folder`,
          keywords: 'change folder location',
          run: () => app.chooseFolderFor(first.id),
        },
      );
    }
    out.push(
      {
        id: 'sel.up',
        group: g,
        title: 'Move the selection up in its queue',
        shortcut: 'Alt+Up',
        keywords: 'reorder priority',
        run: () => app.nudge(-1),
      },
      {
        id: 'sel.down',
        group: g,
        title: 'Move the selection down in its queue',
        shortcut: 'Alt+Down',
        keywords: 'reorder priority',
        run: () => app.nudge(1),
      },
      {
        id: 'sel.clear',
        group: g,
        title: 'Clear the selection',
        shortcut: 'Escape',
        run: () => app.clearSelection(),
      },
    );
  }

  out.push(
    {
      id: 'dl.add',
      group: 'Downloads',
      title: 'Add a download',
      keywords: 'new link url paste',
      shortcut: 'Ctrl+N',
      run: () => app.openDialog({ kind: 'add', urls: [] }),
    },
    {
      id: 'dl.pauseall',
      group: 'Downloads',
      title: 'Pause all downloads',
      keywords: 'stop everything',
      run: () => app.pauseAll(),
    },
    {
      id: 'dl.resumeall',
      group: 'Downloads',
      title: 'Resume all downloads',
      keywords: 'start everything continue',
      run: () => app.resumeAll(),
    },
    {
      id: 'dl.retryfailed',
      group: 'Downloads',
      title: 'Retry all failed downloads',
      keywords: 'try again errors needs attention',
      run: () => app.retryAllFailed(),
    },
    {
      id: 'dl.selectall',
      group: 'Downloads',
      title: 'Select every download in the list',
      shortcut: 'Ctrl+A',
      run: () => app.selectAll(),
    },
    {
      id: 'dl.search',
      group: 'Downloads',
      title: 'Search the list',
      shortcut: 'Ctrl+F',
      keywords: 'find filter',
      run: () => focusSearch(),
    },
  );

  out.push({
    id: 'show.all',
    group: 'Show',
    title: 'Show all downloads',
    run: () => app.show({ kind: 'all' }),
  });
  for (const group of STATUS_GROUPS) {
    out.push({
      id: `show.status.${group}`,
      group: 'Show',
      title: `Show ${STATUS_GROUP_LABEL[group].toLowerCase()}`,
      keywords: 'filter status',
      run: () => app.show({ kind: 'status', group }),
    });
  }
  for (const category of CATEGORIES) {
    out.push({
      id: `show.category.${category}`,
      group: 'Show',
      title: `Show ${CATEGORY_LABEL[category].toLowerCase()}`,
      keywords: 'filter category type',
      run: () => app.show({ kind: 'category', category }),
    });
  }

  out.push({
    id: 'queue.new',
    group: 'Queues',
    title: 'Create a queue',
    keywords: 'new add',
    run: () => app.openDialog({ kind: 'queue', id: null }),
  });
  for (const q of app.queues) {
    out.push(
      {
        id: `queue.show.${q.id}`,
        group: 'Queues',
        title: `Show the ${q.name} queue`,
        keywords: 'open view',
        run: () => app.show({ kind: 'queue', id: q.id }),
      },
      {
        id: `queue.edit.${q.id}`,
        group: 'Queues',
        title: `Edit ${q.name}: name, schedule and limits`,
        keywords: 'rename schedule speed limit concurrent settings',
        run: () => app.openDialog({ kind: 'queue', id: q.id }),
      },
      q.paused
        ? {
            id: `queue.resume.${q.id}`,
            group: 'Queues',
            title: `Resume the ${q.name} queue`,
            keywords: 'start run continue unpause',
            run: () => app.setQueuePaused(q.id, false),
          }
        : {
            id: `queue.pause.${q.id}`,
            group: 'Queues',
            title: `Pause the ${q.name} queue`,
            keywords: 'stop hold',
            run: () => app.setQueuePaused(q.id, true),
          },
    );
    if (q.schedule && app.isOutsideSchedule(q)) {
      out.push({
        id: `queue.startnow.${q.id}`,
        group: 'Queues',
        title: `Start ${q.name} now, outside its hours`,
        keywords: 'run schedule override immediately',
        run: () => app.startQueueNow(q.id),
      });
    }
    if (!q.is_default) {
      out.push({
        id: `queue.delete.${q.id}`,
        group: 'Queues',
        title: `Delete the ${q.name} queue`,
        keywords: 'remove',
        run: () => app.openDialog({ kind: 'queue', id: q.id }),
      });
    }
  }

  out.push({
    id: 'settings.open',
    group: 'Settings',
    title: 'Open settings',
    shortcut: 'Ctrl+,',
    keywords: 'preferences options',
    run: () => app.openSettings(),
  });
  for (const s of SETTINGS) {
    const section = SECTIONS.find((x) => x.id === s.section)?.title ?? '';
    out.push({
      id: `setting.${s.id}`,
      group: 'Settings',
      title: s.id === 'export' || s.id === 'import' ? s.label : `${section}: ${s.label}`,
      keywords: `${s.help} ${s.keywords}`,
      run: () =>
        s.id === 'export' || s.id === 'import'
          ? app.openDialog({ kind: s.id })
          : app.openSettings(s.id),
    });
  }
  const ask = app.settings?.extension.ask_before_download ?? true;
  const progress = app.settings?.show_progress_windows ?? true;
  const capture = app.settings?.extension.capture_enabled ?? true;
  out.push(
    {
      id: 'toggle.ask',
      group: 'Settings',
      title: ask
        ? 'Start browser downloads without asking'
        : 'Show a window before each browser download',
      keywords: 'confirm prompt ask window question',
      run: () => app.setAskBeforeDownload(!ask),
    },
    {
      id: 'toggle.progress',
      group: 'Settings',
      title: progress
        ? 'Close download windows when downloads start'
        : 'Keep download windows open to show progress',
      keywords: 'progress window close keep',
      run: () => app.setShowProgressWindows(!progress),
    },
    {
      id: 'toggle.capture',
      group: 'Settings',
      title: capture ? 'Stop catching browser downloads' : 'Catch browser downloads',
      keywords: 'extension capture off on',
      run: () => app.updateSettings((d) => void (d.extension.capture_enabled = !capture)),
    },
    {
      id: 'theme.system',
      group: 'Settings',
      title: 'Use the system theme',
      keywords: 'appearance auto',
      run: () => app.updateSettings((d) => void (d.theme = 'system')),
    },
    {
      id: 'theme.light',
      group: 'Settings',
      title: 'Use the light theme',
      keywords: 'appearance',
      run: () => app.updateSettings((d) => void (d.theme = 'light')),
    },
    {
      id: 'theme.dark',
      group: 'Settings',
      title: 'Use the dark theme',
      keywords: 'appearance night',
      run: () => app.updateSettings((d) => void (d.theme = 'dark')),
    },
  );

  out.push({
    id: 'help.shortcuts',
    group: 'Help',
    title: 'Show keyboard shortcuts',
    shortcut: 'F1',
    keywords: 'keys hotkeys help',
    run: () => app.openDialog({ kind: 'shortcuts' }),
  });

  if (app.waitingCount > 0) {
    out.push({
      id: 'questions.show',
      group: 'Downloads',
      title:
        app.waitingCount === 1
          ? 'Show the download that is waiting for you'
          : `Show the ${app.waitingCount} downloads that are waiting for you`,
      keywords: 'waiting answer ask confirm pending capture video quality window add-on',
      run: () => app.showQuestionWindows(),
    });
  }

  // The add-on's own actions, so it can be installed or removed without visiting Settings.
  for (const addon of app.addons ?? []) {
    const name = addon.name.toLowerCase();
    const action = addonAction(addon);
    const keywords = 'addon add-on yt-dlp ffmpeg deno youtube video media';
    if (action) {
      const verb = action === 'update' ? 'Update' : 'Install';
      out.push({
        id: `addon.${action}.${addon.id}`,
        group: 'Settings',
        title: `${verb} the ${name}`,
        keywords,
        run: () => {
          app.openSettings('media_addon');
          return action === 'update' ? app.updateAddon(addon.id) : app.installAddon(addon.id);
        },
      });
    }
    if (isInstalled(addon)) {
      out.push({
        id: `addon.check.${addon.id}`,
        group: 'Settings',
        title: `Check for ${name} updates`,
        keywords: `${keywords} update newer version check`,
        run: () => app.checkAddonUpdates(),
      });
      out.push({
        id: `addon.remove.${addon.id}`,
        group: 'Settings',
        title: `Remove the ${name}`,
        keywords: `${keywords} uninstall delete`,
        // Removing stops video downloads, so the palette asks first.
        run: () => app.openDialog({ kind: 'remove-addon', id: addon.id }),
      });
    }
  }

  // Only the mock backend has sample-data actions; they live in the mock module, so none of
  // their text ships in a production build.
  const sample = app.ready ? app.backend.sampleData : undefined;
  for (const c of sample?.commands ?? []) out.push({ ...c, group: 'Sample data' });
  return out;
}

export function focusSearch(): void {
  const el = document.getElementById('list-search');
  if (el instanceof HTMLInputElement) {
    el.focus();
    el.select();
  }
}

function words(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[^a-z0-9]+/)
    .filter(Boolean);
}

function isSubsequence(needle: string, hay: string): boolean {
  let i = 0;
  for (const ch of hay) if (ch === needle[i]) i += 1;
  return i === needle.length;
}

/**
 * Scores how well `query` matches `text`. Every typed word must match the start of a word in
 * the text ("pau" finds "Pause"), or be a short form of one word that starts with the same
 * letter ("pse" finds "Pause"). Letters scattered across words do not count, so "move" does
 * not find "Resume". `-1` means no match.
 */
export function fuzzyScore(query: string, text: string): number {
  const typed = words(query);
  if (typed.length === 0) return 0;
  const available = words(text);
  let score = 0;
  for (const q of typed) {
    let best = -1;
    for (const w of available) {
      if (w === q) best = Math.max(best, SCORE_WHOLE_WORD);
      else if (w.startsWith(q)) best = Math.max(best, SCORE_PREFIX + q.length / w.length);
      else if (q.length >= 2 && w.startsWith(q.charAt(0)) && isSubsequence(q, w))
        best = Math.max(best, SCORE_SHORT_FORM);
    }
    if (best < 0) return -1;
    score += best;
  }
  // The words typed together, in order, beat the same words spread out.
  if (text.toLowerCase().includes(query.toLowerCase().trim())) score += SCORE_PHRASE;
  return score;
}

export function searchCommands(commands: readonly Command[], query: string): Command[] {
  if (!query.trim()) return [...commands];
  const scored: { c: Command; s: number }[] = [];
  for (const c of commands) {
    const titleScore = fuzzyScore(query, c.title);
    // Keywords count for less than the title, but let "bandwidth" find the speed limit.
    const kwScore = c.keywords
      ? fuzzyScore(query, `${c.title} ${c.keywords}`) * KEYWORD_WEIGHT
      : -1;
    const s = Math.max(titleScore, kwScore);
    if (s >= 0) scored.push({ c, s });
  }
  scored.sort((a, b) => b.s - a.s);
  return scored.map((x) => x.c);
}
