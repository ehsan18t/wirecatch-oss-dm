import type {
  AddAction,
  ErrorCode,
  Event,
  ExtensionSettings,
  ExtensionSettingsPatch,
  MediaKind,
  Outcome,
  ProtocolError,
  QueueSummary,
  Reply,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import {
  arrayOf,
  isBoolean,
  isNumber,
  isString,
  literal,
  member,
  nullable,
  object,
  oneOf,
  variants,
  type Guard,
} from './validate';

/** The fields a popup change can carry: whole settings fields plus the list edits. */
export type SettingsChange = Partial<ExtensionSettings> & {
  add_ignored_sites?: string[];
  remove_ignored_sites?: string[];
};

/** A patch that changes only the given fields; every other field is sent as null (no change). */
export function patchOf(changes: SettingsChange): ExtensionSettingsPatch {
  return {
    capture_enabled: changes.capture_enabled ?? null,
    ask_before_download: changes.ask_before_download ?? null,
    show_media_button: changes.show_media_button ?? null,
    ignored_sites: changes.ignored_sites ?? null,
    add_ignored_sites: changes.add_ignored_sites ?? null,
    remove_ignored_sites: changes.remove_ignored_sites ?? null,
    capture_extensions: changes.capture_extensions ?? null,
    min_capture_size: changes.min_capture_size ?? null,
    default_queue: changes.default_queue ?? null,
  };
}

/**
 * Applies a patch the same way the app does (`ExtensionSettings::apply`): the
 * whole list first, then additions (skipping ones already there), then removals.
 */
export function applyPatch(
  settings: ExtensionSettings,
  patch: ExtensionSettingsPatch,
): ExtensionSettings {
  let sites = patch.ignored_sites ?? settings.ignored_sites;
  for (const host of patch.add_ignored_sites ?? []) {
    if (!sites.includes(host)) sites = [...sites, host];
  }
  const removed = patch.remove_ignored_sites ?? [];
  sites = sites.filter((host) => !removed.includes(host));
  return {
    capture_enabled: patch.capture_enabled ?? settings.capture_enabled,
    ask_before_download: patch.ask_before_download ?? settings.ask_before_download,
    show_media_button: patch.show_media_button ?? settings.show_media_button,
    ignored_sites: sites,
    capture_extensions: patch.capture_extensions ?? settings.capture_extensions,
    min_capture_size: patch.min_capture_size ?? settings.min_capture_size,
    default_queue: patch.default_queue ?? settings.default_queue,
  };
}

const CONFIRMED_ACTIONS: ReadonlySet<string> = new Set<AddAction>([
  'started',
  'queued',
  'awaiting_confirmation',
]);

/**
 * True when the app has accepted the download, so the browser copy may go.
 * Anything else (including a value a newer app might invent) keeps the browser copy.
 *
 * Only `started` means the app has already reached the server and data is flowing.
 * `queued` and `awaiting_confirmation` mean accepted but the link not yet tried, so
 * there is a remaining risk: a link that only works once, or one that needs the
 * browser's POST request, may fail later in the app after the browser copy is gone.
 * The app then shows the error with a retry. The extension's webRequest access only
 * observes responses (to spot media); it does not look at download requests.
 */
export function isConfirmed(action: AddAction): boolean {
  return CONFIRMED_ACTIONS.has(action);
}

/*
 * Guards for the protocol types this extension reads from outside: frames from the app,
 * and copies of them kept in browser storage. Each is typed by the generated type, so
 * the compiler rejects a guard that misses a field or a union member. Until W3 generates
 * validators from `crates/protocol`, these are the one place they are written.
 */

const isStringList = arrayOf(isString);

export const isExtensionSettings: Guard<ExtensionSettings> = object({
  capture_enabled: isBoolean,
  ask_before_download: isBoolean,
  show_media_button: isBoolean,
  ignored_sites: isStringList,
  capture_extensions: isStringList,
  min_capture_size: isNumber,
  default_queue: isNumber,
});

export const isQueueSummary: Guard<QueueSummary> = object({
  id: isNumber,
  name: isString,
  is_default: isBoolean,
});

export const isMediaKind = member<MediaKind>({ hls: true, dash: true, direct: true, page: true });

const isAddAction = member<AddAction>({ started: true, queued: true, awaiting_confirmation: true });

const isWithdrawOutcome = member<WithdrawOutcome>({
  withdrawn: true,
  not_found: true,
  already_started: true,
});

const isErrorCode = member<ErrorCode>({
  unsupported_version: true,
  invalid_request: true,
  invalid_url: true,
  not_found: true,
  app_unavailable: true,
  app_not_running: true,
  internal: true,
  not_started: true,
});

const isProtocolError: Guard<ProtocolError> = object({ code: isErrorCode, message: isString });

const isReply = variants<Reply>({
  hello: object({
    type: literal('hello'),
    data: object({ app_version: isString, protocol_version: isNumber }),
  }),
  download_added: object({
    type: literal('download_added'),
    data: object({ id: nullable(isNumber), action: isAddAction }),
  }),
  withdrawn: object({ type: literal('withdrawn'), data: isWithdrawOutcome }),
  extension_settings: object({ type: literal('extension_settings'), data: isExtensionSettings }),
  queues: object({ type: literal('queues'), data: arrayOf(isQueueSummary) }),
  done: object({ type: literal('done') }),
});

/** The app's answer to a request: a reply this extension knows, or an error it knows. */
export const isOutcome: Guard<Outcome> = oneOf(
  object({ status: literal('ok'), value: isReply }),
  object({ status: literal('err'), value: isProtocolError }),
);

export const isEvent = variants<Event>({
  extension_settings_changed: object({
    type: literal('extension_settings_changed'),
    data: isExtensionSettings,
  }),
  queues_changed: object({ type: literal('queues_changed'), data: arrayOf(isQueueSummary) }),
});
