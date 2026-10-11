import type { DownloadItemLike } from './candidate';

/** The download events this extension listens to (`browser.downloads`). */
export interface DownloadEvents {
  /** Chromium only; Firefox does not have it, whatever the shared typings say. */
  onDeterminingFilename?: {
    addListener(
      listener: (item: DownloadItemLike, suggest: () => void) => boolean | undefined,
    ): void;
  };
  onCreated: { addListener(listener: (item: DownloadItemLike) => void): void };
}

export interface CaptureHandler {
  onDownload(item: DownloadItemLike): Promise<unknown>;
}

/**
 * Starts listening for new browser downloads and returns which event is used.
 *
 * Chromium: `onDeterminingFilename`, where the name, type and size are known and
 * the download (and any "Save as" dialog) waits until `suggest` is called, so
 * nothing reaches the disk or the screen before the app has answered. `suggest`
 * is called exactly once on every path, including errors, or the download would
 * hang. Firefox: `onCreated`, the earliest point it offers.
 */
export function registerCapture(
  downloads: DownloadEvents,
  handler: CaptureHandler,
): 'determining_filename' | 'created' {
  const onDeterminingFilename = downloads.onDeterminingFilename;
  if (onDeterminingFilename !== undefined) {
    onDeterminingFilename.addListener((item, suggest) => {
      let suggested = false;
      const suggestOnce = (): void => {
        if (suggested) return;
        suggested = true;
        try {
          suggest();
        } catch {
          // The download was cancelled after the handover; nothing left to name.
        }
      };
      try {
        handler
          .onDownload(item)
          .catch(() => undefined)
          .finally(suggestOnce);
      } catch {
        suggestOnce();
      }
      // `suggest` is called asynchronously, so Chromium must be told to wait for it.
      return true;
    });
    return 'determining_filename';
  }
  downloads.onCreated.addListener((item) => {
    try {
      handler.onDownload(item).catch(() => undefined);
    } catch {
      // A failure here leaves the download in the browser, untouched.
    }
  });
  return 'created';
}
