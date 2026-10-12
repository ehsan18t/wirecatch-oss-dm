// What the person has picked in the Save as part of a quality window, as state the window and
// its Save as part both change.

import type { OutputFormat } from '@wirecatch/protocol';
import type { OutputChoice } from '../media-output';

export class OutputChoiceState implements OutputChoice {
  format = $state<OutputFormat>('original');
  kbps = $state<number | null>(null);
  subtitles = $state<string[]>([]);
  subtitleFiles = $state(false);
  chapters = $state(false);

  constructor(start: OutputChoice) {
    this.reset(start);
  }

  /** Starts again from `next`, as when switching between a video and audio only. */
  reset(next: OutputChoice): void {
    this.format = next.format;
    this.kbps = next.kbps;
    this.subtitles = next.subtitles;
    this.subtitleFiles = next.subtitleFiles;
    this.chapters = next.chapters;
  }
}
