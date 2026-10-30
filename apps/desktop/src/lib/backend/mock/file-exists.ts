// What the mock core does with a download whose file is already in its folder (D88).

import type {
  DownloadId,
  FileExistsAnswer,
  FileExistsQuestion,
  QuestionId,
} from '@wirecatch/protocol';
import { MB } from '@wirecatch/units';
import { BackendError } from '../types';
import { cancel, resume } from './downloads';
import type { MockState } from './state';

/** The core's answer for a question that is no longer open. */
export const ALREADY_ANSWERED =
  'That question was already answered, or its download was resumed, cancelled or removed.';

/** The sample download whose file is already in its folder. */
const SAMPLE = {
  url: 'https://papers.example.org/2026/attention-at-scale.pdf',
  size: 3 * MB,
} as const;

const DAY_MS = 24 * 60 * 60 * 1000;

/** Sample data only: a download pauses because its file is already in its folder; returns the question's id. */
export function simulateFileExists(state: MockState, sameFile = true): QuestionId {
  const row = state.create(SAMPLE.url, { start: false, size: SAMPLE.size });
  state.changed([row]);
  const question: FileExistsQuestion = {
    id: state.newQuestionId(),
    download: row.id,
    url: row.url,
    file_name: row.file_name,
    folder: row.path.slice(0, row.path.length - row.file_name.length - 1),
    size: SAMPLE.size,
    same_file: sameFile,
    saved_at: state.now() - DAY_MS,
  };
  state.fileExists.push(question);
  state.emit({ type: 'file_exists_requested', data: structuredClone(question) });
  return question.id;
}

export function answerFileExists(
  state: MockState,
  id: QuestionId,
  answer: FileExistsAnswer,
): DownloadId {
  const question = state.fileExists.find((q) => q.id === id);
  if (!question) throw new BackendError(ALREADY_ANSWERED);
  state.fileExists = state.fileExists.filter((q) => q.id !== id);
  state.emit({ type: 'confirmation_closed', data: { id } });
  const row = state.row(question.download);
  if (answer.choice === 'cancel') {
    cancel(state, row.id);
    return row.id;
  }
  if (answer.choice === 'keep_both') {
    const dot = row.file_name.lastIndexOf('.');
    const [stem, ext] =
      dot > 0 ? [row.file_name.slice(0, dot), row.file_name.slice(dot)] : [row.file_name, ''];
    const renamed = `${stem} (1)${ext}`;
    row.path = `${row.path.slice(0, row.path.length - row.file_name.length)}${renamed}`;
    row.file_name = renamed;
  }
  if (answer.remember) {
    state.settings.when_file_exists = answer.choice;
    state.settingsChanged();
  }
  resume(state, row.id);
  return row.id;
}
