// The sections of the settings screen, in the order it shows them.

export type SectionId =
  | 'general'
  | 'notifications'
  | 'files'
  | 'browser'
  | 'media'
  | 'addons'
  | 'speed'
  | 'queues'
  | 'network'
  | 'transfer'
  | 'advanced';

export const SECTIONS: readonly { id: SectionId; title: string; blurb: string }[] = [
  { id: 'general', title: 'General', blurb: 'How Wirecatch looks and starts.' },
  {
    id: 'notifications',
    title: 'Notifications',
    blurb: 'How Wirecatch lets you know a download started, finished or failed.',
  },
  { id: 'files', title: 'Saving files', blurb: 'Where finished files go.' },
  {
    id: 'browser',
    title: 'Browser',
    blurb: 'What the browser extension captures, and when it asks first.',
  },
  {
    id: 'media',
    title: 'Media',
    blurb: 'Video quality, formats, subtitles and audio.',
  },
  {
    id: 'addons',
    title: 'Add-ons',
    blurb: 'Optional extras, installed only when you ask.',
  },
  {
    id: 'speed',
    title: 'Speed and limits',
    blurb: 'Limit speed and the number of downloads so the rest of your computer stays responsive.',
  },
  {
    id: 'queues',
    title: 'Queues',
    blurb: 'Each queue can have its own schedule and limits; otherwise it uses the global ones.',
  },
  {
    id: 'network',
    title: 'Network',
    blurb: 'How Wirecatch reaches the internet.',
  },
  {
    id: 'transfer',
    title: 'Import and export',
    blurb: 'Save your settings to a file, or load them from one, for example on another computer.',
  },
  {
    id: 'advanced',
    title: 'Advanced',
    blurb: 'Rarely needed. The defaults suit almost everyone.',
  },
];
