import { describe, expect, it } from 'vitest';
import {
  kbpsChoices,
  kbpsForFormat,
  languageChoices,
  nearestKbps,
  qualityChoices,
  qualityValue,
  videoQuality,
} from './media-choices';

describe('media choices', () => {
  it('offers the usual resolutions, and a saved one that is not among them in its place', () => {
    expect(qualityChoices('best').map((c) => c.label)).toEqual([
      'Best available',
      '2160p',
      '1440p',
      '1080p',
      '720p',
      '480p',
      '360p',
      'Audio only',
    ]);
    expect(qualityChoices({ height: 900 }).map((c) => c.value)).toContain(900);
    for (const quality of ['best', 'audio_only', { height: 720 }] as const) {
      expect(videoQuality(qualityValue(quality))).toEqual(quality);
    }
  });

  it('offers the audio qualities of a format after its usual one, and none for a lossless one', () => {
    expect(kbpsChoices('mp3')[0]).toEqual({ value: 'usual', label: 'Usual quality' });
    expect(kbpsChoices('mp3').length).toBeGreaterThan(1);
    expect(kbpsChoices('flac')).toEqual([]);
    expect(nearestKbps([96, 128, 160, 192], 140)).toBe(128);
    expect(kbpsForFormat('flac', 320)).toBeNull();
    expect(kbpsForFormat('mp3', null)).toBeNull();
  });

  it('names every language with its code, finds old names, and keeps codes saved earlier', () => {
    const choices = languageChoices(['pt-br']);
    expect(choices.find((c) => c.value === 'bn')).toMatchObject({ keywords: 'Bengali' });
    expect(choices.find((c) => c.value === 'en')?.label).toBe('English (en)');
    expect(choices.some((c) => c.value === 'pt-br')).toBe(true);
    // A code the system cannot name is left out, unless it was saved.
    const kept = languageChoices(['qaa']);
    const unnamed = kept.filter((c) => c.label === `${c.value} (${c.value})`);
    expect(unnamed.map((c) => c.value)).toEqual(['qaa']);
    expect(choices.map((c) => c.label)).toEqual(
      [...choices.map((c) => c.label)].sort((a, b) => a.localeCompare(b)),
    );
  });
});
