// Every setting the extension's pages show, with the words a search finds it by. The popup and
// the options page take a setting's name and description from here, so the two always say the same.
import type { Searchable } from '@wirecatch/ui';

export const SETTINGS = {
  captureEnabled: {
    label: 'Capture downloads',
    description: 'Send downloads from this browser to Wirecatch.',
    keywords: ['intercept', 'take over', 'on', 'off'],
  },
  askBeforeDownload: {
    label: 'Show a window before each download',
    description: 'Choose where each file goes before it starts.',
    keywords: ['ask', 'confirm', 'folder', 'save', 'where'],
  },
  defaultQueue: {
    label: 'Default queue',
    description: 'Downloads from the browser join this queue.',
    keywords: ['schedule', 'order'],
  },
  fileTypes: {
    label: 'File types to capture',
    description: 'For example: zip, exe, mp4. Leave empty for every type.',
    keywords: ['extensions', 'filter', 'kind', 'format'],
  },
  minSize: {
    label: 'Minimum file size',
    description: 'Smaller files stay in the browser. 0 means every size.',
    keywords: ['small', 'bytes', 'KB', 'MB', 'GB', 'filter'],
  },
  ignoredSites: {
    label: 'Sites to ignore',
    description: 'Downloads from these sites (and their subdomains) stay in the browser.',
    keywords: ['block', 'exclude', 'domain', 'website', 'host'],
  },
  mediaButton: {
    label: 'Show download button on videos',
    description: 'Adds a button to video and audio on web pages.',
    keywords: ['media', 'audio', 'stream', 'overlay'],
  },
  mediaShortcut: {
    label: 'Keyboard shortcut',
    description: "Sends a page's video or audio to Wirecatch.",
    keywords: ['key', 'hotkey', 'media', 'audio'],
  },
  app: {
    label: 'Wirecatch app',
    description: 'The app these settings belong to.',
    keywords: ['connection', 'status', 'version', 'open', 'native'],
  },
} as const satisfies Record<string, Searchable>;

export type SettingKey = keyof typeof SETTINGS;

export interface OptionSection {
  id: string;
  title: string;
  settings: readonly SettingKey[];
}

/** The options page, section by section, in the order it shows them. */
export const OPTION_SECTIONS: readonly OptionSection[] = [
  {
    id: 'capture',
    title: 'Capture',
    settings: ['captureEnabled', 'askBeforeDownload', 'defaultQueue'],
  },
  { id: 'filters', title: 'Which downloads', settings: ['fileTypes', 'minSize'] },
  { id: 'sites', title: 'Sites', settings: ['ignoredSites'] },
  { id: 'media', title: 'Video and audio', settings: ['mediaButton', 'mediaShortcut'] },
  { id: 'connection', title: 'Connection', settings: ['app'] },
];

/** A setting as the options page's search sees it: its own words and its section's title. */
export function searchable(key: SettingKey): Searchable {
  const section = OPTION_SECTIONS.find((s) => s.settings.includes(key));
  const setting = SETTINGS[key];
  return { ...setting, keywords: [...setting.keywords, section?.title ?? ''] };
}
