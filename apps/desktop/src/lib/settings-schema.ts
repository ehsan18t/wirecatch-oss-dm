// Every setting, in plain words, with a one-line explanation. The settings screen, its search
// and the command palette all read this one list.

export type SectionId = 'general' | 'files' | 'browser' | 'speed' | 'transfer' | 'advanced';

export const SECTIONS: ReadonlyArray<{ id: SectionId; title: string; blurb: string }> = [
  { id: 'general', title: 'General', blurb: 'How Wirecatch looks and starts.' },
  { id: 'files', title: 'Saving files', blurb: 'Where finished files go.' },
  {
    id: 'browser',
    title: 'Browser',
    blurb: 'What the browser extension catches. The extension popup changes these same settings.',
  },
  { id: 'speed', title: 'Speed', blurb: 'Keep downloads from slowing everything else down.' },
  {
    id: 'transfer',
    title: 'Move to another PC',
    blurb: 'Save your settings to a file and load them on another computer.',
  },
  {
    id: 'advanced',
    title: 'Advanced',
    blurb: 'Tuning for people who want it. The defaults suit almost everyone.',
  },
];

export type SettingId =
  | 'theme'
  | 'start_with_system'
  | 'close_to_tray'
  | 'notify_on_complete'
  | 'auto_update_addons'
  | 'default_folder'
  | 'categories'
  | 'capture_enabled'
  | 'ask_before_download'
  | 'show_media_button'
  | 'ignored_sites'
  | 'capture_extensions'
  | 'min_capture_size'
  | 'default_queue'
  | 'global_speed_limit'
  | 'export'
  | 'import'
  | 'connections_per_download'
  | 'max_concurrent_downloads';

export interface SettingDef {
  id: SettingId;
  section: SectionId;
  label: string;
  help: string;
  /** Extra words people might search for. */
  keywords: string;
}

export const SETTINGS: readonly SettingDef[] = [
  {
    id: 'theme',
    section: 'general',
    label: 'Appearance',
    help: 'Follow your system, or always use light or dark.',
    keywords: 'theme dark mode light mode colors',
  },
  {
    id: 'start_with_system',
    section: 'general',
    label: 'Start Wirecatch when you sign in',
    help: 'Runs quietly in the tray so browser downloads are caught right away.',
    keywords: 'windows startup boot login sign in autostart launch',
  },
  {
    id: 'close_to_tray',
    section: 'general',
    label: 'Keep downloading when the window is closed',
    help: 'Closing the window leaves Wirecatch in the tray instead of quitting.',
    keywords: 'tray background minimize quit exit',
  },
  {
    id: 'notify_on_complete',
    section: 'general',
    label: 'Notify me when a download finishes',
    help: 'Shows a system notification for each finished file.',
    keywords: 'notification alert done complete sound',
  },
  {
    id: 'auto_update_addons',
    section: 'general',
    label: 'Keep add-ons up to date',
    help: 'Quietly installs new versions of ffmpeg and the media add-on when they come out.',
    keywords: 'addon add-on update ffmpeg yt-dlp media automatic',
  },
  {
    id: 'default_folder',
    section: 'files',
    label: 'Download folder',
    help: 'Where files go when no file type rule below matches.',
    keywords: 'location directory path save destination',
  },
  {
    id: 'categories',
    section: 'files',
    label: 'Folders by file type',
    help: 'Files are sorted into these folders by their extension.',
    keywords: 'categories video music archive documents programs images extensions rules',
  },
  {
    id: 'capture_enabled',
    section: 'browser',
    label: 'Catch downloads from the browser',
    help: 'Wirecatch takes over downloads you start in Chrome, Edge, Firefox and others.',
    keywords: 'capture extension intercept integration chrome firefox edge',
  },
  {
    id: 'ask_before_download',
    section: 'browser',
    label: 'Ask before each download',
    help: 'Shows the file name and folder first, so you can change them before it starts.',
    keywords: 'confirm prompt dialog question',
  },
  {
    id: 'show_media_button',
    section: 'browser',
    label: 'Show a download button on videos',
    help: 'Adds a small button over video and audio on web pages.',
    keywords: 'media video audio overlay button stream',
  },
  {
    id: 'ignored_sites',
    section: 'browser',
    label: 'Sites to leave to the browser',
    help: 'Downloads from these sites stay in the browser.',
    keywords: 'ignore exclude block skip domains websites',
  },
  {
    id: 'capture_extensions',
    section: 'browser',
    label: 'File types to catch',
    help: 'Leave empty to catch every file type.',
    keywords: 'extensions formats filter types',
  },
  {
    id: 'min_capture_size',
    section: 'browser',
    label: 'Skip files smaller than',
    help: 'Small files, like images on a page, stay in the browser. Zero catches everything.',
    keywords: 'minimum size small threshold',
  },
  {
    id: 'default_queue',
    section: 'browser',
    label: 'Queue for browser downloads',
    help: 'Downloads caught from the browser join this queue.',
    keywords: 'queue default capture',
  },
  {
    id: 'global_speed_limit',
    section: 'speed',
    label: 'Speed limit',
    help: 'Caps the total download speed so browsing and calls stay smooth.',
    keywords: 'bandwidth throttle cap limit rate',
  },
  {
    id: 'export',
    section: 'transfer',
    label: 'Export settings',
    help: 'Saves settings, queues, schedules and folder rules to one file.',
    keywords: 'backup save profile export migrate',
  },
  {
    id: 'import',
    section: 'transfer',
    label: 'Import settings',
    help: 'Loads a settings file and replaces your current settings and queues.',
    keywords: 'restore load profile import migrate',
  },
  {
    id: 'connections_per_download',
    section: 'advanced',
    label: 'Connections per download',
    help: 'More connections can be faster on servers that allow it. 8 suits most servers.',
    keywords: 'threads segments parts parallel connections speed',
  },
  {
    id: 'max_concurrent_downloads',
    section: 'advanced',
    label: 'Downloads at the same time',
    help: 'Across all queues together. Each queue also has its own limit.',
    keywords: 'concurrent simultaneous parallel limit',
  },
];

/** Settings whose label, explanation or keywords contain every word typed. */
export function searchSettings(query: string): SettingDef[] {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...SETTINGS];
  return SETTINGS.filter((s) => {
    const hay = `${s.label} ${s.help} ${s.keywords}`.toLowerCase();
    return words.every((w) => hay.includes(w));
  });
}

export function settingAnchor(id: SettingId): string {
  return `setting-${id}`;
}
