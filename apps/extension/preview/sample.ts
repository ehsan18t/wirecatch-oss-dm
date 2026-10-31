// A scripted background script for the preview page: the pages' real model, fed the snapshot and
// media list of one sample state, answering every command the way the background would.
import {
  applyBrowserControlsPatch,
  applyExtensionSettingsPatch,
  queueId,
  type BrowserControls,
  type ExtensionSettings,
  type QueueSummary,
} from '@wirecatch/protocol';
import type { MediaEntry, MediaView } from '../src/lib/media/center';
import type {
  BackgroundMessage,
  Channel,
  Alert,
  PopupCommand,
  PopupRequest,
  Snapshot,
} from '../src/lib/popup-port';
import type { ConnectionStatus } from '../src/lib/native/status';
import { PopupModel } from '../src/lib/popup/model.svelte';

export const SAMPLE_STATES = [
  'connected',
  'no-media',
  'capture-off',
  'many',
  'closed',
  'missing',
  'no-access',
  'connecting',
  'long',
  'old-app',
] as const;
export type SampleState = (typeof SAMPLE_STATES)[number];

export const isSampleState = (value: unknown): value is SampleState =>
  SAMPLE_STATES.some((state) => state === value);

/** How long the sample app takes to answer a Download button, so its busy state shows. */
const SEND_DELAY_MS = 900;
const MB = 1024 ** 2;

const SETTINGS: ExtensionSettings = {
  capture_enabled: true,
  ask_before_download: true,
  show_media_button: true,
  private_windows: true,
  ignored_sites: ['ads.example.org', 'intranet.example.com'],
  capture_extensions: ['zip', 'exe', 'iso', 'mp4'],
  min_capture_size: 2 * MB,
  default_queue: queueId(1),
};

const CONTROLS: BrowserControls = {
  global_speed_limit: null,
  max_concurrent_downloads: 3,
  media: {
    video: {
      format: 'mp4',
      audio_kbps: null,
      subtitle_languages: ['en'],
      auto_subtitles: true,
      subtitle_files: false,
      unembedded_subtitles: 'files',
      chapters: true,
    },
    audio: {
      format: 'mp3',
      audio_kbps: 192,
      subtitle_languages: [],
      auto_subtitles: true,
      subtitle_files: false,
      unembedded_subtitles: 'files',
      chapters: false,
    },
    ask_each_time: true,
    quality: { height: 1080 },
    fallback: 'lower',
  },
};

const QUEUES: QueueSummary[] = [
  { id: queueId(1), name: 'Main', is_default: true },
  { id: queueId(2), name: 'Night', is_default: false },
];

const MEDIA: MediaEntry[] = [
  { key: 'hls:1', kind: 'hls', title: 'Video stream (HLS)', detail: 'cdn.example.com', size: null },
  {
    key: 'direct:1',
    kind: 'direct',
    title: 'Video file',
    detail: 'keynote-2026-full-session.mp4',
    size: 734 * MB,
  },
];

/** More media than the popup has room for, so its list scrolls. */
const MANY: MediaEntry[] = [
  {
    key: 'dash:1',
    kind: 'dash',
    title: 'Video stream (DASH)',
    detail: 'media.example.net',
    size: null,
  },
  {
    key: 'direct:2',
    kind: 'direct',
    title: 'Audio file',
    detail: 'episode-01-the-wire.mp3',
    size: 41 * MB,
  },
  {
    key: 'direct:3',
    kind: 'direct',
    title: 'Audio file',
    detail: 'episode-02-signals.mp3',
    size: 38 * MB,
  },
  {
    key: 'dash:4',
    kind: 'dash',
    title: 'Video stream (DASH)',
    detail: 'media.example.net',
    size: null,
  },
  {
    key: 'direct:5',
    kind: 'direct',
    title: 'Audio file',
    detail: 'episode-03-noise.mp3',
    size: 44 * MB,
  },
  {
    key: 'direct:6',
    kind: 'direct',
    title: 'Audio file',
    detail: 'episode-04-bandwidth.mp3',
    size: 36 * MB,
  },
  { key: 'page:7', kind: 'page', title: 'This video page', detail: 'www.example.com', size: null },
];

/** Names as long as real ones get: a CDN host, a file name without spaces, a queue named by a person. */
const LONG_MEDIA: MediaEntry[] = [
  {
    key: 'direct:long',
    kind: 'direct',
    title: 'Video file',
    detail: 'conference-2026-keynote-full-session-with-questions-and-answers-1080p.mp4',
    size: 1834 * MB,
  },
  {
    key: 'hls:long',
    kind: 'hls',
    title: 'Video stream (HLS)',
    detail: 'video-delivery-edge-cache-eu-west.streaming.example-media-network.com',
    size: null,
  },
];

