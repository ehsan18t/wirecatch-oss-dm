/**
 * The one place in the extension that writes to the console, and only in development builds
 * (`log.ts` decides). The shared ESLint configuration allows `console` in this file alone, so
 * every other module logs through `log.ts` and its redaction.
 */
import type { DiagEntry } from './log';

/** Writes one entry at its own console level, so the browser's console filters work on it. */
export function writeToConsole(entry: DiagEntry): void {
  const line = `[wirecatch:${entry.scope}] ${entry.message}`;
  const args = entry.detail === null ? [line] : [line, entry.detail];
  switch (entry.level) {
    case 'error':
      console.error(...args);
      break;
    case 'warn':
      console.warn(...args);
      break;
    case 'info':
      console.info(...args);
      break;
    case 'debug':
      console.debug(...args);
      break;
  }
}
