// A scripted background script for the preview page: the pages' real model, fed the snapshot and
// media list of one sample state, answering every command the way the background would.
import type { OptionList } from '@wirecatch/protocol';
import { DEFAULT_OPTIONS, LONG_OPTIONS } from '@wirecatch/protocol/testing/options';
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
] as const;
export type SampleState = (typeof SAMPLE_STATES)[number];

export const isSampleState = (value: unknown): value is SampleState =>
  SAMPLE_STATES.some((state) => state === value);

/** How long the sample app takes to answer a Download button, so its busy state shows. */
const SEND_DELAY_MS = 900;
const MB = 1024 ** 2;

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
      return { state: 'connected', appVersion: '0.1.0' };
  }
}

/** The app is connected, so the popup has its settings. */
function connected(state: SampleState): boolean {
  return status(state).state === 'connected';
}

/** The options with capture turned off, as the "capture-off" state shows them. */
function withCaptureOff(list: OptionList): OptionList {
  return withValue(list, 'capture_enabled', false);
}

/** `list` with option `id` holding `value`, as the app sends it after a change. */
function withValue(list: OptionList, id: string, value: unknown): OptionList {
  return {
    ...list,
    groups: list.groups.map((group) => ({
      ...group,
      options: group.options.map((option) =>
        typeof option === 'object' && option !== null && 'id' in option && option.id === id
          ? { ...option, value }
          : option,
      ),
    })),
  };
}

function snapshot(state: SampleState): Snapshot {
  if (!connected(state)) {
    return { status: status(state), options: null, privateWindows: true, alerts: [] };
  }
  const options = state === 'long' ? LONG_OPTIONS : DEFAULT_OPTIONS;
  return {
    status: status(state),
    options: state === 'capture-off' ? withCaptureOff(options) : options,
    privateWindows: true,
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
  if (command.type === 'set_option' && shown.options !== null && 'set' in command.change) {
    return { ...shown, options: withValue(shown.options, command.id, command.change.set) };
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
    if (command.type === 'page_options') {
      // The sample app ignores no site: the page's own is named.
      send({ type: 'result', id, ok: true, page: { site: command.host, ignored: false } });
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
