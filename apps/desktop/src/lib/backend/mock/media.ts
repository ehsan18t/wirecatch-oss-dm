// Media pages: the qualities the browser finds, the choice the person makes, and the add-on that choice may need.

import type {
  AddonRequestItem,
  DownloadId,
  MediaChoice,
  MediaOption,
  MediaSelection,
  OutputFit,
  ProcessingStep,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { BackendError } from '../types';
import { ALREADY_ANSWERED } from './captures';
import { folderFor } from './data';
import {
  MEDIA_SAMPLES,
  SAMPLE_DURATION_MS,
  SAMPLE_SUBTITLES,
  sampleMediaOptions,
} from './media-samples';
import { nonBlank, type MockState } from './state';

/** The core's answer for a quality that needs the media add-on while it is missing. */
export const ADDON_REQUIRED =
  'This needs the media add-on, which joins video and audio and reads video pages. Install it from Settings, Add-ons (it is offered right away), then try again.';

/** A sample page that needs the media add-on, and one that takes a while to read. */
export const ADDON_SAMPLE_PAGE = 'https://www.youtube.com/watch?v=sample';
export const SLOW_SAMPLE_PAGE = 'https://video.example.com/watch/slow-page';

/** The size of a sample quality that does not say. */
const MEDIA_FALLBACK_SIZE = 48 * MB;
/** Sample media finishes downloading in this many seconds, so the joining step is quick to see. */
const MEDIA_DOWNLOAD_SECONDS = 8;
/** Output formats that keep the audio bitrate the person picked. */
const BITRATE_FORMATS: readonly string[] = ['mp3', 'm4a', 'opus'];

/**
 * Sample data only: the browser found the `index`th sample media with several qualities. A page
 * that was being read (or waited for the add-on) passes its own address and id.
 */
export function simulateMediaChoice(
  state: MockState,
  index: number,
  page?: string,
  id?: number,
): void {
  const sample = MEDIA_SAMPLES[index % MEDIA_SAMPLES.length] ?? MEDIA_SAMPLES[0];
  if (!sample) return;
  const isPage = sample.source === 'page';
  const choice: MediaChoice = {
    id: id ?? state.nextPendingId++,
    title: sample.title,
    page_url: page ?? sample.page,
    options: sampleMediaOptions(isPage),
    folder: folderFor('video', state.settings),
    queue: state.settings.extension.default_queue,
    subtitles: isPage ? SAMPLE_SUBTITLES.map((s) => ({ ...s })) : [],
    chapters: isPage,
    duration_ms: SAMPLE_DURATION_MS,
  };
  // Like the core: the reading is announced, then the choice comes under the same id.
  state.emit({ type: 'media_analysing', data: { id: choice.id, url: choice.page_url ?? '' } });
  state.reading = state.reading.filter((r) => r.id !== choice.id);
  state.mediaChoices.push(choice);
  state.emit({ type: 'media_choice_requested', data: structuredClone(choice) });
}

/** Sample data only: a page starts being read; returns its id. */
export function startReading(state: MockState, page: string): number {
  const id = state.nextPendingId++;
  state.reading.push({ id, url: page });
  state.emit({ type: 'media_analysing', data: { id, url: page } });
  return id;
}

/** Sample data only: a page needs the media add-on, which is missing. */
export function askForAddon(state: MockState, page: string): void {
  const id = state.nextPendingId++;
  state.waitingForAddon.push({ id, url: page });
  const addon = structuredClone(state.addon('media'));
  state.emit({ type: 'addon_required', data: { addon, url: page, id } });
}

export function pendingAddonRequests(state: MockState): AddonRequestItem[] {
  const addon = state.addon('media');
  return state.waitingForAddon.map((w) => ({ ...w, addon: structuredClone(addon) }));
}

function openChoice(state: MockState, id: number): MediaChoice {
  const choice = state.mediaChoices.find((c) => c.id === id);
  if (!choice) throw new BackendError(ALREADY_ANSWERED);
  return choice;
}

function offeredOption(choice: MediaChoice, selection: MediaSelection): MediaOption {
  const option = choice.options.find((o) => o.id === selection.option_id);
  if (!option) {
    throw new BackendError(
      'That quality is not offered for this video. Choose one of the listed options.',
    );
  }
  return option;
}

function offeredFit(option: MediaOption, selection: MediaSelection): OutputFit {
  const fit = option.outputs.find((f) => f.format === selection.output);
  if (!fit) {
    throw new BackendError(
      'This quality cannot be saved in that format. Choose one of the formats listed for it.',
    );
  }
  return fit;
}

/** Like the core: without the add-on, ask for it and keep the choice open. */
function requireAddon(state: MockState, choice: MediaChoice): void {
  if (state.mediaReady()) return;
  const url = choice.page_url ?? choice.title;
  const addon = structuredClone(state.addon('media'));
  state.emit({ type: 'addon_required', data: { addon, url, id: choice.id } });
  throw new BackendError(ADDON_REQUIRED, 'addon_required');
}

/** What ffmpeg does once the download is in, besides joining video and audio. */
function processingStep(
  fit: OutputFit,
  selection: MediaSelection,
  extras: boolean,
): ProcessingStep | null {
  if (fit.video === 'encode' || fit.audio === 'encode' || selection.audio_kbps !== null) {
    return 'converting';
  }
  return selection.output !== 'original' || extras ? 'finishing' : null;
}

/** What the person picked from a media choice, checked against what it offers. */
interface Picked {
  choice: MediaChoice;
  option: MediaOption;
  fit: OutputFit;
  selection: MediaSelection;
}

export function chooseMedia(state: MockState, id: number, selection: MediaSelection): DownloadId {
  const choice = openChoice(state, id);
  const option = offeredOption(choice, selection);
  const picked: Picked = { choice, option, fit: offeredFit(option, selection), selection };
  const extras = selection.subtitles.length > 0 || (selection.chapters && choice.chapters);
  if (option.needs_ffmpeg || selection.output !== 'original' || extras) {
    requireAddon(state, choice);
  }
  state.mediaChoices = state.mediaChoices.filter((c) => c.id !== id);
  state.emit({ type: 'confirmation_closed', data: { id } });
  if (option.outputs.length > 1) rememberMediaChoice(state, picked);
  const row = addMediaDownload(state, picked);
  const sim = state.sim.get(row.id);
  if (sim) {
    sim.needsJoin = option.needs_ffmpeg;
    sim.step = processingStep(picked.fit, selection, extras);
    sim.rate = (row.total ?? MEDIA_FALLBACK_SIZE) / MEDIA_DOWNLOAD_SECONDS;
  }
  state.changed([row]);
  return row.id;
}

function addMediaDownload(state: MockState, { choice, option, fit, selection }: Picked) {
  const row = state.create(choice.page_url ?? choice.title, {
    queue: selection.queue ?? choice.queue,
    folder: selection.folder ?? choice.folder,
    fileName: nonBlank(selection.file_name) ?? `${choice.title}.${fit.ext}`,
    start: !selection.later,
    size: option.size ?? MEDIA_FALLBACK_SIZE,
  });
  row.media = {
    quality: option.label.split(',')[0] ?? option.label,
    source: MEDIA_SAMPLES.find((s) => s.title === choice.title)?.source ?? 'page',
    video_key: null,
  };
  return row;
}

/** Like the core: the next window for this kind starts from these choices (D24). */
function rememberMediaChoice(state: MockState, { choice, option, selection }: Picked) {
  const kind = option.audio_only ? 'audio' : 'video';
  const remembered = state.settings.media[kind];
  remembered.format = selection.output;
  if (BITRATE_FORMATS.includes(selection.output)) {
    remembered.audio_kbps = selection.audio_kbps;
  }
  if (choice.subtitles.length > 0) {
    const picked = choice.subtitles.filter((s) => selection.subtitles.includes(s.id));
    remembered.subtitle_languages = [...new Set(picked.map((s) => s.language.toLowerCase()))];
    if (picked.some((s) => s.auto)) remembered.auto_subtitles = true;
    else if (choice.subtitles.some((s) => s.auto)) remembered.auto_subtitles = false;
    if (kind === 'video') remembered.subtitle_files = selection.subtitle_files;
  }
  if (choice.chapters) remembered.chapters = selection.chapters;
  state.settingsChanged();
}

export function dismissMediaChoice(state: MockState, id: number): void {
  // Like the core, this drops a quality choice or a page still waiting for the add-on.
  const known =
    state.mediaChoices.some((c) => c.id === id) ||
    state.waitingForAddon.some((w) => w.id === id) ||
    state.reading.some((r) => r.id === id);
  state.reading = state.reading.filter((r) => r.id !== id);
  if (!known) throw new BackendError(ALREADY_ANSWERED);
  state.mediaChoices = state.mediaChoices.filter((c) => c.id !== id);
  state.waitingForAddon = state.waitingForAddon.filter((w) => w.id !== id);
  state.emit({ type: 'confirmation_closed', data: { id } });
}
