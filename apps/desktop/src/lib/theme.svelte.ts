// The colour theme of a window: the theme setting on the page's root element, where app.css reads
// it. "System" leaves it to the operating system's preference.

import type { Theme } from '@wirecatch/protocol';

/** Keeps the page's theme on the setting `theme` reads; call it while a component starts. */
export function followTheme(theme: () => Theme | undefined): void {
  $effect(() => {
    const chosen = theme() ?? 'system';
    const root = document.documentElement;
    if (chosen === 'system') delete root.dataset.theme;
    else root.dataset.theme = chosen;
  });
}
