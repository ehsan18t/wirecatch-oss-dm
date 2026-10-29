// Where a video's quality window starts, and whether it opens at all (D50): a port of `MediaDefaults::start` in crates/protocol/src/media_start.rs, for the desktop's sample backend, tested against the cases written from there (`generated/fixtures.json`), so both languages agree.

import type { MediaDefaults } from './generated/MediaDefaults';
import type { MediaOption } from './generated/MediaOption';
import type { MediaSelection } from './generated/MediaSelection';
import type { MediaSubtitle } from './generated/MediaSubtitle';
import type { OutputDefaults } from './generated/OutputDefaults';
import type { OutputFormat } from './generated/OutputFormat';
import type { StreamAction } from './generated/StreamAction';
import { AUDIO_DEFAULT_KBPS, AUDIO_KBPS_CHOICES } from './generated/constants';

/** Where a video's quality window starts, and whether it must open. */
export interface MediaStart {
  /** The quality picked, saved as the saved choices say. */
  selection: MediaSelection;
  /** The saved resolution, when the video does not offer it ("1080p"). */
  missing: string | null;
  /** The window must open: the person asks for each video, or the video does not offer the saved resolution and the fallback is to ask. */
  asks: boolean;
}

/** Where the quality window starts for a video offering `options` (best first), `subtitles`, and chapters when `chapters`. */
export function mediaStart(
  saved: MediaDefaults,
  options: readonly MediaOption[],
  subtitles: readonly MediaSubtitle[],
  chapters: boolean,
): MediaStart {
  const [index, missing] = pick(saved, options);
  const option = options[index];
  const selection = option
    ? selectionFor(option.audio_only ? saved.audio : saved.video, option, subtitles, chapters)
    : selectionOf(saved.video, '', [], false);
  const asks = saved.ask_each_time || (missing !== null && saved.fallback === 'ask');
  return { selection, missing, asks };
}

/** The option the saved resolution picks, as its place in `options`, and the saved resolution when the video does not offer it. */
function pick(saved: MediaDefaults, options: readonly MediaOption[]): [number, string | null] {
  const quality = saved.quality;
  if (quality === 'best') return [0, null];
  if (quality === 'audio_only') {
    const index = options.findIndex((o) => o.audio_only);
    return index === -1 ? [0, 'Audio only'] : [index, null];
  }
  const wanted = quality.height;
  const videos = options.flatMap((o, i): [number, number][] =>
    !o.audio_only && o.height !== null ? [[i, o.height]] : [],
  );
  if (videos.length === 0) return [0, null];
  const exact = videos.find(([, h]) => h === wanted);
  if (exact) return [exact[0], null];
  const lowest = minBy(videos);
  const tallest = maxBy(videos);
  const picked =
    saved.fallback === 'higher'
      ? (minBy(videos.filter(([, h]) => h > wanted)) ?? tallest)
      : (maxBy(videos.filter(([, h]) => h < wanted)) ?? lowest);
  return [picked?.[0] ?? 0, `${String(wanted)}p`];
}

/** The first video of the least height, as Rust's `min_by_key` keeps it. */
function minBy(videos: readonly [number, number][]): [number, number] | undefined {
  return videos.reduce<[number, number] | undefined>(
    (m, v) => (!m || v[1] < m[1] ? v : m),
    undefined,
  );
}

/** The last video of the greatest height, as Rust's `max_by_key` keeps it. */
function maxBy(videos: readonly [number, number][]): [number, number] | undefined {
  return videos.reduce<[number, number] | undefined>(
    (m, v) => (!m || v[1] >= m[1] ? v : m),
    undefined,
  );
}

/** `option` saved as these choices say, as far as it and the video offer them. */
function selectionFor(
  last: OutputDefaults,
  option: MediaOption,
  subtitles: readonly MediaSubtitle[],
  chapters: boolean,
): MediaSelection {
  const fit = option.outputs.find((f) => f.format === last.format);
  const format: OutputFormat = fit ? fit.format : 'original';
  const audio = (fit ?? option.outputs.find((f) => f.format === format))?.audio ?? null;
  return {
    ...selectionOf(last, option.id, subtitles, chapters),
    output: format,
    audio_kbps: audioKbps(format, audio, last.audio_kbps),
  };
}

function selectionOf(
  last: OutputDefaults,
  optionId: string,
  subtitles: readonly MediaSubtitle[],
  chapters: boolean,
): MediaSelection {
  return {
    option_id: optionId,
    folder: null,
    file_name: null,
    queue: null,
    later: false,
    output: 'original',
    audio_kbps: null,
    subtitles: preselectedSubtitles(subtitles, last.subtitle_languages, last.auto_subtitles),
    subtitle_files: last.subtitle_files,
    unembedded_subtitles: last.unembedded_subtitles,
    chapters: chapters && last.chapters,
    every_video: null,
  };
}

/** The audio quality to start from for `format`, whose audio is `audio`: none for a format without a quality to pick; the offered quality nearest the saved one; none when the downloaded audio fits as it is; otherwise the format's usual quality. */
function audioKbps(
  format: OutputFormat,
  audio: StreamAction | null,
  saved: number | null,
): number | null {
  const choices = AUDIO_KBPS_CHOICES[format];
  if (!choices || choices.length === 0) return null;
  if (saved !== null) {
    let best = choices[0] ?? saved;
    for (const c of choices) if (Math.abs(c - saved) < Math.abs(best - saved)) best = c;
    return best;
  }
  if (audio !== null && audio !== 'encode') return null;
  return AUDIO_DEFAULT_KBPS[format] ?? choices[0] ?? null;
}

/** The subtitles to pick for the saved `languages`, each once: for each language its subtitles written by people, or, when `auto` and it has none, its auto-generated ones. A language the video does not have is skipped, never replaced by another. */
export function preselectedSubtitles(
  offered: readonly MediaSubtitle[],
  languages: readonly string[],
  auto: boolean,
): string[] {
  const picked: string[] = [];
  for (const language of languages) {
    const ofLanguage = offered.filter((s) => sameLanguage(s.language, language));
    const found = ofLanguage.find((s) => !s.auto) ?? (auto ? ofLanguage.find((s) => s.auto) : null);
    if (found && !picked.includes(found.id)) picked.push(found.id);
  }
  return picked;
}

/** Whether `language` (a site's code, `en-GB`) is the language `wanted` (`en`, `en-gb`). */
function sameLanguage(language: string, wanted: string): boolean {
  const have = language.toLowerCase();
  const want = wanted.toLowerCase();
  return have === want || have.split(/[-_]/)[0] === want;
}
