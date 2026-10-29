// The sections of the settings screen, in the order it shows them (D85).

export type SectionId =
  'general' | 'notifications' | 'files' | 'browser' | 'media' | 'speed' | 'network';

export const SECTIONS: readonly { id: SectionId; title: string; blurb: string }[] = [
  { id: 'general', title: 'General', blurb: 'Appearance, startup, updates, backup and logs.' },
  {
    id: 'notifications',
    title: 'Notifications',
    blurb: 'When Wirecatch tells you about a download.',
  },
  { id: 'files', title: 'Saving files', blurb: 'Where finished files go.' },
  { id: 'browser', title: 'Browser', blurb: 'What the extension captures.' },
  { id: 'media', title: 'Media', blurb: 'The media add-on, video and audio.' },
  { id: 'speed', title: 'Speed and queues', blurb: 'Limits, and the queues that use them.' },
  { id: 'network', title: 'Network', blurb: 'Proxies and your local network.' },
];
