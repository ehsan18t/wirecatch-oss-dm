// The name and help of each setting the popup shows. Those the app's settings show too come from the words both share (`SETTING_WORDS`, D61), so the two say the same.
import { SETTING_WORDS } from '@wirecatch/protocol';

interface SettingText {
  label: string;
  help: string;
}

/** A shared setting's name, with the popup's short help where the app's names parts of its own screen. */
function shared(id: keyof typeof SETTING_WORDS): SettingText {
  const words = SETTING_WORDS[id];
  return { label: words.label, help: 'brief' in words ? words.brief : words.help };
}

export const SETTINGS = {
  captureEnabled: {
    label: 'Capture downloads',
    help: 'Send downloads from this browser to Wirecatch.',
  },
  askBeforeDownload: {
    label: 'Show a window before each download',
    help: 'Choose where each file goes before it starts.',
  },
  askEachVideo: {
    label: 'Ask about each video',
    help: 'Choose a quality for each video. When off, videos use your Video and audio settings.',
  },
  defaultQueue: shared('default_queue'),
  speedLimit: shared('global_speed_limit'),
  concurrent: shared('max_concurrent_downloads'),
  mediaButton: shared('show_media_button'),
  privateWindows: shared('private_windows'),
  shortcut: {
    label: 'Keyboard shortcut',
    help: "Sends a page's video or audio to Wirecatch.",
  },
  /** Firefox has no shortcuts page the popup can open, so the row says where it is. */
  shortcutFirefox: {
    label: 'Keyboard shortcut',
    help: "Sends a page's video or audio to Wirecatch. Set it in the add-ons manager, under Manage Extension Shortcuts.",
  },
  quality: shared('media_quality'),
  fallback: shared('media_fallback'),
  videoFormat: shared('media'),
  subtitles: shared('media_subtitles'),
  autoSubtitles: shared('media_auto_subtitles'),
  subtitleFiles: shared('media_subtitle_files'),
  unembeddedSubtitles: shared('media_unembedded_subtitles'),
  chapters: shared('media_chapters'),
  audioFormat: shared('media_audio'),
  audioQuality: {
    label: 'Audio quality',
    help: 'Higher sounds better and takes more space.',
  },
} as const satisfies Record<string, SettingText>;
