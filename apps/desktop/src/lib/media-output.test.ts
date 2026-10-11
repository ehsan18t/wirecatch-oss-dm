import type { MediaChoice, MediaOption, OutputFit, OutputFormat } from '@wirecatch/protocol';
import { describe, expect, it } from 'vitest';
import {
  conversionNote,
  formatChoiceLabel,
  kbpsFor,
  nearestKbps,
  outputSelection,
  preselectSubtitles,
  qualityNotes,
  rowTag,
  sizeFor,
  startingChoice,
} from './media-output';

function fit(format: OutputFormat, rest: Partial<OutputFit> = {}): OutputFit {
  return {
    format,
    ext: format === 'original' ? 'webm' : format,
    converts_video: false,
    converts_audio: false,
    video_codec: 'VP9',
    audio_codec: 'Opus',
    audio_kbps: 160,
    lossless_source: false,
    size: 1000,
    embeds_subtitles: true,
    chapters: true,
    ...rest,
  };
}

const uhd: MediaOption = {
  id: '313+251',
  label: '2160p, WEBM',
  quality: '2160p',
  height: 2160,
  audio_only: false,
  size: 1000,
  needs_ffmpeg: true,
  outputs: [
    fit('original'),
    fit('mp4', { converts_video: true, video_codec: 'H.264', audio_codec: 'AAC', audio_kbps: 128 }),
    fit('mkv'),
    fit('webm'),
  ],
};

const audio: MediaOption = {
  id: '140',
  label: 'Audio only, M4A',
  quality: 'Audio only',
  height: null,
  audio_only: true,
  size: 100,
  needs_ffmpeg: false,
  outputs: [
    fit('original', { ext: 'm4a', video_codec: null, audio_codec: 'AAC', audio_kbps: 129 }),
    fit('mp3', {
      video_codec: null,
      converts_audio: true,
      embeds_subtitles: false,
      chapters: false,
    }),
    fit('m4a', { video_codec: null, audio_codec: 'AAC', audio_kbps: 129, embeds_subtitles: false }),
    fit('opus', { video_codec: null, embeds_subtitles: false, chapters: false }),
    fit('flac', {
      video_codec: null,
      converts_audio: true,
      embeds_subtitles: false,
      chapters: false,
    }),
  ],
};

/** What `option` offers as `format`. */
function out(option: MediaOption, format: OutputFormat): OutputFit {
  const found = option.outputs.find((f) => f.format === format);
  if (!found) throw new Error(`${option.id} does not offer ${format}`);
  return found;
}

const choice: MediaChoice = {
  id: 1,
  title: 'Cats',
  page_url: null,
  options: [uhd, audio],
  folder: 'C:/Videos',
  queue: 1,
  subtitles: [
    { id: 'sub:en-GB', language: 'en-GB', name: 'English', auto: false },
    { id: 'sub:fr', language: 'fr', name: 'French', auto: false },
    { id: 'auto:en-orig', language: 'en', name: 'English', auto: true },
    { id: 'auto:de-orig', language: 'de', name: 'German', auto: true },
  ],
  chapters: true,
  duration_ms: 60_000,
};

