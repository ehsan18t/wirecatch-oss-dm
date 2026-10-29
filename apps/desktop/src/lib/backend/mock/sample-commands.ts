// The palette actions of the sample data: each pretends the browser or another window did something.

import type { SampleData } from '../types';
import type { MockFinished } from './finished';

/** What the mock can pretend happened. */
export interface Scenarios {
  simulateCapture: () => void;
  simulateConfirmation: () => void;
  simulateMediaChoice: () => void;
  simulateAddonRequired: () => void;
  simulateAnsweredElsewhere: () => void;
  simulateSlowReading: () => void;
  simulateFileExists: () => void;
}

export function sampleCommands(mock: Scenarios, finished: MockFinished): SampleData['commands'] {
  return [
    {
      id: 'mock.capture',
      title: 'Pretend the browser sent a download',
      keywords: 'simulate capture',
      run: () => mock.simulateCapture(),
    },
    {
      id: 'mock.confirm',
      title: 'Pretend the browser sent a download that asks first',
      keywords: 'simulate confirmation',
      run: () => mock.simulateConfirmation(),
    },
    {
      id: 'mock.media',
      title: 'Pretend the browser found a video with several qualities',
      keywords: 'simulate media quality picker stream youtube',
      run: () => mock.simulateMediaChoice(),
    },
    {
      id: 'mock.addon',
      title: 'Pretend a site needs the media add-on',
      keywords: 'simulate addon add-on required youtube yt-dlp',
      run: () => mock.simulateAddonRequired(),
    },
    {
      id: 'mock.elsewhere',
      title: 'Pretend the oldest question was answered somewhere else',
      keywords: 'simulate confirmation closed withdrawn another window',
      run: () => mock.simulateAnsweredElsewhere(),
    },
    {
      id: 'mock.reading',
      title: 'Pretend the browser sent a video page that takes a while to read',
      keywords: 'simulate reading analysing page media',
      run: () => mock.simulateSlowReading(),
    },
    {
      id: 'mock.file-exists',
      title: 'Pretend a download found its file already in the folder',
      keywords: 'simulate duplicate file exists replace keep both',
      run: () => mock.simulateFileExists(),
    },
    {
      id: 'mock.finished',
      title: 'Pretend a download finished, told in the download complete window',
      keywords: 'simulate finished complete window done notify',
      run: () => finished.sampleOne(),
    },
    {
      id: 'mock.queue-finished',
      title: 'Pretend a queue is done, told in the download complete window',
      keywords: 'simulate queue finished complete window done summary notify',
      run: () => finished.sampleQueueRun(),
    },
  ];
}
