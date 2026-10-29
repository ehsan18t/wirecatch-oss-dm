// The text "Copy diagnostics" puts on the clipboard (EXT-7): the extension, the browser and the connection, then what was logged. Entries made while a private window was open stay out, as private browsing leaves nothing behind.
import { PRODUCT_NAME } from '@wirecatch/product';
import type { HelloIdentity } from '../native/client';
import type { DiagEntry } from './log';

export interface ReportFacts {
  identity: HelloIdentity;
  /** The connection to the app as the popup shows it ("Connected", "Closed"). */
  connection: string;
  /** Milliseconds since the epoch. */
  now: number;
  entries: readonly DiagEntry[];
}

export function diagnosticsReport({ identity, connection, now, entries }: ReportFacts): string {
  const browser = identity.browser
    ? [identity.browser.name, identity.browser.version].filter(Boolean).join(' ')
    : 'unknown';
  const shared = entries.filter((entry) => !entry.private);
  const left = entries.length - shared.length;
  const lines = [
    `${PRODUCT_NAME} extension ${identity.client_version}`,
    `Browser: ${browser}`,
    `Connection: ${connection}`,
    `Copied: ${new Date(now).toISOString()}`,
    '',
    shared.length === 0
      ? 'Nothing was logged.'
      : `${String(shared.length)} ${shared.length === 1 ? 'entry' : 'entries'}, oldest first:`,
  ];
  if (left > 0) lines.push(`${String(left)} from private windows left out.`);
  for (const entry of shared) {
    const times = entry.repeated > 0 ? ` (${String(entry.repeated + 1)} times)` : '';
    lines.push(
      `${new Date(entry.at).toISOString()} ${entry.level} ${entry.scope}: ${entry.message}${times}`,
    );
    if (entry.detail) lines.push(`  ${entry.detail}`);
  }
  return lines.join('\n');
}
