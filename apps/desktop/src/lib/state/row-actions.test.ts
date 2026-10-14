import type { DownloadStatus } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import {
  applicable,
  canCancel,
  canChangeFolder,
  canOpen,
  canPause,
  canRefreshLink,
  canResume,
  canRetry,
  linkMayHaveExpired,
} from './row-actions';

const ALL: readonly DownloadStatus[] = [
  'queued',
  'connecting',
  'downloading',
  'processing',
  'paused',
  'completed',
  'failed',
  'cancelled',
];

const takes = (test: (row: { status: DownloadStatus }) => boolean) =>
  ALL.filter((status) => test({ status }));

describe('which actions apply to a download', () => {
  it('offers each action for the statuses it makes sense in', () => {
    expect(takes(canPause)).toEqual(['queued', 'connecting', 'downloading', 'processing']);
    expect(takes(canResume)).toEqual(['paused']);
    expect(takes(canRetry)).toEqual(['failed', 'cancelled']);
    expect(takes(canOpen)).toEqual(['completed']);
    expect(takes(canCancel)).toEqual([
      'queued',
      'connecting',
      'downloading',
      'processing',
      'paused',
      'failed',
    ]);
    expect(takes(canRefreshLink)).toEqual(['queued', 'connecting', 'downloading', 'paused']);
    expect(takes(canChangeFolder)).not.toContain('completed');
  });

  it('offers what any download of a selection takes, and opening for one finished file only', () => {
    const many = applicable([{ status: 'downloading' }, { status: 'paused' }]);
    expect(many).toEqual({ pause: true, resume: true, retry: false, open: false, cancel: true });
    expect(applicable([{ status: 'completed' }]).open).toBe(true);
    expect(applicable([{ status: 'completed' }, { status: 'completed' }]).open).toBe(false);
    expect(applicable([])).toEqual({
      pause: false,
      resume: false,
      retry: false,
      open: false,
      cancel: false,
    });
  });

  it('asks about an expired link only where the link may be why the download is stuck', () => {
    const healthy = ALL.filter((status) => linkMayHaveExpired({ status, waiting: null }));
    expect(healthy).toEqual(['paused']);
    const refused = { type: 'server_refused', data: { retry_at: 0 } } as const;
    expect(linkMayHaveExpired({ status: 'queued', waiting: refused })).toBe(true);
    expect(linkMayHaveExpired({ status: 'failed', waiting: refused })).toBe(false);
  });
});
