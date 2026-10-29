// What a download does when its file is already in the folder (D88), in the words Settings uses.

import type { WhenFileExists } from '@wirecatch/protocol';
import type { SelectOption } from '@wirecatch/ui';

export const WHEN_FILE_EXISTS_OPTIONS: readonly SelectOption<WhenFileExists>[] = [
  {
    value: 'ask',
    label: 'Ask me',
    description: 'Each time, with a choice to remember the answer.',
  },
  {
    value: 'replace',
    label: 'Replace the file',
    description: 'Downloads it again over the file in the folder.',
  },
  {
    value: 'keep_existing',
    label: 'Keep the existing file',
    description: 'Downloads nothing. The download shows as finished, with the file already there.',
  },
  {
    value: 'keep_both',
    label: 'Keep both',
    description: 'Saves the new one under a new name, like report (1).pdf.',
  },
];
