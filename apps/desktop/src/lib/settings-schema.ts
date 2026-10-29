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
    help: 'Runs in the tray so browser downloads are captured right away.',
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
    help: 'For browser downloads that start without asking. A notice appears in the current tab, or a system notification when the browser is in the background.',
    keywords: 'started start begin note browser tab toast notification notify tell',
  },
  {
    id: 'start_notice_time',
    section: 'notifications',
    label: 'Hide the start notice after',
    help: 'Also applies to the system notification shown when the browser is in the background. Used only when start notices appear in the browser.',
    keywords: 'seconds duration time long short note browser started',
  },
  {
    id: 'finish_notice',
    section: 'notifications',
    label: 'When a download finishes',
    help: 'How Wirecatch notifies you when a download finishes. Failed downloads are reported the same way, right away.',
    keywords:
      'notification notify alert done complete finished failed popup dialog window toast message tell browser note',
  },
  {
    id: 'finish_notice_time',
    section: 'notifications',
    label: 'Hide the finish notice after',
    help: 'Notices about failed downloads stay until you close them. Used only when finish notices appear in the browser.',
    keywords: 'seconds duration time long note browser finished until closed',
  },
  {
    id: 'notify_during_do_not_disturb',
    section: 'notifications',
    label: 'Show notifications during Do Not Disturb',
    help: 'Notifications about finished downloads, failed downloads and problems still appear while Do Not Disturb is on. Windows can still block them in its notification settings.',
    keywords:
      'do not disturb dnd focus assist quiet hours silent mute priority important notification',
  },
  {
    id: 'default_folder',
    section: 'files',
    label: 'Download folder',
    help: 'Used when no file type rule below matches.',
    keywords: 'location directory path save destination',
  },
  {
    id: 'categories',
    section: 'files',
    label: 'Folders by file type',
    help: 'Files are saved to these folders based on their extension.',
    keywords: 'categories video music archive documents programs images extensions rules',
  },
  {
    id: 'capture_enabled',
    section: 'browser',
    label: 'Capture downloads from the browser',
    help: 'Wirecatch takes over downloads you start in Chrome, Edge, Firefox and other browsers.',
    keywords: 'capture extension intercept integration chrome firefox edge',
  },
  {
    id: 'ask_before_download',
    section: 'browser',
    label: 'Show a window before each download',
    help: 'Lets you rename the file, choose its folder and queue, or save it for later. Videos follow Settings > Media.',
    keywords: 'confirm prompt dialog question ask window start without asking',
  },
  {
    id: 'show_progress_windows',
    section: 'browser',
    label: 'Keep the download window open to show progress',
    help: 'After you click Download now, the window stays open and shows the download’s progress.',
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
    label: 'Sites to ignore',
    help: 'Downloads from these sites are left to the browser.',
    keywords: 'ignore exclude block skip leave domains websites',
  },
  {
    id: 'capture_extensions',
    section: 'browser',
    label: 'Only capture these file types',
    help: 'Leave empty to capture all file types.',
    keywords: 'extensions formats filter types',
  },
  {
    id: 'min_capture_size',
    section: 'browser',
    label: 'Skip files smaller than',
    help: 'Smaller files, such as images on a page, are left to the browser. Set to 0 to capture files of any size.',
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
    label: 'When you download a video',
    help: 'Ask for a quality each time, or start right away with the settings below. Installing the media add-on always asks first.',
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
    label: 'Additional video sites',
    help: 'Wirecatch looks for video on pages from these sites, whether you use the download button in the browser or add a link. YouTube, Vimeo, TikTok and many others are already supported.',
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
    help: 'Used when a language has no subtitles written by people.',
    keywords: 'subtitles captions automatic auto-generated audio',
    group: 'Audio only',
  },
  {
    id: 'media_audio_chapters',
    section: 'media',
    label: 'Add chapters to M4A audio',
    help: 'Adds the video’s chapters to M4A files.',
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
    help: 'Installs new versions of the media add-on automatically.',
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
    section: 'import_export',
    label: 'Export settings',
    help: 'Saves your settings, queues, schedules and folder rules to a single file.',
    keywords: 'backup save profile export migrate',
  },
  {
    id: 'import',
    section: 'import_export',
    label: 'Import settings',
    help: 'Shows what will change before replacing your settings and queues.',
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
    label: 'Global proxy',
    help: 'Used by every queue that has no proxy of its own. Updates and add-ons use the Main queue’s proxy. Changes apply to downloads that start afterwards.',
    keywords: 'proxy socks socks5 http https firewall vpn corporate office no proxy direct system',
  },
  {
    id: 'proxy_servers',
    section: 'network',
    label: 'Proxy servers',
    help: 'The proxy servers you use, with their sign-in details. The global proxy and each queue can use any of them.',
    keywords: 'proxy servers servers login password user name test connection copy from system',
  },
  {
    id: 'proxy_exceptions',
    section: 'network',
    label: 'Bypass proxy for',
    help: 'These hosts are reached directly by every queue that uses the global list.',
    keywords: 'proxy exceptions bypass no_proxy direct intranet local hosts',
  },
  {
    id: 'ftp_bypass_proxy',
    section: 'network',
    label: 'Bypass proxy for FTP links',
    help: 'Only SOCKS5 proxies support FTP. When on, FTP downloads connect directly if the proxy is another type. When off, those downloads fail.',
    keywords: 'ftp ftps proxy direct',
  },
  {
    id: 'local_sites',
    section: 'advanced',
    label: 'Local network sites',
    help: 'Links from these sites may reach devices on your network, such as a NAS or router. IP addresses and single-word names are always treated as local.',
    keywords: 'local network intranet nas router private lan trusted hosts',
  },
  {
    id: 'update_suffix_list',
    section: 'advanced',
    label: 'Update the Public Suffix List automatically',
    help: 'Checks publicsuffix.org once a week while nothing is downloading. The list tells Wirecatch which sites belong to the same owner. Only Wirecatch’s name and version are sent; the site sees your IP address.',
    keywords: 'public suffix list psl domains owner privacy update publicsuffix.org',
  },
  {
    id: 'queues',
    section: 'queues',
    label: 'Your queues',
    help: 'Test a link to see which queue it goes to. Edit a queue to change its name, limits, rules, schedule or notifications.',
    keywords:
      'queue queues schedule overnight limit at once concurrent notify notification all done finished batch auto claim claims site try link',
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
