import {
  GLOBAL,
  MANUAL_QUEUE,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownSetting,
  type QueueSettings,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { describe, expect, it } from 'vitest';
import {
  concurrentCapWarning,
  firstConcurrent,
  globalLimitText,
  queueConcurrentProblem,
  queueConnectionsProblem,
  queueLimitFacts,
} from './limits';

const FOLLOWS: QueueSettings = {
  name: 'Plain',
  description: '',
  max_concurrent: GLOBAL,
  speed_limit: GLOBAL,
  connections: GLOBAL,
  proxy: GLOBAL,
  exceptions: GLOBAL,
  add_downloads: MANUAL_QUEUE,
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
        proxy: GLOBAL,
        exceptions: GLOBAL,
      }),
    ).toEqual(['2 at once', 'Up to 4 MB/s', '1 connection each']);
    expect(queueLimitFacts({ ...FOLLOWS, max_concurrent: ownSetting(2) })).toEqual([
      '2 at once',
      'Other limits are global',
    ]);
  });

  it('warns only when the global limit caps a queue’s own, and names the global value it follows', () => {
    expect(concurrentCapWarning(2, 5)).toBeNull();
    expect(concurrentCapWarning(8, 5)).toBe(
      'The global limit of 5 is lower, so at most 5 run at once.',
    );
    expect(concurrentCapWarning(8, undefined)).toBeNull();
    expect(firstConcurrent(5)).toBe(4);
    expect(firstConcurrent(1)).toBe(1);
    expect(globalLimitText('max_concurrent', 5)).toBe('Global: 5');
    expect(globalLimitText('speed_limit', null)).toBe('Global: no limit');
    expect(globalLimitText('speed_limit', 2 * MB)).toBe('Global: 2 MB/s');
    expect(globalLimitText('connections', undefined)).toBeNull();
  });
});
