// What the main window does with keys, pasted text and dropped links, wherever they land: each key runs the palette's command for it, so a key and the palette never differ; a link pasted or dropped anywhere opens the Add dialog with it, and while one is dragged over the window the window says it takes it.

import { runKey } from '$lib/commands';
import { isDownloadDrag } from '$lib/dnd';
import { isEditable, windowCommand } from '$lib/keymap';
import type { AppState } from '$lib/state/app.svelte';
import { cycleRegion } from './regions';

/** What the window's input needs to know beyond the state: whether a layer of its own (the navigation drawer) has the keyboard. */
export interface WindowInputHost {
  app: AppState;
  /** A modal layer is open that takes every key, as the drawer does. */
  layerOpen: () => boolean;
  /** A link is being dragged over the window (true), or has left it or been dropped (false). */
  linkOver: (over: boolean) => void;
}

/** A key or a paste in one of the sample app's download windows, which float in this page and keep their own keys. */
function inDownloadWindow(e: Event): boolean {
  return e.target instanceof Element && e.target.closest('[data-download-window]') !== null;
}

/** A link dragged in from a browser or another app, not one of the list's own rows. */
function isLinkDrag(e: DragEvent): boolean {
  const types = e.dataTransfer?.types ?? [];
  return !isDownloadDrag(e) && (types.includes('text/uri-list') || types.includes('text/plain'));
}

/** F6 and Shift+F6: to the next region or the one before; inside a modal layer (the details over a veil) the focus stays in it. */
function regionKey(e: KeyboardEvent, command: 'next-region' | 'previous-region'): void {
  if (e.target instanceof Element && e.target.closest('dialog[open]')) return;
  cycleRegion(command === 'next-region' ? 1 : -1);
}

export function windowInput({ app, layerOpen, linkOver }: WindowInputHost) {
  /** A dialog takes the keyboard and the pointer; questions have their own windows (D23). */
  const blocked = () => app.dialog !== null;

  return {
    onkeydown: (e: KeyboardEvent) => {
      // A layer nearer the key (the details panel, a menu, the search) already used it.
      if (e.defaultPrevented || inDownloadWindow(e) || layerOpen()) return;
      const command = windowCommand(e);
      if (command === null) return;
      if (command === 'palette') {
        // The palette's own key also closes it.
        e.preventDefault();
        if (!blocked()) app.paletteOpen = !app.paletteOpen;
        return;
      }
      // Every other key waits while the palette or a dialog is open.
      if (app.paletteOpen || blocked()) return;
      e.preventDefault();
      if (command === 'next-region' || command === 'previous-region') regionKey(e, command);
      else runKey(app, command);
    },

    onpaste: (e: ClipboardEvent) => {
      if (inDownloadWindow(e) || app.paletteOpen || blocked() || isEditable(e.target)) return;
      const text = e.clipboardData?.getData('text/plain') ?? '';
      if (!text) return;
      e.preventDefault();
      app.pasteText(text);
    },

    ondragover: (e: DragEvent) => {
      if (blocked() || !isLinkDrag(e)) return;
      e.preventDefault();
      if (e.dataTransfer) e.dataTransfer.dropEffect = 'copy';
      linkOver(true);
    },

    // Moving from one element to another inside the window also leaves the first; only leaving the window has nothing to go to.
    ondragleave: (e: DragEvent) => {
      if (!(e.relatedTarget instanceof Node)) linkOver(false);
    },

    ondrop: (e: DragEvent) => {
      linkOver(false);
      if (blocked() || !isLinkDrag(e)) return;
      e.preventDefault();
      // `getData` gives an empty string for a type the drag does not carry.
      const links = e.dataTransfer?.getData('text/uri-list') ?? '';
      const text = links === '' ? (e.dataTransfer?.getData('text/plain') ?? '') : links;
      if (text) app.pasteText(text);
    },
  };
}
