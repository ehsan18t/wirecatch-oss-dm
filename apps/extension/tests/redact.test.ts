import {
  expected,
  redactionCases,
  secretsFor,
  wrappings,
} from '@wirecatch/protocol/testing/redaction';
import { describe, expect, it } from 'vitest';
import { describeError, MAX_TEXT, redact } from '../src/lib/diag/redact';
import { RequestError } from '../src/lib/native/client';

// The samples are in crates/test-support/data/redaction.json, which the log's tests load too (D40.10); only what tests this package's own code stays here.
describe('redact, the shared samples', () => {
  const cases = redactionCases();

  it('reads every sample with an expected entry as expected', () => {
    const wrong = cases.flatMap((sample) => {
      const entry = expected(sample, 'diagnostics');
      const got = redact(sample.text);
      return entry === null || got === entry
        ? []
        : [
            `${JSON.stringify(sample.text)} gave ${JSON.stringify(got)}, not ${JSON.stringify(entry)}`,
          ];
    });
    expect(wrong).toEqual([]);
  });

  it('keeps no secret of a sample it reads, in any of the shared wrappings', () => {
    const leaked = cases.flatMap((sample) =>
      wrappings(sample.text).flatMap((wrapped) => {
        const entry = redact(wrapped);
        return secretsFor(sample, 'diagnostics')
          .filter((secret) => entry.includes(secret))
          .map((secret) => `${JSON.stringify(wrapped)} kept ${secret}: ${JSON.stringify(entry)}`);
      }),
    );
    expect(leaked).toEqual([]);
  });

  it('keeps no address path, file path or name of a person', () => {
    const kept = cases.flatMap((sample) => {
      const entry = redact(sample.text);
      return sample.private
        .filter((part) => entry.includes(part))
        .map((part) => `${JSON.stringify(sample.text)} kept ${part}: ${JSON.stringify(entry)}`);
    });
    expect(kept).toEqual([]);
  });
});

describe('redact, the length of an entry', () => {
  it('cuts a long text, page-sized ones included', () => {
    const text = redact(`<html>${'page text '.repeat(500_000)}</html>`);
    expect(text).toHaveLength(MAX_TEXT);
    expect(text.endsWith('…')).toBe(true);
  });
});

describe('redact, worst cases', () => {
  /** Longest a single entry may take to redact, far above what any input should cost. */
  const CEILING_MS = 30;
  /** Shapes that make a backtracking pattern retry at every position of the longest input read. */
  const HOSTILE = [
    'key-'.repeat(300),
    'sid-'.repeat(300),
    'token'.repeat(240),
    'a-'.repeat(600),
    '"a":'.repeat(300),
    '["a",'.repeat(240),
    'name:"a",value:'.repeat(80),
    '%41'.repeat(400),
    `{"token":${'['.repeat(1190)}`,
    `"token":"${'\\'.repeat(1190)}`,
    ` ${'a.'.repeat(590)}/x`,
    'Cookie: a\n b'.repeat(100),
    `see (https://a.example/${'('.repeat(1150)}`,
  ];

  it.each(HOSTILE.map((text) => [text.slice(0, 16), text]))(
    'redacts %s… in a bounded time',
    (_name, text) => {
      // The best of three, so a pause of the machine itself is not counted.
      const times = [0, 1, 2].map(() => {
        const start = performance.now();
        redact(text);
        return performance.now() - start;
      });
      expect(Math.min(...times)).toBeLessThan(CEILING_MS);
    },
  );
});

describe('describeError', () => {
  it('names an error and its redacted message', () => {
    expect(describeError(new TypeError('bad https://a.example/x?token=1'))).toBe(
      'TypeError: bad https://a.example/…',
    );
    expect(describeError(new Error(''))).toBe('Error');
  });

  it('reads error-shaped objects, and only names others', () => {
    expect(describeError({ message: 'Could not establish connection.' })).toBe(
      'Could not establish connection.',
    );
    expect(describeError({ html: '<p>private page</p>' })).toBe('[object Object]');
    expect(describeError(Object.create(null))).toBe('[object Object]');
  });

  it('describes values that are not objects', () => {
    expect(describeError('plain')).toBe('plain');
    expect(describeError(42)).toBe('42');
    expect(describeError(null)).toBe('null');
    expect(describeError(undefined)).toBe('undefined');
    expect(describeError(Symbol('s'))).toBe('symbol');
    expect(describeError(() => 1)).toBe('function');
  });

  it('keeps one level of cause, aggregated errors and the kind of a request error', () => {
    const cause = new TypeError('socket closed at https://a.example/x');
    expect(describeError(new Error('send failed', { cause }))).toBe(
      'Error: send failed (cause: TypeError: socket closed at https://a.example/…)',
    );
    expect(describeError(new AggregateError([new Error('a'), new Error('b')], 'all failed'))).toBe(
      'AggregateError: all failed (2 errors, first: Error: a)',
    );
    expect(
      describeError(
        new RequestError('timeout', 'Wirecatch did not answer in time.', {
          code: 'internal',
          message: String.raw`disk full at C:\Users\Alice`,
        }),
      ),
    ).toBe('RequestError [timeout, internal]: Wirecatch did not answer in time.');
  });

  it('never throws, whatever it is given', () => {
    const hostile = {
      get message(): string {
        throw new Error('boom');
      },
    };
    expect(describeError(hostile)).toBe('an error that could not be read');
    const proxy = new Proxy(
      {},
      {
        has: () => {
          throw new Error('trap');
        },
      },
    );
    expect(describeError(proxy)).toBe('an error that could not be read');
  });
});
