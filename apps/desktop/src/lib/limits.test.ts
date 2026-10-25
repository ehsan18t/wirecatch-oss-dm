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
  });

  it('says what the app still limits, whether a queue has its own limit or not', () => {
    expect(concurrentLimitHelp(false, 5)).toBe("Off, only the app's limit of 5 applies.");
    expect(concurrentLimitHelp(true, 5)).toBe(
      "The rest wait their turn; the app's limit of 5 still applies.",
    );
    expect(speedLimitHelp(false, null)).toBe('Off, it downloads at full speed.');
    expect(speedLimitHelp(true, 2 * MB)).toBe(
      "Shared by everything running in this queue; the app's limit of 2 MB/s still applies.",
    );
    expect(ownConnectionsHelp(false, 8)).toBe("Off, it uses the app's 8.");
  });
});
