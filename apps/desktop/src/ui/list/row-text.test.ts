import { describe, expect, it } from 'vitest';
import { failureShort, finishedWhen } from './row-text';

describe('the line under a row', () => {
  it('says what went wrong in its first sentence, leaving what to do to the details', () => {
    expect(failureShort('The download link expired. Get a new link from the page.')).toBe(
      'The download link expired.',
    );
    // A dot inside a name or an address does not end the sentence.
    expect(failureShort('Drive C: is full. Free up space on drive C.')).toBe('Drive C: is full.');
    expect(failureShort('Could not save to the folder chosen')).toBe(
      'Could not save to the folder chosen',
    );
  });

  it('says when a finished download finished, without saying Finished again', () => {
    const now = new Date(2026, 9, 22, 12, 0);
    const at = (d: Date) => ({ completed_at: d.getTime() });
    expect(finishedWhen(at(new Date(2026, 9, 22, 9, 5)), now)).toBe('Today at 09:05');
    expect(finishedWhen(at(new Date(2026, 9, 21, 23, 12)), now)).toBe('Yesterday at 23:12');
    expect(finishedWhen(at(new Date(2026, 8, 3, 9, 0)), now)).toBe('3 Sep 2026');
    expect(finishedWhen({ completed_at: null }, now)).toBe('Finished');
  });
});
