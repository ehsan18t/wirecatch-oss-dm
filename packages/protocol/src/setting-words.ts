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
    help: 'Also captures downloads and videos in private windows. The browser must also allow Wirecatch there, in its extension settings.',
    brief: 'The browser must also allow Wirecatch there.',
  },
  default_queue: {
    label: 'Queue for browser downloads',
    help: 'Downloads captured from the browser are added to this queue.',
  },
  global_speed_limit: {
    label: 'Global speed limit',
    help: 'Keeps browsing and calls smooth while downloads run.',
  },
  max_concurrent_downloads: {
    label: 'Downloads at the same time',
    help: 'Across all queues. Each queue can set a lower limit.',
    brief: 'Across all queues.',
  },
  media_quality: {
    label: 'Resolution',
    help: 'Videos download at this resolution when Wirecatch doesn’t ask, and the quality window opens on it. Choosing another one there updates this setting. Audio-only downloads use the audio settings below.',
    brief: 'Used for videos that download without asking, and preselected in the quality window.',
  },
  media_fallback: {
    label: 'If a video doesn’t offer that resolution',
    help: 'Next lower picks the lowest available if none is lower. Next higher picks the best available if none is higher. Ask me opens the quality window.',
  },
  media: {
    label: 'Save videos as',
    help: 'Original keeps the site’s format. Other formats convert only when needed.',
  },
  media_subtitles: {
    label: 'Subtitle languages for videos',
    help: 'Uses the creator’s subtitles for each language, or auto-generated ones if allowed below. Languages a video doesn’t have are skipped.',
    brief: 'Languages a video doesn’t have are skipped.',
  },
  media_auto_subtitles: {
    label: 'Use auto-generated subtitles for videos',
    help: 'Used when a language has no subtitles written by people.',
  },
  media_subtitle_files: {
    label: 'Save subtitles as separate files',
    help: 'Saves subtitles as .srt files next to the video instead of inside it.',
  },
  media_chapters: {
    label: 'Add chapters to videos',
    help: 'Adds a video’s chapters to MP4, MKV and WebM files, so players can jump between them.',
  },
  media_audio: {
    label: 'Save audio as',
    help: 'MP3, M4A and FLAC files include the title, artist and cover art.',
  },
} as const satisfies Record<string, SettingWords>;
