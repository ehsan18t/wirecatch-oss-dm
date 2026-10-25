// The name and help of each setting that both the app's settings and the extension's popup show (D61), so the two always say the same. `brief` is the popup's shorter help, for a setting whose help in the app points at other parts of the app's own screen.

export interface SettingWords {
  label: string;
  help: string;
  brief?: string;
}

export const SETTING_WORDS = {
  show_media_button: {
    label: 'Show a download button on videos',
    help: 'Adds a small button over video and audio on web pages.',
  },
  private_windows: {
    label: 'Work in private windows',
    help: 'Takes downloads and videos from private windows too. The browser must also allow Wirecatch there, in its extension settings.',
    brief: 'The browser must also allow Wirecatch there.',
  },
  default_queue: {
    label: 'Queue for browser downloads',
    help: 'Downloads caught from the browser join this queue.',
  },
  global_speed_limit: {
    label: 'Limit total speed',
    help: 'Keeps browsing and calls smooth while downloads run.',
  },
  max_concurrent_downloads: {
    label: 'Downloads at the same time',
    help: 'Across all queues. A queue can run fewer, set in the queue.',
    brief: 'Across all queues.',
  },
  media_quality: {
    label: 'Resolution',
    help: 'What the quality window starts on, and what downloads without asking. Picking another in the window changes it. Audio only uses the Audio only choices below.',
    brief: 'What videos download at without asking, and where the quality window starts.',
  },
  media_fallback: {
    label: 'If a video doesn’t offer that resolution',
    help: 'Next lower takes the lowest when none is lower, and Next higher the best when none is higher. Ask me opens the quality window.',
  },
  media: {
    label: 'Save videos as',
    help: 'Original keeps the site’s format. The others convert only when they must.',
  },
  media_subtitles: {
    label: 'Subtitle languages for videos',
    help: 'For each language, its creator’s subtitles, or its auto-generated ones if allowed below. A language a video lacks is skipped.',
    brief: 'A language a video lacks is skipped.',
  },
  media_auto_subtitles: {
    label: 'Use auto-generated subtitles for videos',
    help: 'For a language with no subtitles written by people.',
  },
  media_subtitle_files: {
    label: 'Save subtitles as separate files',
    help: 'Puts subtitles next to a video as .srt files instead of inside it.',
  },
  media_chapters: {
    label: 'Add chapters to videos',
    help: 'Adds a video’s chapters to MP4, MKV and WebM files, so players can jump between them.',
  },
  media_audio: {
    label: 'Save audio as',
    help: 'Title, artist and cover picture go into MP3, M4A and FLAC files.',
  },
} as const satisfies Record<string, SettingWords>;
