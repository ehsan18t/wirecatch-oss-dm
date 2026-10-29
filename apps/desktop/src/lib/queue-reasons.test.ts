import { describe, expect, it } from 'vitest';
import { queueLine, trialLines } from './queue-reasons';

describe('why a download is in its queue', () => {
  it('says each kind of claim and each other reason in words', () => {
    const claimed = (claim: Parameters<typeof queueLine>[1] & { kind: 'claimed' }) =>
      queueLine('Q', claim);
    expect(claimed({ kind: 'claimed', claim: { kind: 'site', claim: 'youtube.com' } })).toBe(
      'Queue: Q. Its rules match youtube.com.',
    );
    expect(claimed({ kind: 'claimed', claim: { kind: 'file_name', claim: '*.iso' } })).toBe(
      'Queue: Q. Its rules match files named *.iso.',
    );
    expect(claimed({ kind: 'claimed', claim: { kind: 'file_type', claim: 'tar.gz' } })).toBe(
      'Queue: Q. Its rules match .tar.gz files.',
    );
    expect(claimed({ kind: 'claimed', claim: { kind: 'category', claim: 'video' } })).toBe(
      'Queue: Q. Its rules match videos.',
    );
    expect(claimed({ kind: 'claimed', claim: { kind: 'protocol', claim: 'ftp' } })).toBe(
      'Queue: Q. Its rules match FTP links.',
    );
    expect(queueLine('Q', { kind: 'picked' })).toBe('Queue: Q. You picked it.');
    expect(queueLine('Q', { kind: 'browser_queue' })).toBe(
      'Queue: Q. It is the queue for browser downloads.',
    );
    expect(queueLine('Q', { kind: 'default_queue' })).toBe('Queue: Q. No queue’s rules match it.');
  });

  it('names the queue not made yet by the name its form has', () => {
    const lines = trialLines(
      { taken: { queue: null, claim: { kind: 'site', claim: 'a.example' } }, switched_off: null },
      (queue) => (queue === null ? 'New' : 'Saved'),
      'Main',
    );
    expect(lines).toEqual(['Queue: New. Its rules match a.example.']);
  });
});