const LONG_QUEUES: QueueSummary[] = [
  { id: queueId(1), name: 'Main', is_default: true },
  {
    id: queueId(2),
    name: 'Large files overnight, only while on the home network',
    is_default: false,
  },
];

const LONG_SITES = [
  'objects.githubusercontent.com',
  'release-assets.downloads.mirror-network.example-university.edu',
];

/** A speed limit with a decimal, as the "long" state shows it. */
const LONG_SPEED_LIMIT = 1.5 * MB;

/** The address of the page the "long" popup is opened over. */
export const LONG_PAGE_URL =
  'https://release-assets.downloads.mirror-network.example-university.edu/x';

const ALERTS: Alert[] = [
  {
    id: 'sample-alert',
    downloadId: null,
    untilResumed: false,
    urgent: false,
    tone: 'warning',
    message: 'setup.exe stayed in the browser: Wirecatch could not be reached in time.',
    at: 0,
  },
];

function status(state: SampleState): ConnectionStatus {
  switch (state) {
    case 'closed':
      return { state: 'unavailable', problem: { kind: 'app_closed', detail: null }, retryAt: null };
    case 'missing':
      return {
        state: 'unavailable',
        problem: { kind: 'host_missing', detail: 'Specified native messaging host not found.' },
        retryAt: Date.now(),
      };
    case 'connecting':
      return { state: 'connecting' };
    case 'connected':
    case 'no-media':
    case 'capture-off':
    case 'long':
    case 'many':
    case 'no-access':
    case 'old-app':
      return { state: 'connected', appVersion: '0.1.0' };
  }
}

/** The app is connected, so the popup has its settings. */
function connected(state: SampleState): boolean {
  return status(state).state === 'connected';
}

function snapshot(state: SampleState): Snapshot {
  if (!connected(state)) {
    return { status: status(state), settings: null, controls: null, queues: [], alerts: [] };
  }
  return {
    status: status(state),
    settings: {
      ...SETTINGS,
      capture_enabled: state !== 'capture-off',
      ...(state === 'long' ? { ignored_sites: LONG_SITES, default_queue: queueId(2) } : {}),
    },
    controls:
      state === 'old-app'
        ? null
        : state === 'long'
          ? { ...CONTROLS, global_speed_limit: LONG_SPEED_LIMIT }
          : CONTROLS,
    queues: state === 'long' ? LONG_QUEUES : QUEUES,
    alerts: state === 'many' || state === 'long' ? ALERTS : [],
  };
}

function media(state: SampleState, tabId: number): MediaView {
  if (!connected(state) || state === 'no-media') return { tabId, page: 'web', entries: [] };
  if (state === 'long') return { tabId, page: 'web', entries: LONG_MEDIA };
  return { tabId, page: 'web', entries: state === 'many' ? [...MEDIA, ...MANY] : MEDIA };
}

/** What the popup shows once the sample app carried out `command`: `shown` itself when nothing changed. */
function changed(shown: Snapshot, command: PopupCommand): Snapshot {
  const { settings, controls } = shown;
  if (command.type === 'update_settings' && settings !== null) {
    return { ...shown, settings: applyExtensionSettingsPatch(settings, command.patch) };
  }
  if (command.type === 'update_controls' && controls !== null) {
    return { ...shown, controls: applyBrowserControlsPatch(controls, command.patch) };
  }
  if (command.type === 'dismiss_alert') {
    return { ...shown, alerts: shown.alerts.filter((n) => n.id !== command.id) };
  }
  return shown;
}

/** A model for the pages, as the background would feed it in `state`. */
export function sampleModel(state: SampleState): PopupModel {
  const listeners: ((message: unknown) => void)[] = [];
  const send = (message: BackgroundMessage): void => {
    queueMicrotask(() => {
      for (const listener of listeners) listener(message);
    });
  };
  let current = snapshot(state);

  function answer(id: number, command: PopupCommand): void {
    if (command.type === 'download_media') {
      setTimeout(() => {
        send({ type: 'result', id, ok: true, message: 'Sent to Wirecatch.' });
      }, SEND_DELAY_MS);
      return;
    }
    if (command.type === 'watch_media') send({ type: 'media', view: media(state, command.tabId) });
    const next = changed(current, command);
    if (next !== current) {
      current = next;
      send({ type: 'snapshot', snapshot: current });
    }
    send({ type: 'result', id, ok: true });
  }

  const channel: Channel<PopupRequest, unknown> = {
    postMessage: ({ id, command }) => {
      answer(id, command);
    },
    onMessage: { addListener: (listener) => listeners.push(listener) },
    onDisconnect: { addListener: () => undefined },
  };
  const model = new PopupModel(channel);
  send({ type: 'snapshot', snapshot: current });
  return model;
}
