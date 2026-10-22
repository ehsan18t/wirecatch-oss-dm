import { describe, expect, it } from 'vitest';
import fixtures from './generated/fixtures.json';
import { isMediaDefaults, isMediaOption, isMediaSubtitle } from './generated/validators';
import { mediaStart } from './media-start';

describe('mediaStart', () => {
  it('starts every video where the app starts it', () => {
    expect(fixtures.media_start.length).toBeGreaterThan(0);
    for (const fixture of fixtures.media_start) {
      const { defaults, chapters, start } = fixture;
      // Read as unknown, so each guard says what it checked.
      const offered: readonly unknown[] = fixture.options;
      const named: readonly unknown[] = fixture.subtitles;
      const options = offered.filter(isMediaOption);
      const subtitles = named.filter(isMediaSubtitle);
      if (
        !isMediaDefaults(defaults) ||
        options.length !== fixture.options.length ||
        subtitles.length !== fixture.subtitles.length
      ) {
        throw new Error(`A fixture is not saved choices and a video: ${JSON.stringify(defaults)}`);
      }
      const video = `${JSON.stringify(defaults)} with ${options.map((o) => o.id).join(', ')}`;
      expect(mediaStart(defaults, options, subtitles, chapters), video).toEqual(start);
    }
  });
});
