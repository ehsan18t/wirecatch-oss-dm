// The sample page's download windows (D23) and its download complete window (D52), made over the
// sample backend, which only the main window's state holds.

import type { Backend } from '../backend';
import { DownloadWindowState, type OpenDownloadWindow } from './download-window.svelte';
import { FinishedWindowState, type OpenFinishedWindow } from './finished-window.svelte';

export interface SampleWindows {
  openWindow: OpenDownloadWindow;
  openFinished: OpenFinishedWindow;
}

/** Makes the sample page's windows over `backend`. */
export function sampleWindows(backend: () => Backend): SampleWindows {
  return {
    openWindow: (target, host) => new DownloadWindowState({ ...host, backend }, target),
    openFinished: (host) =>
      new FinishedWindowState({
        ...host,
        backend,
        finished: () => {
          const sample = backend().sampleData;
          if (!sample) throw new Error('Only the sample app shows its windows inside the page.');
          return sample.finished;
        },
      }),
  };
}
