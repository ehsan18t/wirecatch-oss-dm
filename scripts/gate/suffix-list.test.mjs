import assert from 'node:assert/strict';
import { test } from 'node:test';
import { GateError } from './process.mjs';
import { pinnedVersion, staleSuffixList } from './suffix-list.mjs';

const TIMES = {
  created: '2019-01-01T00:00:00.000Z',
  modified: '2026-10-07T00:00:00.000Z',
  '7.4.10': '2026-07-01T00:00:00.000Z',
  '7.4.11': '2026-08-01T00:00:00.000Z',
  '8.0.0-beta.1': '2026-07-15T00:00:00.000Z',
  '7.4.12': '2026-10-07T00:00:00.000Z',
};

test('a pin on the latest release is current while upstream keeps publishing', () => {
  const now = new Date('2027-03-01T00:00:00.000Z');
  assert.equal(staleSuffixList({ pinned: '7.4.12', times: TIMES, now }), null);
});

test('a newer release out for less than the limit does not block a release', () => {
  const now = new Date('2026-09-15T00:00:00.000Z');
  assert.equal(staleSuffixList({ pinned: '7.4.10', times: TIMES, now }), null);
});

test('a newer release out for longer than the limit blocks a release, naming the newest overdue one', () => {
  const now = new Date('2026-12-31T00:00:00.000Z');
  const problem = staleSuffixList({ pinned: '7.4.10', times: TIMES, now });
  assert.match(problem ?? '', /7\.4\.10 is more than 60 days behind 7\.4\.12/);
});

test('prereleases and registry bookkeeping are not releases', () => {
  const now = new Date('2026-09-20T00:00:00.000Z');
  const times = { ...TIMES, '7.4.11': '2026-09-10T00:00:00.000Z' };
  assert.equal(staleSuffixList({ pinned: '7.4.10', times, now }), null);
});

test('a pin the registry does not know is reported', () => {
  const problem = staleSuffixList({ pinned: '9.9.9', times: TIMES, now: new Date() });
  assert.match(problem ?? '', /not in the registry/);
});

test('an upstream silent for longer than the limit blocks a release, even on its latest release', () => {
  const now = new Date('2027-06-01T00:00:00.000Z');
  const problem = staleSuffixList({ pinned: '7.4.12', times: TIMES, now });
  assert.match(problem ?? '', /no release of tldts for more than 180 days/i);
  assert.match(problem ?? '', /7\.4\.12/);
});

test('release dates the registry sent but that cannot be read block a release', () => {
  const now = new Date('2027-06-01T00:00:00.000Z');
  const unreadable = [
    { pinned: '7.4.10', times: { ...TIMES, '7.4.10': 'garbage' } },
    { pinned: '7.4.10', times: { ...TIMES, '7.4.12': 'garbage' } },
    { pinned: '7.4.12', times: { ...TIMES, '7.4.11': 17 } },
    { pinned: '7.4.12', times: null },
    { pinned: '7.4.12', times: ['7.4.12'] },
  ];
  for (const { pinned, times } of unreadable) {
    assert.match(
      staleSuffixList({ pinned, times, now }) ?? '',
      /could not be read/,
      JSON.stringify(times),
    );
  }
});

test('the extension must pin the list to an exact version', () => {
  assert.equal(pinnedVersion({ dependencies: { tldts: '7.4.18' } }), '7.4.18');
  for (const manifest of [{ dependencies: { tldts: '^7.4.18' } }, { dependencies: {} }, {}]) {
    assert.throws(() => pinnedVersion(manifest), GateError);
  }
});
