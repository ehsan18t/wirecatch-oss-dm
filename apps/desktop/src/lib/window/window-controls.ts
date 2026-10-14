// A download window's caption buttons (D23, D36). The window only closes: it is never maximized
// and its title bar shows no minimize, so its close is all there is to call, and closing through
// the window's own state leaves its question waiting, as Escape does.

import type { WindowControls } from '@wirecatch/ui';

/** The title bar's calls for a window whose only caption button is Close, which runs `close`. */
export function closeOnlyControls(close: () => void): WindowControls {
  const offered = () => Promise.reject(new Error('A download window can only be closed.'));
  return {
    minimize: offered,
    toggleMaximize: offered,
    close: () => {
      close();
      return Promise.resolve();
    },
    isMaximized: () => Promise.resolve(false),
    // Never maximized, so nothing changes for the title bar to follow.
    onResized: () => Promise.resolve(() => undefined),
  };
}
