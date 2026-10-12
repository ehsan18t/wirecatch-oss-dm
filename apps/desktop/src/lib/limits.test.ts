import {
  DEFAULT_CONNECTIONS_PER_DOWNLOAD,
  DEFAULT_MAX_CONCURRENT_DOWNLOADS,
  MAX_CONCURRENT_DOWNLOADS,
  MAX_CONNECTIONS_PER_DOWNLOAD,
} from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { concurrentDownloads, connectionsPerDownload, queueConcurrentProblem } from './limits';

describe('download limits', () => {
  it('accepts as many downloads at once as the core does, not 32', () => {
    expect(concurrentDownloads(50)).toBe(50);
    expect(concurrentDownloads(MAX_CONCURRENT_DOWNLOADS)).toBe(MAX_CONCURRENT_DOWNLOADS);
    expect(concurrentDownloads(MAX_CONCURRENT_DOWNLOADS + 1)).toBe(MAX_CONCURRENT_DOWNLOADS);
    expect(concurrentDownloads(0)).toBe(1);
    expect(concurrentDownloads(2.6)).toBe(3);
  });

  it("falls back to the core's defaults for what is not a number", () => {
    expect(concurrentDownloads(Number.NaN)).toBe(DEFAULT_MAX_CONCURRENT_DOWNLOADS);
    expect(connectionsPerDownload(Number.NaN)).toBe(DEFAULT_CONNECTIONS_PER_DOWNLOAD);
  });

  it('keeps connections per download within the core limit', () => {
    expect(connectionsPerDownload(MAX_CONNECTIONS_PER_DOWNLOAD + 1)).toBe(
      MAX_CONNECTIONS_PER_DOWNLOAD,
    );
    expect(connectionsPerDownload(-3)).toBe(1);
  });

  it('lets a queue run as many downloads at once as the core allows', () => {
    expect(queueConcurrentProblem(50)).toBeNull();
    expect(queueConcurrentProblem(MAX_CONCURRENT_DOWNLOADS)).toBeNull();
    expect(queueConcurrentProblem(MAX_CONCURRENT_DOWNLOADS + 1)).toBe(
      `Downloads at the same time must be between 1 and ${MAX_CONCURRENT_DOWNLOADS}.`,
    );
    expect(queueConcurrentProblem(0)).not.toBeNull();
    expect(queueConcurrentProblem(Number.NaN)).not.toBeNull();
  });
});
