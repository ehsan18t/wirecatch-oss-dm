import { describe, expect, it } from 'vitest';
import { queueLine, trialLines } from './queue-reasons';

describe('why a download is in its queue', () => {
  it('says each kind of rule and each other reason in words', () => {
    const matched = (rule: Parameters<typeof queueLine>[1] & { kind: 'matched' }) =>
      queueLine('Q', rule);
    expect(matched({ kind: 'matched', rule: { kind: 'site', value: 'youtube.com' } })).toBe(
      'Queue: Q. Its rules match youtube.com.',
    );
    expect(matched({ kind: 'matched', rule: { kind: 'file_name', value: '*.iso' } })).toBe(
      'Queue: Q. Its rules match files named *.iso.',
    );
    expect(matched({ kind: 'matched', rule: { kind: 'file_type', value: 'tar.gz' } })).toBe(
      'Queue: Q. Its rules match .tar.gz files.',
    );
    expect(matched({ kind: 'matched', rule: { kind: 'category', value: 'video' } })).toBe(
      'Queue: Q. Its rules match videos.',
    );
    expect(matched({ kind: 'matched', rule: { kind: 'protocol', value: 'ftp' } })).toBe(
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
      { matched: { queue: null, rule: { kind: 'site', value: 'a.example' } }, switched_off: null },
      (queue) => (queue === null ? 'New' : 'Saved'),
      'Main',
    );
    expect(lines).toEqual(['Queue: New. Its rules match a.example.']);
  });
});
