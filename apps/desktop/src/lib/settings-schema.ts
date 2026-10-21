// Every setting, in plain words, with a one-line explanation. The settings screen, its search
// and the command palette all read this one list.

import { DEFAULT_CONNECTIONS_PER_DOWNLOAD } from '@wirecatch/protocol';

export type SectionId =
  | 'general'
  | 'files'
  | 'browser'
  | 'media'
  | 'addons'
  | 'speed'
  | 'queues'
  | 'transfer'
  | 'advanced';

export const SECTIONS: readonly { id: SectionId; title: string; blurb: string }[] = [
  { id: 'general', title: 'General', blurb: 'How Wirecatch looks and starts.' },
  { id: 'files', title: 'Saving files', blurb: 'Where finished files go.' },
  {
    id: 'browser',
    title: 'Browser',
    blurb: 'What the browser extension catches. The extension popup changes these same settings.',
  },
  {
    id: 'media',
    title: 'Media',
    blurb:
      'What the quality window for a video starts with. It remembers your last choices here, separately for videos and for audio only.',
  },
  {
    id: 'addons',
    title: 'Add-ons',
    blurb: 'Optional extras. Wirecatch works without them and installs them only when you ask.',
  },
  { id: 'speed', title: 'Speed', blurb: 'Keep downloads from slowing everything else down.' },
  {
    id: 'queues',
    title: 'Queues',
    blurb:
      'Each queue has its own schedule and limits. Downloads from the browser join the one chosen under Browser.',
  },
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

/** What every setting says about itself. */
interface SettingText {
  id: string;
  section: SectionId;
  label: string;
  help: string;
  /** Extra words people might search for. */
  keywords: string;
}

const DEFS = [
  {
    id: 'theme',
    section: 'general',
    label: 'Appearance',
    help: 'Follow your system, or always use light or dark.',
    keywords: 'theme dark mode light mode colors',
  },
  {
    id: 'accent',
    section: 'general',
    label: 'Accent colour',
    help: 'The colour that marks what is selected and what is downloading. System uses the colour you chose in Windows.',
    keywords: 'accent color colour highlight tint',
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
    label: 'Show a window before each download',
    help: 'Each file from the browser opens its own window first, so you can change its name, folder and queue, or download it later. Off: files start at once, and the main window comes forward with a note for each. Videos always ask for a quality.',
    keywords: 'confirm prompt dialog question ask window start without asking',
  },
  {
    id: 'show_progress_windows',
    section: 'browser',
    label: 'Keep the window open to show progress',
    help: 'After Download now, the download’s window stays and shows its progress. Off: the window closes when the download starts.',
    keywords: 'progress window close speed popup',
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
    id: 'media',
    section: 'media',
    label: 'Save videos as',
    help: 'Original keeps the format the site sends. MP4, MKV and WebM copy the video when it fits, and convert it only when the quality has no version in that format.',
    keywords: 'format container convert mp4 mkv webm save as video h264 vp9 av1',
  },
  {
    id: 'media_subtitles',
    section: 'media',
    label: 'Subtitle languages for videos',
    help: 'Ticked first when a video has them, for example en or fr.',
    keywords: 'subtitles captions cc languages srt',
  },
  {
    id: 'media_auto_subtitles',
    section: 'media',
    label: 'Use auto-generated subtitles for videos',
    help: 'When a language has no subtitles written by people, tick the ones made by speech recognition.',
    keywords: 'subtitles captions automatic auto-generated speech recognition',
  },
  {
    id: 'media_subtitle_files',
    section: 'media',
    label: 'Save subtitles as separate files',
    help: 'Puts subtitles next to a video as .srt files instead of inside it.',
    keywords: 'subtitles srt sidecar files embed',
  },
  {
    id: 'media_chapters',
    section: 'media',
    label: 'Add chapters to videos',
    help: 'Adds a video’s chapters to MP4, MKV and WebM files, so players can jump between them.',
    keywords: 'chapters markers sections',
  },
  {
    id: 'media_audio',
    section: 'media',
    label: 'Save audio as',
    help: 'For Audio only. Title, artist and the video’s picture go into MP3, M4A and FLAC files.',
    keywords: 'audio music mp3 m4a aac opus flac wav bitrate kbps quality format',
  },
  {
    id: 'media_audio_subtitles',
    section: 'media',
    label: 'Subtitle languages for audio',
    help: 'Saved as .srt files next to the audio file.',
    keywords: 'subtitles lyrics captions languages srt audio',
  },
  {
    id: 'media_audio_auto_subtitles',
    section: 'media',
    label: 'Use auto-generated subtitles for audio',
    help: 'When a language has no subtitles written by people, tick the ones made by speech recognition.',
    keywords: 'subtitles captions automatic auto-generated audio',
  },
  {
    id: 'media_audio_chapters',
    section: 'media',
    label: 'Add chapters to M4A audio',
    help: 'Adds a video’s chapters to M4A files.',
    keywords: 'chapters markers audio m4a',
  },
  {
    id: 'media_addon',
    section: 'addons',
    label: 'Media add-on',
    help: 'Downloads video from streaming sites, YouTube and about 1,800 other sites. Includes yt-dlp, ffmpeg and Deno.',
    keywords: 'addon add-on youtube video stream yt-dlp ffmpeg deno install media extractor',
  },
  {
    id: 'auto_update_addons',
    section: 'addons',
    label: 'Keep add-ons up to date',
    help: 'Quietly installs new versions of the media add-on when they come out.',
    keywords: 'addon add-on update ffmpeg yt-dlp media automatic',
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
    help: `More connections can be faster on servers that allow it. ${DEFAULT_CONNECTIONS_PER_DOWNLOAD} suits most servers.`,
    keywords: 'threads segments parts parallel connections speed',
  },
  {
    id: 'max_concurrent_downloads',
    section: 'advanced',
    label: 'Downloads at the same time',
    help: 'Across all queues together. Each queue also has its own limit.',
    keywords: 'concurrent simultaneous parallel limit',
  },
  {
    id: 'local_sites',
    section: 'advanced',
    label: 'Your sites on your network',
    help: 'Links on pages from these sites may lead to devices on your network, like a NAS or a router. Pages at a network address, or with a one-word name, count without being listed.',
    keywords: 'local network intranet nas router private lan trusted hosts',
  },
  {
    id: 'update_suffix_list',
    section: 'advanced',
    label: 'Keep the list of public domains up to date',
    help: 'The Public Suffix List tells Wirecatch which addresses belong to one owner, for example which of your sites count as on your network. Once a week while nothing downloads, or the next day after a check that failed, Wirecatch fetches it from publicsuffix.org. Privacy: the request names Wirecatch and its version and sends nothing about your downloads or browsing; publicsuffix.org sees your IP address.',
    keywords: 'public suffix list psl domains owner privacy update publicsuffix.org',
  },
  {
    id: 'queues',
    section: 'queues',
    label: 'Your queues',
    help: 'Edit a queue to change its name, when it runs, how many of its downloads run at once and its speed limit.',
    keywords: 'queue queues schedule overnight limit at once concurrent',
  },
] as const satisfies readonly SettingText[];

/** Every setting, by the id the screen, the palette and the core's field names share. */
export type SettingId = (typeof DEFS)[number]['id'];

/** The settings of one section, so each section's screen must show every one of them. */
export type SettingIn<S extends SectionId> = Extract<(typeof DEFS)[number], { section: S }>['id'];

export interface SettingDef extends SettingText {
  id: SettingId;
}

export const SETTINGS: readonly SettingDef[] = DEFS;

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
