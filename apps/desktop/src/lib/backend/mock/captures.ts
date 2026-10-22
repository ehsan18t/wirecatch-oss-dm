// Downloads the browser hands over, the questions they open, and the sample captures the mock pretends to receive.

import type {
  CaptureConfirmation,
  DownloadId,
  DownloadSummary,
  PendingCapture,
  Question,
  QuestionId,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { fileNameFromUrl } from '../../format';
import { BackendError } from '../types';
import { categoryFor, folderFor } from './data';
import type { MockState } from './state';

/** The core's answer for a question that is no longer open. */
export const ALREADY_ANSWERED =
  'That download was already confirmed or dismissed, or the browser took it back.';

/** Sample downloads the browser "sends", in turn. */
const CAPTURE_SAMPLES = [
  {
    url: 'https://github.com/example/app/releases/download/v3.2.0/app-setup-3.2.0.exe',
    size: 88 * MB,
  },
  { url: 'https://cdn.example-video.com/tutorials/color-grading-basics.mp4', size: 640 * MB },
  { url: 'https://papers.example.org/2026/attention-at-scale.pdf', size: 3 * MB },
  { url: 'https://downloads.example.com/drivers/gpu-driver-581.42.exe', size: 712 * MB },
] as const;

/** Closes question `id` everywhere it is shown. */
function closeQuestion(state: MockState, id: QuestionId) {
  state.emit({ type: 'confirmation_closed', data: { id } });
}

export function confirmCapture(
  state: MockState,
  id: QuestionId,
  confirmation: CaptureConfirmation,
): DownloadId {
  const p = state.pending.find((x) => x.id === id);
  if (!p) throw new BackendError(ALREADY_ANSWERED);
  state.pending = state.pending.filter((x) => x.id !== id);
  closeQuestion(state, id);
  const row = state.create(p.url, {
    queue: confirmation.queue ?? p.queue,
    folder: confirmation.folder ?? p.folder,
    fileName: confirmation.file_name ?? p.file_name,
    start: !confirmation.later,
    size: p.size,
  });
  state.changed([row]);
  return row.id;
}

export function rejectCapture(state: MockState, id: QuestionId): void {
  if (!state.pending.some((x) => x.id === id)) throw new BackendError(ALREADY_ANSWERED);
  state.pending = state.pending.filter((x) => x.id !== id);
  closeQuestion(state, id);
}

export function getQuestion(state: MockState, id: QuestionId): Question | null {
  const capture = state.pending.find((p) => p.id === id);
  if (capture) return { kind: 'capture', data: { ...capture } };
  const choice = state.mediaChoices.find((c) => c.id === id);
  if (choice) return { kind: 'choice', data: structuredClone(choice) };
  const page = state.waitingForAddon.find((w) => w.id === id);
  if (page) {
    return { kind: 'addon', data: { ...page, addon: structuredClone(state.addon('media')) } };
  }
  const reading = state.reading.find((r) => r.id === id);
  if (reading) return { kind: 'reading', data: { ...reading } };
  return null;
}

/** The newest download of the link question `id` asks about, if it was downloaded before. */
export function earlierDownload(state: MockState, id: QuestionId): DownloadSummary | null {
  const link =
    state.pending.find((p) => p.id === id)?.url ??
    state.mediaChoices.find((c) => c.id === id)?.page_url;
  if (!link) return null;
  let newest: DownloadSummary | undefined;
  for (const rowId of state.order) {
    const row = state.rows.get(rowId);
    if (row?.url === link && (!newest || row.added_at >= newest.added_at)) newest = row;
  }
  return newest ? { ...newest } : null;
}

/**
 * Sample data only: the oldest open question is answered somewhere else (another window, or
 * the browser took the download back), so the core closes it.
 */
export function simulateAnsweredElsewhere(state: MockState): void {
  const question = state.pending[0] ?? state.mediaChoices[0] ?? state.waitingForAddon[0];
  if (!question) return;
  state.pending = state.pending.filter((x) => x.id !== question.id);
  state.mediaChoices = state.mediaChoices.filter((x) => x.id !== question.id);
  state.waitingForAddon = state.waitingForAddon.filter((x) => x.id !== question.id);
  closeQuestion(state, question.id);
}

/** Sample data only: pretends the browser extension handed over the `index`th sample download. */
export function simulateCapture(state: MockState, index: number): void {
  const { url, size } = CAPTURE_SAMPLES[index % CAPTURE_SAMPLES.length] ?? CAPTURE_SAMPLES[0];
  const queue = state.settings.extension.default_queue;
  if (state.settings.extension.ask_before_download) {
    const name = fileNameFromUrl(url);
    const category = categoryFor(name, state.settings.categories);
    const pending: PendingCapture = {
      id: state.newQuestionId(),
      url,
      file_name: name,
      folder: folderFor(category, state.settings),
      category,
      size,
      queue,
      page_url: `https://${new URL(url).host}/`,
    };
    state.pending.push(pending);
    state.emit({ type: 'confirmation_requested', data: { ...pending } });
    return;
  }
  const row = state.create(url, { queue, start: true, size });
  state.changed([row]);
  state.emit({ type: 'captured', data: structuredClone(row) });
}
