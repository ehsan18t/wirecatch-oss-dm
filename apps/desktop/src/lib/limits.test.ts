import {
  FOLLOW_APP,
  MANUAL_QUEUE,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownSetting,
  type QueueSettings,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { describe, expect, it } from 'vitest';
import {
  concurrentLimitHelp,
  firstConcurrent,
  ownConnectionsHelp,
  queueConcurrentProblem,
  queueConnectionsProblem,
  queueLimitFacts,
  speedLimitHelp,
} from './limits';

const FOLLOWS: QueueSettings = {
  name: 'Plain',
  description: '',
  max_concurrent: FOLLOW_APP,
  speed_limit: FOLLOW_APP,
  connections: FOLLOW_APP,
  proxy: FOLLOW_APP,
  exceptions: FOLLOW_APP,
  takes: MANUAL_QUEUE,
  schedule: null,
  notify: 'each_download',
};

describe('download limits', () => {
  it('lets a queue run as many downloads at once as the core allows', () => {
    expect(queueConcurrentProblem(50)).toBeNull();
    expect(queueConcurrentProblem(MAX_CONCURRENT_DOWNLOADS)).toBeNull();
    expect(queueConcurrentProblem(MAX_CONCURRENT_DOWNLOADS + 1)).toBe(
      `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`,
    );
    expect(queueConcurrentProblem(0)).not.toBeNull();
    expect(queueConcurrentProblem(Number.NaN)).not.toBeNull();
  });

  it('lets a queue use as many connections per download as the core allows', () => {
    expect(queueConnectionsProblem(1)).toBeNull();
    expect(queueConnectionsProblem(MAX_CONNECTIONS_PER_DOWNLOAD)).toBeNull();
    expect(queueConnectionsProblem(MAX_CONNECTIONS_PER_DOWNLOAD + 1)).toBe(
      `Connections per download must be between 1 and ${MAX_CONNECTIONS_PER_DOWNLOAD}.`,
    );
    expect(queueConnectionsProblem(0)).not.toBeNull();
  });

  it("says a queue's own limits, or that it follows the app's", () => {
    expect(queueLimitFacts(FOLLOWS)).toEqual(['Uses global limits']);
    expect(
      queueLimitFacts({
        ...FOLLOWS,
        max_concurrent: ownSetting(2),
        speed_limit: ownSetting(4 * MB),
        connections: ownSetting(1),
        proxy: FOLLOW_APP,
        exceptions: FOLLOW_APP,
      }),
    ).toEqual(['2 at once', 'Up to 4 MB/s', '1 connection each']);
    expect(queueLimitFacts({ ...FOLLOWS, max_concurrent: ownSetting(2) })).toEqual([
      '2 at once',
      'Other limits are global',
    ]);
  });

  it('says what the app still limits, whether a queue has its own limit or not', () => {
    expect(concurrentLimitHelp(null, 5)).toBe('Uses the global limit: 5 at once.');
    expect(concurrentLimitHelp(2, 5)).toBe(
      'Up to this many of its downloads run at once, and the rest wait their turn. The global limit of 5 still applies across all queues.',
    );
    expect(concurrentLimitHelp(8, 5)).toMatch(
      /The global limit of 5 is lower, so at most 5 run at once\.$/,
    );
    expect(firstConcurrent(5)).toBe(4);
    expect(firstConcurrent(1)).toBe(1);
    expect(speedLimitHelp(false, null)).toBe('Uses the global limit: none.');
    expect(speedLimitHelp(true, 2 * MB)).toBe(
      'Shared by all downloads in this queue. The global limit of 2 MB/s still applies.',
    );
    expect(ownConnectionsHelp(false, 8)).toBe('Uses the global setting: 8 per download.');
    expect(ownConnectionsHelp(true, 8)).toBe(
      'Each download in this queue uses up to this many connections instead of the global 8.',
    );
  });
});
