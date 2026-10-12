// Sample media for the mock backend: the pages the browser "finds", and the qualities and formats each offers.

import type { MediaOption, MediaSubtitle, OutputFit } from '@wirecatch/protocol';
import { bytesAtBitRate, MB } from '@wirecatch/units';

/** A sample media page: its title, its address, and whether it is a page or a stream. */
export interface MediaSample {
  title: string;
  page: string;
  source: 'hls' | 'page';
}

/** Sample media pages, offered in turn. */
export const MEDIA_SAMPLES: readonly MediaSample[] = [
  {
    title: 'Northern lights over Tromso, 4K time-lapse',
    page: 'https://video.example.com/watch/aurora-tromso',
    source: 'page',
  },
  {
    title: 'Conference talk: fast downloads explained',
    page: 'https://talks.example.org/2026/fast-downloads',
    source: 'hls',
  },
];

/** How long the sample videos are: 12 minutes 34 seconds. */
export const SAMPLE_DURATION_MS = 754_000;

/** The subtitles a sample page offers; a stream offers none. */
export const SAMPLE_SUBTITLES: readonly MediaSubtitle[] = [
  { id: 'sub:en', language: 'en', name: 'English', auto: false },
  { id: 'sub:es', language: 'es', name: 'Spanish', auto: false },
  { id: 'auto:en-orig', language: 'en', name: 'English', auto: true },
];

/**
 * One "Save as" choice of a sample quality, as the core describes it (D24). An audio choice
 * without a known size gives `outputKbps`, the bitrate its size follows from.
 */
type SampleFit = Partial<OutputFit> & Pick<OutputFit, 'format' | 'ext'> & { outputKbps?: number };

/** A sample quality: the format it comes in, then the formats it can be saved as. */
interface SampleQuality {
  id: string;
  quality: string;
  height: number | null;
  needsFfmpeg: boolean;
  original: SampleFit;
  conversions: readonly SampleFit[];
}

const VIDEO_EXTRAS = { embeds_subtitles: true, chapters: true } as const;
const VP9 = { video_codec: 'VP9', audio_codec: 'Opus', audio_kbps: 160, ...VIDEO_EXTRAS } as const;
const H264 = {
  video_codec: 'H.264',
  audio_codec: 'AAC',
  audio_kbps: 128,
  ...VIDEO_EXTRAS,
} as const;
const AAC_AUDIO = {
  video: 'none',
  audio_codec: 'AAC',
  audio_kbps: 129,
  outputKbps: 129,
  chapters: true,
} as const;
const OPUS_AUDIO = { video: 'none', audio_codec: 'Opus', audio_kbps: 160 } as const;

/**
 * The sample qualities, best first: 4K only as VP9 (MP4 converts it), 1080p and 720p as H.264
 * (WebM converts them), and audio as AAC and Opus.
 */
const SAMPLE_QUALITIES: readonly SampleQuality[] = [
  {
    id: '2160',
    quality: '2160p',
    height: 2160,
    needsFfmpeg: true,
    original: { ...VP9, format: 'original', ext: 'webm', size: 1840 * MB },
    conversions: [
      {
        ...VP9,
        format: 'mp4',
        ext: 'mp4',
        video_codec: 'H.264',
        audio_codec: 'AAC',
        audio_kbps: 128,
        video: 'encode',
        size: 2600 * MB,
      },
      { ...VP9, format: 'mkv', ext: 'mkv', size: 1840 * MB },
      { ...VP9, format: 'webm', ext: 'webm', size: 1840 * MB },
    ],
  },
  {
    id: '1080',
    quality: '1080p',
    height: 1080,
    needsFfmpeg: true,
    original: { ...H264, format: 'original', ext: 'mp4', size: 612 * MB },
    conversions: [
      { ...H264, format: 'mp4', ext: 'mp4', size: 612 * MB },
      { ...H264, format: 'mkv', ext: 'mkv', size: 612 * MB },
      {
        ...H264,
        format: 'webm',
        ext: 'webm',
        video_codec: 'VP9',
        audio_codec: 'Opus',
        audio_kbps: 160,
        video: 'encode',
        size: 480 * MB,
      },
    ],
  },
  {
    id: '720',
    quality: '720p',
    height: 720,
    needsFfmpeg: false,
    original: { ...H264, format: 'original', ext: 'mp4', size: 298 * MB },
    conversions: [
      { ...H264, format: 'mp4', ext: 'mp4', size: 298 * MB },
      { ...H264, format: 'mkv', ext: 'mkv', size: 298 * MB },
      {
        ...H264,
        format: 'webm',
        ext: 'webm',
        video_codec: 'VP9',
        video: 'encode',
        audio: 'encode',
        size: 230 * MB,
      },
    ],
  },
  {
    id: 'audio',
    quality: 'Audio only',
    height: null,
    needsFfmpeg: false,
    original: { ...AAC_AUDIO, format: 'original', ext: 'm4a' },
    conversions: [
      { ...OPUS_AUDIO, format: 'mp3', ext: 'mp3', audio: 'encode', outputKbps: 192 },
      { ...AAC_AUDIO, format: 'm4a', ext: 'm4a' },
      { ...OPUS_AUDIO, format: 'opus', ext: 'opus', outputKbps: 160 },
      { ...OPUS_AUDIO, format: 'flac', ext: 'flac', audio: 'encode' },
      // Uncompressed: 48,000 samples a second, 16 bits each, two channels.
      { ...OPUS_AUDIO, format: 'wav', ext: 'wav', audio: 'encode', outputKbps: 1536 },
    ],
  },
];

/** Bytes of audio at `kbps` for the length of the sample videos. */
function audioBytes(kbps: number): number {
  return bytesAtBitRate(kbps, SAMPLE_DURATION_MS);
}

/** A sample fit as the core sends it, with everything it leaves out set to its default. */
function toFit({ format, ext, size, outputKbps, ...rest }: SampleFit): OutputFit {
  return {
    format,
    ext,
    video: 'copy',
    audio: 'copy',
    video_codec: null,
    audio_codec: null,
    audio_kbps: null,
    lossless_source: false,
    size: size ?? (outputKbps === undefined ? null : audioBytes(outputKbps)),
    embeds_subtitles: false,
    chapters: false,
    ...rest,
  };
}

function toOption(sample: SampleQuality, page: boolean): MediaOption {
  const original = toFit(sample.original);
  const { ext, size } = original;
  return {
    id: sample.id,
    label: `${sample.quality}, ${ext.toUpperCase()}`,
    quality: sample.quality,
    height: sample.height,
    audio_only: sample.height === null,
    size,
    needs_ffmpeg: sample.needsFfmpeg,
    // A stream offers only its own format.
    outputs: page
      ? [original, ...sample.conversions.map(toFit)]
      : [toFit({ format: 'original', ext, size })],
  };
}

/** The qualities a sample page (or, with `page` false, a stream) offers. */
export function sampleMediaOptions(page: boolean): MediaOption[] {
  return SAMPLE_QUALITIES.map((sample) => toOption(sample, page));
}
