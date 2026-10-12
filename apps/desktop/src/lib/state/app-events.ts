// What the main window does with each of the core's events.

import type { AppState } from './app.svelte';
import { ignore, type UiEventHandlers } from './events';

export function mainWindowEvents(app: AppState): UiEventHandlers {
  return {
    downloads_changed: (e) => app.downloads.apply(e.data),
    downloads_removed: (e) => {
      app.downloads.remove(e.data);
      app.selection.forget(e.data);
    },
    queues_changed: (e) => {
      app.queues.list = e.data;
      const view = app.view;
      // A queue on screen that was deleted leaves the list showing everything.
      if (view.kind === 'queue' && !e.data.some((q) => q.id === view.id)) {
        app.view = { kind: 'all' };
      }
    },
    settings_changed: (e) => {
      app.settings = e.data;
    },
    captured: (e) => {
      app.downloads.apply([e.data]);
      app.toasts.captured(e.data);
    },
    confirmation_requested: (e) => app.questions.confirmationRequested(e.data),
    media_choice_requested: (e) => app.questions.mediaChoiceRequested(e.data),
    media_analysing: (e) => app.questions.analysing(e.data.id),
    addon_required: (e) => app.questions.addonRequired(e.data.id),
    confirmation_closed: (e) => app.questions.closed(e.data.id),
    addons_changed: (e) => app.addons.changed(e.data),
    settings_imported: (e) => {
      app.transfer.lastImport = e.data;
    },
    // Not a passing note: it stays until the person deals with it.
    storage_failed: (e) => {
      app.storageProblem = e.data.message;
    },
    // The desktop shell brings the window to the front; nothing changes in it.
    window_requested: ignore,
  };
}
