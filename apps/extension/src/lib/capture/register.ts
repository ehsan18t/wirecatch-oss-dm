import { fallback, log } from '../diag/log';
import type { DownloadItemLike } from './candidate';

/** A failed takeover leaves the download in the browser, so the person sees it there, not in the app. */
const TAKEOVER_FAILED = 'Taking over a download failed; the browser keeps it';

/** The download events this extension listens to (`browser.downloads`). */
export interface DownloadEvents {
  /** Chromium only; Firefox does not have it, whatever the shared typings say. */
  onDeterminingFilename?: {
    addListener(
      listener: (item: DownloadItemLike, suggest: () => void) => boolean | undefined,
    ): void;
  };
  onCreated: { addListener(listener: (item: DownloadItemLike) => void): void };
  onChanged: { addListener(listener: (delta: DownloadDelta) => void): void };
  onErased: { addListener(listener: (downloadId: number) => void): void };
}

/** What changed about a download (`downloads.onChanged`); only the fields read here. */
export interface DownloadDelta {
  id: number;
  paused?: { current?: boolean };
  state?: { current?: string };
}

export interface CaptureHandler {
  onDownload(item: DownloadItemLike): Promise<unknown>;
  /** The download now runs, finished, was cancelled or was erased from the list: it no longer waits paused. */
  onDownloadMoved(downloadId: number): void;
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
  downloads.onChanged.addListener((delta) => {
    const finished = delta.state?.current !== undefined && delta.state.current !== 'in_progress';
    if (delta.paused?.current === false || finished) handler.onDownloadMoved(delta.id);
  });
  downloads.onErased.addListener((downloadId) => {
    handler.onDownloadMoved(downloadId);
  });
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
          .catch(fallback('capture', TAKEOVER_FAILED, undefined, 'error'))
          .finally(suggestOnce);
      } catch (error) {
        log.error('capture', TAKEOVER_FAILED, error);
        suggestOnce();
      }
      // `suggest` is called asynchronously, so Chromium must be told to wait for it.
      return true;
    });
    return 'determining_filename';
  }
  downloads.onCreated.addListener((item) => {
    try {
      handler.onDownload(item).catch(fallback('capture', TAKEOVER_FAILED, undefined, 'error'));
    } catch (error) {
      log.error('capture', TAKEOVER_FAILED, error);
    }
  });
  return 'created';
}
