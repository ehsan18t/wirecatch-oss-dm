import { browser } from 'wxt/browser';
import { sessionArea } from '../browser/adapters';
import { diagnostics as contextDiagnostics, type Diagnostics } from './log';
import {
  forgetPrivate,
  logUncaught,
  persistDiagnostics,
  type DiagContext,
  type ErrorEvents,
} from './persist';

/** Whether any private window is open now. */
async function anyPrivateWindow(): Promise<boolean> {
  const windows = await browser.windows.getAll();
  return windows.some((window) => window.incognito);
}

/**
 * Follows private windows from the background, which serves them too: entries made while one is
 * open are marked, and dropped, from memory and from every context's saved copy, when the last
 * one closes, so nothing about a private session outlives it. Until the first answer comes, one is
 * taken as open, since the background may have been woken by a private tab's event.
 */
function followPrivateWindows(
  diagnostics: Diagnostics,
  area: ReturnType<typeof sessionArea>,
): void {
  diagnostics.setPrivateSession(null);
  const check = (): void => {
    anyPrivateWindow()
      .then((open) => {
        diagnostics.setPrivateSession(open);
        // The background's own key is saved again by its writer, from the entries just filtered.
        if (!open && area !== undefined) {
          forgetPrivate(area, 'background').catch((error: unknown) => {
            diagnostics.log(
              'warn',
              'diag',
              'Could not forget the private entries of the popup',
              error,
            );
          });
        }
      })
      .catch((error: unknown) => {
        // Taken as open, so an entry is kept private rather than not.
        diagnostics.setPrivateSession(true);
        diagnostics.log('warn', 'diag', 'Could not tell whether a private window is open', error);
      });
  };
  browser.windows.onCreated.addListener((window) => {
    if (window.incognito) diagnostics.setPrivateSession(true);
  });
  browser.windows.onRemoved.addListener(check);
  check();
}

/**
 * Decides whether the popup is part of a private session from the window it was opened in. Its
 * process does not tell: in Chromium's default "spanning" mode a popup opened from an incognito
 * window runs in the regular profile, where `extension.inIncognitoContext` is false. Until the
 * window answers, and when it cannot, the popup is taken as private.
 */
function followPopupWindow(diagnostics: Diagnostics): void {
  if (browser.extension.inIncognitoContext) {
    diagnostics.setPrivateSession(true);
    return;
  }
  diagnostics.setPrivateSession(null);
  browser.windows
    .getCurrent()
    .then((window) => {
      diagnostics.setPrivateSession(window.incognito);
    })
    .catch((error: unknown) => {
      diagnostics.setPrivateSession(true);
      diagnostics.log('warn', 'diag', 'Could not tell whether this window is private', error);
    });
}

/**
 * Sets up this context's diagnostics: everything uncaught is logged, entries are kept in session
 * storage when the browser has it, and entries made for private windows do not outlive them.
 * Called first thing in the background and the popup, with this context's `diagnostics`.
 */
export function installDiagnostics(
  context: DiagContext,
  scope: ErrorEvents,
  diagnostics: Diagnostics = contextDiagnostics,
): void {
  logUncaught(scope, context);
  const area = sessionArea();
  if (context === 'background') followPrivateWindows(diagnostics, area);
  // A popup opened in a private window is part of that session as a whole.
  else followPopupWindow(diagnostics);
  if (area !== undefined) void persistDiagnostics(diagnostics, area, context);
}
