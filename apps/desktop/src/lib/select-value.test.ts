import { AUDIO_KBPS_CHOICES, type OutputFormat } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import { offeredKbps, offeredValue } from './select-value';

const FORMATS: readonly OutputFormat[] = ['original', 'mp4', 'mkv'];
const MP3_KBPS = AUDIO_KBPS_CHOICES.mp3 ?? [];

describe('reading a menu value back', () => {
  it('gives the offered option the text names', () => {
    expect(offeredValue(FORMATS, 'mkv')).toBe('mkv');
    expect(offeredValue([1, 7, 12], '7')).toBe(7);
  });

  it('refuses text that names no offered option', () => {
    expect(offeredValue(FORMATS, 'mp3')).toBeUndefined();
    expect(offeredValue(FORMATS, 'MP4')).toBeUndefined();
    expect(offeredValue([1, 7, 12], '7.0')).toBeUndefined();
    expect(offeredValue([1, 7, 12], '')).toBeUndefined();
    expect(offeredValue([1, 7, 12], 'NaN')).toBeUndefined();
  });

  it('reads an audio quality as one of the offered bit rates or the usual one', () => {
    const offered = MP3_KBPS.at(-1);
    expect(offered).toBeDefined();
    expect(offeredKbps(MP3_KBPS, String(offered), { usual: true })).toBe(offered);
    expect(offeredKbps(MP3_KBPS, '', { usual: true })).toBeNull();
    expect(offeredKbps(MP3_KBPS, '100000', { usual: true })).toBeUndefined();
  });

  it('refuses the usual quality where the menu does not offer it', () => {
    expect(offeredKbps(MP3_KBPS, '', { usual: false })).toBeUndefined();
    expect(offeredKbps(MP3_KBPS, String(MP3_KBPS[0]), { usual: false })).toBe(MP3_KBPS[0]);
  });
});
