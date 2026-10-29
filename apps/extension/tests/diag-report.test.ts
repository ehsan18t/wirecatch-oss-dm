import { describe, expect, it } from 'vitest';
import { diagnosticsReport } from '../src/lib/diag/report';
import type { DiagEntry } from '../src/lib/diag/log';

const AT = Date.UTC(2026, 9, 29, 8, 30);

function entry(change: Partial<DiagEntry>): DiagEntry {
  return {
    at: AT,
    level: 'warn',
    scope: 'native',
    message: 'The app did not answer',
    detail: null,
    repeated: 0,
    private: false,
    ...change,
  };
}

const identity = {
  client: 'extension' as const,
  client_version: '1.2.3',
  browser: { name: 'edge', version: '130.0' },
};

describe('the diagnostics report', () => {
  it('names the extension, the browser and the connection, then each entry with its detail', () => {
    const text = diagnosticsReport({
      identity,
      connection: 'Closed',
      now: AT + 60_000,
      entries: [entry({ repeated: 2, detail: 'timed out after 5 s' })],
    });
    expect(text.split('\n')).toEqual([
      'Wirecatch extension 1.2.3',
      'Browser: edge 130.0',
      'Connection: Closed',
      'Copied: 2026-10-29T08:31:00.000Z',
      '',
      '1 entry, oldest first:',
      '2026-10-29T08:30:00.000Z warn native: The app did not answer (3 times)',
      '  timed out after 5 s',
    ]);
  });

  it('leaves out what was logged while a private window was open, and says how much', () => {
    const text = diagnosticsReport({
      identity: { ...identity, browser: null },
      connection: 'Connected',
      now: AT,
      entries: [entry({ private: true, message: 'from a private tab' })],
    });
    expect(text).toContain('Browser: unknown');
    expect(text).toContain('Nothing was logged.');
    expect(text).toContain('1 from private windows left out.');
    expect(text).not.toContain('from a private tab');
  });
});
