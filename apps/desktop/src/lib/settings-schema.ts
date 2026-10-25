// Every setting, in plain words, with a one-line explanation. The settings screen, its search
// and the command palette all read this one list.

import { DEFAULT_CONNECTIONS_PER_DOWNLOAD, SETTING_WORDS } from '@wirecatch/protocol';
import type { SectionId } from './settings-sections';

/** The name and help of a setting the extension's popup shows too, in the words both share (D61). */
function words(id: keyof typeof SETTING_WORDS): { label: string; help: string } {
  const { label, help } = SETTING_WORDS[id];
  return { label, help };
}

/** What every setting says about itself. */
interface SettingText {
  id: string;
  section: SectionId;
  label: string;
  help: string;
  /** Extra words people might search for. */
  keywords: string;
  /** A heading the setting sits under within its section, for a section that holds two kinds of thing (Media: videos and audio only). */
  group?: string;
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
    id: 'start_notice',
    section: 'notifications',
    label: 'When a download starts',
    help: 'For downloads from the browser that start without a window. A note shows in the tab you are looking at, or a notification when the browser is not in front.',
    keywords: 'started start begin note browser tab toast notification notify tell',
  },
  {
    id: 'start_note_time',
    section: 'notifications',
    label: 'Show the start note for',
    help: 'How long the note about a download that started shows before it goes by itself, and the notification when the browser is not in front. Only when a download start is shown in the browser.',
    keywords: 'seconds duration time long short note browser started',
  },
  {
    id: 'finish_notice',
    section: 'notifications',
    label: 'When a download finishes',
    help: 'How Wirecatch tells you a download finished. You hear about a failed download the same way, right away.',
    keywords:
      'notification notify alert done complete finished failed popup dialog window toast message tell browser note',
  },
  {
    id: 'finish_note_time',
    section: 'notifications',
    label: 'Show the finish note for',
    help: 'How long the note in the browser about finished downloads shows before it goes by itself. A note about a failure stays until you close it. Only when a finish is shown in the browser.',
    keywords: 'seconds duration time long note browser finished until closed',
  },
  {
    id: 'notify_during_do_not_disturb',
    section: 'notifications',
    label: 'Show notifications during Do Not Disturb',
    help: 'Notifications about finished and failed downloads, and about problems, still show while Windows is set to Do Not Disturb. Windows can still hold them back in its own settings for Wirecatch.',
    keywords:
      'do not disturb dnd focus assist quiet hours silent mute priority important notification',
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
    help: 'A window lets you rename the file, pick its folder and queue, or keep it for later. Videos follow Settings, Media.',
    keywords: 'confirm prompt dialog question ask window start without asking',
  },
  {
    id: 'show_progress_windows',
    section: 'browser',
    label: 'Keep the window open to show progress',
    help: 'After Download now, the window stays open with the progress.',
    keywords: 'progress window close speed popup',
  },
  {
    id: 'show_media_button',
    section: 'browser',
    ...words('show_media_button'),
    keywords: 'media video audio overlay button stream',
  },
  {
    id: 'private_windows',
    section: 'browser',
    ...words('private_windows'),
    keywords: 'private incognito inprivate window tab',
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
    ...words('default_queue'),
    keywords: 'queue default capture',
  },
  {
    id: 'media_ask',
    section: 'media',
    label: 'When a video is caught',
    help: 'Ask for a quality each time, or download at once with the choices below. Installing the media add-on always asks.',
    keywords: 'ask quality window prompt automatic remember preference without asking every time',
    group: 'Videos',
  },
  {
    id: 'media_quality',
    section: 'media',
    ...words('media_quality'),
    keywords: 'resolution quality 4k 2160p 1440p 1080p 720p 480p height best audio only preferred',
    group: 'Videos',
  },
  {
    id: 'media_fallback',
    section: 'media',
    ...words('media_fallback'),
    keywords: 'fallback missing unavailable lower higher closest ask resolution',
    group: 'Videos',
  },
  {
    id: 'media',
    section: 'media',
    ...words('media'),
    keywords: 'format container convert mp4 mkv webm save as video h264 vp9 av1',
    group: 'Videos',
  },
  {
    id: 'media_subtitles',
    section: 'media',
    ...words('media_subtitles'),
    keywords: 'subtitles captions cc languages srt',
    group: 'Videos',
  },
  {
    id: 'media_auto_subtitles',
    section: 'media',
    ...words('media_auto_subtitles'),
    keywords: 'subtitles captions automatic auto-generated speech recognition',
    group: 'Videos',
  },
  {
    id: 'media_subtitle_files',
    section: 'media',
    ...words('media_subtitle_files'),
    keywords: 'subtitles srt sidecar files embed',
    group: 'Videos',
  },
  {
    id: 'media_chapters',
    section: 'media',
    ...words('media_chapters'),
    keywords: 'chapters markers sections',
    group: 'Videos',
  },
  {
    id: 'video_sites',
    section: 'media',
    label: 'More video sites',
    help: 'Their pages are read for their video, from the browser’s video button and from links you add. YouTube, Vimeo, TikTok and many others are known already.',
    keywords: 'video sites pages websites platforms youtube add paste link extractor supported',
    group: 'Videos',
  },
  {
    id: 'media_audio',
    section: 'media',
    ...words('media_audio'),
    keywords: 'audio music mp3 m4a aac opus flac wav bitrate kbps quality format',
    group: 'Audio only',
  },
  {
    id: 'media_audio_subtitles',
    section: 'media',
    label: 'Subtitle languages for audio',
    help: 'Saved as .srt files next to the audio file.',
    keywords: 'subtitles lyrics captions languages srt audio',
    group: 'Audio only',
  },
  {
    id: 'media_audio_auto_subtitles',
    section: 'media',
    label: 'Use auto-generated subtitles for audio',
    help: 'For a language with no subtitles written by people.',
    keywords: 'subtitles captions automatic auto-generated audio',
    group: 'Audio only',
  },
  {
    id: 'media_audio_chapters',
    section: 'media',
    label: 'Add chapters to M4A audio',
    help: 'Adds a video’s chapters to M4A files.',
    keywords: 'chapters markers audio m4a',
    group: 'Audio only',
  },
  {
    id: 'media_addon',
    section: 'addons',
    label: 'Media add-on',
    help: 'Downloads video from YouTube and about 1,800 other sites.',
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
    ...words('global_speed_limit'),
    keywords: 'bandwidth throttle cap limit rate',
  },
  {
    id: 'export',
    section: 'transfer',
    label: 'Save to a file',
    help: 'Settings, queues, schedules and folder rules, in one file.',
    keywords: 'backup save profile export migrate',
  },
  {
    id: 'import',
    section: 'transfer',
    label: 'Load from a file',
    help: 'Shows what changes before it replaces your settings and queues.',
    keywords: 'restore load profile import migrate',
  },
  {
    id: 'max_concurrent_downloads',
    section: 'speed',
    ...words('max_concurrent_downloads'),
    keywords: 'concurrent simultaneous parallel limit',
  },
  {
    id: 'connections_per_download',
    section: 'speed',
    label: 'Connections per download',
    help: `More connections can be faster on servers that allow it. ${DEFAULT_CONNECTIONS_PER_DOWNLOAD} suits most servers.`,
    keywords: 'threads segments parts parallel connections speed',
  },
  {
    id: 'proxy',
    section: 'network',
    label: 'Proxy',
    help: 'Everything Wirecatch sends goes through it: downloads, videos, add-ons and updates. Changes take effect when you save them.',
    keywords:
      'proxy socks socks5 http https firewall vpn corporate office no proxy direct system exceptions bypass login password',
  },
  {
    id: 'local_sites',
    section: 'advanced',
    label: 'Sites on your own network',
    help: 'Their pages may link to devices like a NAS or a router. Network addresses and one-word names count without being listed.',
    keywords: 'local network intranet nas router private lan trusted hosts',
  },
  {
    id: 'update_suffix_list',
    section: 'advanced',
    label: 'Keep the list of public domains up to date',
    help: 'Weekly from publicsuffix.org, while nothing downloads. It tells which sites share an owner. Only Wirecatch’s name and version are sent; the site sees your IP address.',
    keywords: 'public suffix list psl domains owner privacy update publicsuffix.org',
  },
  {
    id: 'queues',
    section: 'queues',
    label: 'Your queues',
    help: 'Edit a queue to change its name, hours, limits and when it tells you its downloads finished.',
    keywords:
      'queue queues schedule overnight limit at once concurrent notify notification all done finished batch',
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