describe('the Save as part of a quality window (D24)', () => {
  it('names formats plainly', () => {
    expect(formatChoiceLabel(out(uhd, 'original'))).toBe('Original (WebM)');
    expect(formatChoiceLabel(out(audio, 'original'))).toBe('Original (M4A)');
    expect(formatChoiceLabel(out(uhd, 'mp4'))).toBe('MP4');
  });

  it('ticks the remembered languages, auto-generated ones only when wanted', () => {
    const offered = choice.subtitles;
    // `en` matches the English (United Kingdom) subtitles; German has only auto-generated ones.
    expect(preselectSubtitles(offered, ['en', 'de'], false)).toEqual(['sub:en-GB']);
    expect(preselectSubtitles(offered, ['en', 'de'], true)).toEqual(['sub:en-GB', 'auto:de-orig']);
    expect(preselectSubtitles(offered, ['xx'], true)).toEqual([]);
  });

  it('starts from the last choices for the kind, as far as they are offered', () => {
    const video = startingChoice(uhd, choice, {
      format: 'mp4',
      audio_kbps: 320,
      subtitle_languages: ['fr'],
      auto_subtitles: false,
      subtitle_files: true,
      chapters: true,
    });
    expect(video).toEqual({
      format: 'mp4',
      kbps: null,
      subtitles: ['sub:fr'],
      subtitleFiles: true,
      chapters: true,
    });
    // An audio format remembered for videos is not offered there: Original.
    expect(startingChoice(uhd, choice, { ...defaults(), format: 'mp3' }).format).toBe('original');
    // Nothing chosen before: Original, nothing ticked.
    expect(startingChoice(audio, choice, null)).toEqual({
      format: 'original',
      kbps: null,
      subtitles: [],
      subtitleFiles: false,
      chapters: false,
    });
    expect(
      startingChoice(audio, choice, { ...defaults(), format: 'mp3', audio_kbps: 300 }).kbps,
    ).toBe(320);
  });

  it('keeps the downloaded audio when it fits, else uses the wanted or usual quality', () => {
    const m4a = out(audio, 'm4a');
    const mp3 = out(audio, 'mp3');
    expect(kbpsFor(m4a, 'm4a', null)).toBeNull();
    expect(kbpsFor(m4a, 'm4a', 250)).toBe(256);
    expect(kbpsFor(mp3, 'mp3', null)).toBe(192);
    expect(kbpsFor(fit('flac'), 'flac', 320)).toBeNull();
    expect(nearestKbps([96, 128, 160, 192], 140)).toBe(128);
  });

  it('marks each row as converting or copying for the chosen format', () => {
    expect(rowTag(uhd, 'original', null)).toBeNull();
    expect(rowTag(uhd, 'mp4', null)).toBe('converts');
    expect(rowTag(uhd, 'mkv', null)).toBe('copied');
    expect(rowTag(audio, 'm4a', null)).toBe('copied');
    expect(rowTag(audio, 'm4a', 192)).toBe('converts');
    expect(rowTag(audio, 'mp4', null)).toBeNull();
  });

  it('says before the start whether it converts, in plain words', () => {
    expect(conversionNote(out(uhd, 'original'), 'original', null)).toBeNull();
    expect(conversionNote(out(uhd, 'mp4'), 'mp4', null)).toMatch(
      /no MP4 version, so Wirecatch converts the video to H\.264.*takes longer/,
    );
    expect(conversionNote(out(uhd, 'mkv'), 'mkv', null)).toMatch(
      /^No conversion: .* takes seconds\.$/,
    );
    expect(
      conversionNote(fit('mp4', { converts_audio: true, audio_codec: 'Opus' }), 'mp4', null),
    ).toBe(
      'The video is copied as it is; only the audio (Opus) is converted for MP4, which is quick.',
    );
    expect(conversionNote(out(audio, 'mp3'), 'mp3', 320)).toBe(
      'The audio is converted to MP3 at 320 kbps after it is downloaded, which is quick.',
    );
    expect(conversionNote(out(audio, 'm4a'), 'm4a', null)).toBe(
      'No conversion: the audio is copied into M4A as it is.',
    );
    for (const note of [
      conversionNote(out(uhd, 'mp4'), 'mp4', null),
      ...qualityNotes(out(audio, 'flac'), 'flac', null),
    ]) {
      expect(note).not.toContain(String.fromCharCode(0x2014));
    }
  });

  it('notes a bit rate above the download and a lossless format from lossy audio', () => {
    expect(qualityNotes(out(audio, 'mp3'), 'mp3', 320)).toEqual([
      '320 kbps is more than the 160 kbps of the download, so it does not add quality; the file only gets bigger.',
    ]);
    expect(qualityNotes(out(audio, 'mp3'), 'mp3', 128)).toEqual([]);
    expect(qualityNotes(out(audio, 'flac'), 'flac', null)[0]).toMatch(
      /^The download's audio is Opus, which already left out some detail\. FLAC keeps/,
    );
    expect(qualityNotes(fit('flac', { lossless_source: true }), 'flac', null)).toEqual([]);
  });

  it('builds the selection the core takes, leaving out what is not offered', () => {
    const picked = {
      format: 'mp4' as const,
      kbps: 320,
      subtitles: ['sub:fr', 'sub:gone'],
      subtitleFiles: true,
      chapters: true,
    };
    expect(outputSelection(uhd, choice, picked)).toEqual({
      output: 'mp4',
      audio_kbps: null,
      subtitles: ['sub:fr'],
      subtitle_files: true,
      chapters: true,
    });
    expect(outputSelection(audio, choice, { ...picked, format: 'mp3' })).toMatchObject({
      output: 'mp3',
      audio_kbps: 320,
      chapters: false,
    });
    // A format the quality does not offer falls back to Original.
    expect(outputSelection(audio, choice, picked).output).toBe('original');
  });

  it('estimates the size for the audio quality picked', () => {
    expect(sizeFor(audio, 'mp3', 320, 60_000)).toBe(2_400_000);
    expect(sizeFor(uhd, 'mp4', null, 60_000)).toBe(1000);
  });
});

function defaults() {
  return {
    format: 'original' as OutputFormat,
    audio_kbps: null,
    subtitle_languages: [],
    auto_subtitles: false,
    subtitle_files: false,
    chapters: false,
  };
}
