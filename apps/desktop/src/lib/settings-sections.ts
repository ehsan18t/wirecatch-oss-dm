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
    blurb: 'How Wirecatch tells you a download started, finished or failed.',
  },
  { id: 'files', title: 'Saving files', blurb: 'Where finished files go.' },
  {
    id: 'browser',
    title: 'Browser',
    blurb: 'What the browser extension catches, and how it asks.',
  },
  {
    id: 'media',
    title: 'Media',
    blurb: 'Whether videos ask for a quality, and what they start with.',
  },
  {
    id: 'addons',
    title: 'Add-ons',
    blurb: 'Optional extras, installed only when you ask.',
  },
  {
    id: 'speed',
    title: 'Speed and limits',
    blurb: 'How fast, and how many at once, so the rest of your computer stays quick.',
  },
  {
    id: 'queues',
    title: 'Queues',
    blurb: "Each queue has its own hours, and uses the app's limits unless you set its own.",
  },
  {
    id: 'network',
    title: 'Network',
    blurb: 'How Wirecatch reaches the internet.',
  },
  {
    id: 'transfer',
    title: 'Move to another PC',
    blurb: 'Save your settings to a file and load them on another computer.',
  },
  {
    id: 'advanced',
    title: 'Advanced',
    blurb: 'Rarely needed. The defaults suit almost everyone.',
  },
];
