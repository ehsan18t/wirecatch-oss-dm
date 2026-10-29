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
  /** The heading the setting sits under within its section. */
  group?: string;
}

const DEFS = [
  {
    id: 'theme',
    section: 'general',
    label: 'Theme',
    help: 'Follow your system, or always use light or dark.',
    keywords: 'theme appearance dark mode light mode colors',
    group: 'Appearance',
  },
  {
    id: 'accent',
    section: 'general',
    label: 'Accent colour',
    help: 'Marks what is selected and what is downloading. System uses your Windows accent colour.',
    keywords: 'accent color colour highlight tint',
    group: 'Appearance',
  },
  {
    id: 'start_with_system',
    section: 'general',
    label: 'Start Wirecatch when you sign in',
    help: 'Runs in the tray so browser downloads are captured right away.',
    keywords: 'windows startup boot login sign in autostart launch',
    group: 'Startup',
  },
  {
    id: 'close_to_tray',
    section: 'general',
    label: 'Keep downloading when the window is closed',
    help: 'Closing the window leaves Wirecatch in the tray instead of quitting.',
    keywords: 'tray background minimize quit exit',
    group: 'Startup',
  },
  {
    id: 'auto_update_addons',
    section: 'general',
    label: 'Keep add-ons up to date',
    help: 'Installs new versions of the media add-on automatically.',
    keywords: 'addon add-on update ffmpeg yt-dlp media automatic',
    group: 'Updates',
  },
  {
    id: 'update_suffix_list',
    section: 'general',
    label: 'Update the Public Suffix List automatically',
    help: 'Weekly, while nothing downloads. The list tells Wirecatch which sites share an owner; publicsuffix.org sees only your IP address and Wirecatch’s version.',
    keywords: 'public suffix list psl domains owner privacy update publicsuffix.org',
    group: 'Updates',
  },
  {
    id: 'export',
    section: 'general',
    label: 'Export settings',
    help: 'Saves your settings, queues, schedules and folder rules to a single file.',
    keywords: 'backup save profile export migrate',
    group: 'Backup',
  },
  {
    id: 'import',
    section: 'general',
    label: 'Import settings',
    help: 'Shows what will change before replacing your settings and queues.',
    keywords: 'restore load profile import migrate',
    group: 'Backup',
  },
  {
    id: 'notify_on_start',
    section: 'notifications',
    label: 'When a download starts',
    help: 'For browser downloads that start without asking: a notice in the tab, or a system notification while the browser is in the background.',
    keywords: 'started start begin notice browser tab toast notification notify tell',
  },
  {
    id: 'start_notice_time',
    section: 'notifications',
    label: 'Hide the start notice after',
    help: 'Applies to start notices in the browser and to their system notifications.',
    keywords: 'seconds duration time long short notice browser started',
  },
  {
    id: 'notify_on_finish',
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
    help: 'Applies to finish notices in the browser. Notices about failed downloads stay until you close them.',
    keywords: 'seconds duration time long notice browser finished until closed',
  },
  {
    id: 'notify_during_do_not_disturb',
    section: 'notifications',
    label: 'Show notifications during Do Not Disturb',
    help: 'Finished and failed downloads and problems still notify you. Windows can still block them in its own settings.',
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
    group: 'Capture',
  },
  {
    id: 'ignored_sites',
    section: 'browser',
    label: 'Sites to ignore',
    help: 'Downloads from these sites are left to the browser.',
    keywords: 'ignore exclude block skip leave domains websites',
    group: 'Capture',
  },
  {
    id: 'capture_extensions',
    section: 'browser',
    label: 'Only capture these file types',
    help: 'Leave empty to capture all file types.',
    keywords: 'extensions formats filter types',
    group: 'Capture',
  },
  {
    id: 'min_capture_size',
    section: 'browser',
    label: 'Skip files smaller than',
    help: 'Smaller files, like images on a page, stay in the browser. 0 captures every size.',
    keywords: 'minimum size small threshold',
    group: 'Capture',
  },
  {
    id: 'private_windows',
    section: 'browser',
    ...words('private_windows'),
    keywords: 'private incognito inprivate window tab',
    group: 'Capture',
  },
  {
    id: 'show_media_button',
    section: 'browser',
    ...words('show_media_button'),
    keywords: 'media video audio overlay button stream',
    group: 'Capture',
  },
  {
    id: 'default_queue',
    section: 'browser',
    ...words('default_queue'),
    keywords: 'queue default capture',
    group: 'Capture',
  },
  {
    id: 'ask_before_download',
    section: 'browser',
    label: 'Show a window before each download',
    help: 'Rename the file, pick its folder and queue, or save it for later. Videos follow Settings > Media.',
    keywords: 'confirm prompt dialog question ask window start without asking',
    group: 'Download window',
  },
  {
    id: 'show_progress_windows',
    section: 'browser',
    label: 'Keep the download window open to show progress',
    help: 'After you click Download now, the window stays open and shows the download’s progress.',
    keywords: 'progress window close speed popup',
    group: 'Download window',
  },
  {
    id: 'media_addon',
    section: 'media',
    label: 'Media add-on',
    help: 'Downloads video from YouTube and about 1,800 other sites.',
    keywords: 'addon add-on youtube video stream yt-dlp ffmpeg deno install media extractor',
  },
  {
    id: 'media_ask',
    section: 'media',
    label: 'When you download a video',
    help: 'Ask for a quality each time, or start at once with the settings below.',
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
    help: 'Wirecatch also looks for video on pages from these sites. YouTube, Vimeo, TikTok and many others work already.',
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
    id: 'global_speed_limit',
    section: 'speed',
    ...words('global_speed_limit'),
    keywords: 'bandwidth throttle cap limit rate',
    group: 'Limits',
  },
  {
    id: 'max_concurrent_downloads',
    section: 'speed',
    ...words('max_concurrent_downloads'),
    keywords: 'concurrent simultaneous parallel limit',
    group: 'Limits',
  },
  {
    id: 'connections_per_download',
    section: 'speed',
    label: 'Connections per download',
    help: `More connections can be faster on servers that allow it. ${DEFAULT_CONNECTIONS_PER_DOWNLOAD} suits most servers.`,
    keywords: 'threads segments parts parallel connections speed',
    group: 'Limits',
  },
  {
    id: 'queues',
    section: 'speed',
    label: 'Your queues',
    help: 'Edit a queue to change its limits, rules, schedule or notifications.',
    keywords:
      'queue queues schedule overnight limit at once concurrent notify notification all done finished batch auto claim claims site try link',
  },
  {
    id: 'proxy',
    section: 'network',
    label: 'Global proxy',
    help: 'Used by every queue without a proxy of its own. Updates and add-ons use the Main queue’s proxy.',
    keywords: 'proxy socks socks5 http https firewall vpn corporate office no proxy direct system',
    group: 'Proxy',
  },
  {
    id: 'proxy_servers',
    section: 'network',
    label: 'Proxy servers',
    help: 'Saved proxies and their sign-in details, for the global proxy and any queue.',
    keywords: 'proxy servers servers login password user name test connection copy from system',
    group: 'Proxy',
  },
  {
    id: 'proxy_exceptions',
    section: 'network',
    label: 'Hosts that skip the proxy',
    help: 'Queues using the global list connect to these hosts directly.',
    keywords: 'proxy exceptions bypass skip no_proxy direct intranet local hosts global list',
    group: 'Proxy',
  },
  {
    id: 'ftp_bypass_proxy',
    section: 'network',
    label: 'FTP links skip the proxy',
    help: 'Only SOCKS5 proxies carry FTP. On, FTP links go direct past other proxies; off, they fail.',
    keywords: 'ftp ftps proxy direct',
    group: 'Proxy',
  },
  {
    id: 'local_sites',
    section: 'network',
    label: 'Local network sites',
    help: 'Links from these sites may reach devices on your network, like a NAS or router. IP addresses and one-word names always count as local.',
    keywords: 'local network intranet nas router private lan trusted hosts',
  },
  {
    id: 'log_folder',
    section: 'general',
    label: 'Log folder',
    help: 'What Wirecatch did, for a support request. Logs leave out passwords and other secrets.',
    keywords: 'log logs diagnostics troubleshooting support debug folder problem report',
    group: 'Troubleshooting',
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
