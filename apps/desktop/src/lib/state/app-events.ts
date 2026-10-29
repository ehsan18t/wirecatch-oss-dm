// What the main window does with each of the core's events.

import type { AppSettings, DownloadId } from '@wirecatch/protocol';
import type { Addons } from './addons.svelte';
import type { DownloadStore } from './downloads.svelte';
import { ignore, type UiEventHandlers } from './events';
import type { ProxyServers } from './proxy-servers.svelte';
import type { Questions } from './questions.svelte';
import type { Queues } from './queues.svelte';
import type { Selection } from './selection.svelte';
import type { SuffixList } from './suffix-list.svelte';
import type { ImportExport } from './import-export.svelte';
import type { View } from './view';

/** What the core's events change in the main window. */
export interface MainWindowEventsHost {
  readonly downloads: Pick<DownloadStore, 'apply' | 'remove'>;
  readonly selection: Pick<Selection, 'forget'>;
  /** The download whose details show; they close when it is removed. */
  readonly details: DownloadId | null;
  closeDetails: () => void;
  readonly queues: Pick<Queues, 'list'>;
  readonly proxies: Pick<ProxyServers, 'list'>;
  view: View;
  settings: AppSettings | null;
  /** Takes the settings the core sent. */
  settingsArrived: (settings: AppSettings) => void;
  readonly questions: Pick<
    Questions,
    | 'confirmationRequested'
    | 'mediaChoiceRequested'
    | 'fileExistsRequested'
    | 'analysing'
    | 'addonRequired'
    | 'closed'
  >;
  readonly addons: Pick<Addons, 'changed'>;
  readonly suffixList: Pick<SuffixList, 'checked'>;
  readonly transfer: Pick<ImportExport, 'lastImport'>;
  storageProblem: string | null;
}

export function mainWindowEvents(app: MainWindowEventsHost): UiEventHandlers {
  return {
    downloads_changed: (e) => app.downloads.apply(e.data),
    downloads_removed: (e) => {
      app.downloads.remove(e.data);
      app.selection.forget(e.data);
      if (app.details !== null && e.data.includes(app.details)) app.closeDetails();
    },
    queues_changed: (e) => {
      app.queues.list = e.data;
      const view = app.view;
      // A queue on screen that was deleted leaves the list showing everything.
      if (view.kind === 'queue' && e.data.every((q) => q.id !== view.id)) {
        app.view = { kind: 'all' };
      }
    },
    proxy_servers_changed: (e) => {
      app.proxies.list = e.data;
    },
    settings_changed: (e) => app.settingsArrived(e.data),
    // The shell tells the person in a brief notification (D63); the list shows the download.
    captured: (e) => app.downloads.apply([e.data]),
    confirmation_requested: (e) => app.questions.confirmationRequested(e.data),
    media_choice_requested: (e) => app.questions.mediaChoiceRequested(e.data),
    file_exists_requested: (e) => app.questions.fileExistsRequested(e.data),
    // About the download windows, which the shell brings forward (D89).
    capture_repeated: ignore,
    media_analysing: (e) => app.questions.analysing(e.data.id),
    addon_required: (e) => app.questions.addonRequired(e.data.id),
    confirmation_closed: (e) => app.questions.closed(e.data.id),
    addons_changed: (e) => app.addons.changed(e.data),
    suffix_list_checked: (e) => app.suffixList.checked(e.data),
    settings_imported: (e) => {
      app.transfer.lastImport = e.data;
    },
    // Not a passing note: it stays until the person deals with it.
    storage_failed: (e) => {
      app.storageProblem = e.data.message;
    },
    // The desktop shell brings the window to the front; nothing changes in it.
    window_requested: ignore,
    // The shell brings the window up and hands the browser settings over as a reveal, which a
    // page still loading takes too (D61).
    browser_settings_requested: ignore,
    // The shell tells the person (D52); the list already shows how each download ended.
    queue_finished: ignore,
    // Between the shell and the browsers (D65).
    notice_shown: ignore,
    notice_pressed: ignore,
    // The shell tells it in a note (D67); its download arrives in the list when it starts.
    media_captured: ignore,
  };
}
