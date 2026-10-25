import {
  FOLLOW_APP,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
  ownSetting,
  type QueueSettings,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { describe, expect, it } from 'vitest';
import {
  concurrentLimitHelp,
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
    expect(queueLimitFacts(FOLLOWS)).toEqual(["Follows the app's limits"]);
    expect(
      queueLimitFacts({
        ...FOLLOWS,
        max_concurrent: ownSetting(2),
        speed_limit: ownSetting(4 * MB),
        connections: ownSetting(1),
      }),
    ).toEqual(['2 at once', 'Up to 4 MB/s', '1 connection each']);
    expect(queueLimitFacts({ ...FOLLOWS, max_concurrent: ownSetting(2) })).toEqual([
      '2 at once',
      'Other limits follow the app',
    ]);
  });

  it('says what the app still limits, whether a queue has its own limit or not', () => {
    expect(concurrentLimitHelp(false, 5)).toBe("Uses the app's limit: 5 at once.");
    expect(concurrentLimitHelp(true, 5)).toBe(
      "Others in this queue wait their turn. The app's limit of 5 still applies to all downloads.",
    );
    expect(speedLimitHelp(false, null)).toBe("Uses the app's speed limit, which is off.");
    expect(speedLimitHelp(true, 2 * MB)).toBe(
      "Shared by everything running in this queue. The app's limit of 2 MB/s still applies.",
    );
    expect(ownConnectionsHelp(false, 8)).toBe("Uses the app's 8.");
    expect(ownConnectionsHelp(true, 8)).toBe(
      "Each download in this queue opens up to this many, instead of the app's 8.",
    );
  });
});
