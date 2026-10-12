import type {
  AddAction,
  AddedDownload,
  ErrorCode,
  Event,
  ExtensionSettings,
  ExtensionSettingsPatch,
  HelloReply,
  MediaKind,
  Outcome,
  ProtocolError,
  QueueSummary,
  Reply,
  WithdrawOutcome,
} from '@wirecatch/protocol';
import {
  arrayOf,
  fields,
  isBoolean,
  isNumber,
  isString,
  literal,
  member,
  nullable,
  type Variant,
  variants,
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

/*
 * Guards for the protocol types this extension reads from outside: frames from the app,
 * and copies of them kept in browser storage. Built from `fields`, `member` and `variants`
 * on the generated types, so the compiler rejects one that misses or narrows a field, a
 * union member or a variant. Until W3 generates validators from `crates/protocol`, these
 * are the one place they are written.
 */

const isStringList = arrayOf(isString);

export const isExtensionSettings = fields<ExtensionSettings>({
  capture_enabled: isBoolean,
  ask_before_download: isBoolean,
  show_media_button: isBoolean,
  ignored_sites: isStringList,
  capture_extensions: isStringList,
  min_capture_size: isNumber,
  default_queue: isNumber,
});

export const isQueueSummary = fields<QueueSummary>({
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

const isProtocolError = fields<ProtocolError>({ code: isErrorCode, message: isString });

const isReply = variants<Reply>({
  hello: fields<Variant<Reply, 'hello'>>({
    type: literal('hello'),
    data: fields<HelloReply>({ app_version: isString, protocol_version: isNumber }),
  }),
  download_added: fields<Variant<Reply, 'download_added'>>({
    type: literal('download_added'),
    data: fields<AddedDownload>({ id: nullable(isNumber), action: isAddAction }),
  }),
  withdrawn: fields<Variant<Reply, 'withdrawn'>>({
    type: literal('withdrawn'),
    data: isWithdrawOutcome,
  }),
  extension_settings: fields<Variant<Reply, 'extension_settings'>>({
    type: literal('extension_settings'),
    data: isExtensionSettings,
  }),
  queues: fields<Variant<Reply, 'queues'>>({
    type: literal('queues'),
    data: arrayOf(isQueueSummary),
  }),
  done: fields<Variant<Reply, 'done'>>({ type: literal('done') }),
});

/** The app's answer to a request: a reply this extension knows, or an error it knows. */
export const isOutcome = variants<Outcome, 'status'>({
  ok: fields<Variant<Outcome, 'ok', 'status'>>({ status: literal('ok'), value: isReply }),
  err: fields<Variant<Outcome, 'err', 'status'>>({
    status: literal('err'),
    value: isProtocolError,
  }),
});

export const isEvent = variants<Event>({
  extension_settings_changed: fields<Variant<Event, 'extension_settings_changed'>>({
    type: literal('extension_settings_changed'),
    data: isExtensionSettings,
  }),
  queues_changed: fields<Variant<Event, 'queues_changed'>>({
    type: literal('queues_changed'),
    data: arrayOf(isQueueSummary),
  }),
});
