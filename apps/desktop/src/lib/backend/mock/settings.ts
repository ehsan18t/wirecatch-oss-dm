// What the mock core does with settings: change them, and export and import settings files.

import {
  PROFILE_FORMAT_VERSION,
  type AddonId,
  type AppSettingsPatch,
  type ProxyChange,
  type ProxyPasswords,
  type ProxyRole,
  type ProxySettings,
  type ProxyTest,
  type ProxyTestResult,
  type ImportedFolder,
  type ImportPreview,
  type SettingsProfile,
} from '@wirecatch/protocol';
import { version } from '../../../../package.json';
import { defaultProxySettings, emptyServer, sameProxy } from '../../proxy';
import { applyPatch } from '../../state/settings-changes.svelte';
import { BackendError } from '../types';
import type { MockState } from './state';

export function changeSettings(state: MockState, patch: AppSettingsPatch): void {
  state.settings = applyPatch(state.settings, patch);
  state.settingsChanged();
}

const ROLES: readonly ProxyRole[] = ['all', 'http', 'https', 'ftp', 'socks'];

/** Refuses a manual setting with no proxy entered where it is used, as the core does. */
function checkProxy(proxy: ProxySettings): void {
  if (proxy.mode !== 'manual') return;
  const used: ProxyRole[] = proxy.per_protocol ? ['http', 'https', 'ftp', 'socks'] : ['all'];
  if (used.every((role) => proxy[role].host.trim() === '')) {
    throw new BackendError(
      'Enter a proxy server, or choose another proxy option.',
      'invalid_request',
    );
  }
}

/** The roles with a password after `passwords` changed the ones in `saved`, for the proxies `was` named becoming those `now` names. */
function withPasswords(
  saved: ReadonlySet<ProxyRole>,
  passwords: ProxyPasswords,
  was: ProxySettings,
  now: ProxySettings,
): Set<ProxyRole> {
  const kept = new Set(saved);
  for (const role of ROLES) {
    const change = passwords[role];
    if (change === undefined) {
      if (!sameProxy(was[role], now[role])) kept.delete(role);
      continue;
    }
    if (change === null || change === '') kept.delete(role);
    else kept.add(role);
  }
  return kept;
}

export function setProxy(state: MockState, change: ProxyChange): void {
  checkProxy(change.settings);
  const kept = withPasswords(
    state.proxyPasswords,
    change.passwords,
    state.settings.proxy,
    change.settings,
  );
  state.proxyPasswords.clear();
  for (const role of kept) state.proxyPasswords.add(role);
  const proxy = structuredClone(change.settings);
  for (const role of ROLES) proxy[role].has_password = kept.has(role);
  state.settings = { ...state.settings, proxy };
  state.settingsChanged();
}

/**
 * What the mock's test finds, by the proxy's host: one containing `refuse` turns the login away
 * unless a password is set, `slow` never answers, `nowhere` cannot be reached, and any other works.
 */
export function testProxy(state: MockState, test: ProxyTest): ProxyTestResult {
  checkProxy(test.settings);
  const proxy = test.settings;
  if (proxy.mode !== 'manual') return { outcome: 'works' };
  const role: ProxyRole = proxy.per_protocol ? 'https' : 'all';
  const host = proxy[role].host || proxy.socks.host;
  const hasPassword = withPasswords(
    state.proxyPasswords,
    test.passwords,
    state.settings.proxy,
    proxy,
  ).has(role);
  if (host.includes('refuse') && !hasPassword) return { outcome: 'login_refused' };
  if (host.includes('slow')) return { outcome: 'timed_out' };
  if (host.includes('nowhere')) {
    return { outcome: 'unreachable', reason: `could not connect to ${host}: connection refused` };
  }
  return { outcome: 'works' };
}

/** The proxy a believable office computer sets. */
export function systemProxy(): ProxySettings {
  return {
    ...defaultProxySettings(),
    mode: 'manual',
    all: { ...emptyServer(), host: 'proxy.corp.example', port: 3128 },
    exceptions: ['*.corp.example', '<local>'],
  };
}

export function exportSettings(state: MockState, path: string): void {
  state.files.set(path, {
    format_version: PROFILE_FORMAT_VERSION,
    app_version: version,
    exported_at: state.now(),
    settings: {
      ...structuredClone(state.settings),
      // Passwords stay on this computer, as in the core.
      proxy: withoutPasswords(state.settings.proxy),
    },
    queues: structuredClone(state.queues),
    addons: state.addons.filter((a) => a.state.type !== 'not_installed').map((a) => a.id),
  });
}

function withoutPasswords(proxy: ProxySettings): ProxySettings {
  const copy = structuredClone(proxy);
  for (const role of ROLES) copy[role].has_password = false;
  return copy;
}

function profileAt(state: MockState, path: string): SettingsProfile {
  const profile = state.files.get(path);
  if (!profile) {
    throw new BackendError(
      'That file is not a Wirecatch settings file. Choose a file made with Export settings.',
    );
  }
  return profile;
}

/**
 * The default folder of `profile` as the core would place it: a startup folder is refused and
 * replaced by Downloads, a folder outside the person's own folders is flagged.
 */
function importedFolder(profile: SettingsProfile): ImportedFolder {
  const requested = profile.settings.default_folder;
  const startup = /Start Menu|[\\/]\.config[\\/]autostart/i.test(requested);
  const used = startup ? '{Downloads}' : requested;
  return {
    setting: 'Default folder',
    requested,
    used,
    resolved: used,
    reason: startup
      ? `"${requested}" is a system or startup folder, where a downloaded file could run by itself. Folders from a settings file cannot be there; choose another folder.`
      : null,
    outside_user_folders: !used.startsWith('{'),
  };
}

function addonsToOffer(state: MockState, profile: SettingsProfile): AddonId[] {
  // Like the core: offered, never installed by the import itself.
  return profile.addons.filter(
    (id) => state.addons.find((a) => a.id === id)?.state.type === 'not_installed',
  );
}

export function previewImport(state: MockState, path: string): ImportPreview {
  const profile = profileAt(state, path);
  const known = new Set(state.queues.map((q) => q.id));
  return {
    folders: [importedFolder(profile)],
    queues_added: profile.queues.filter((q) => !known.has(q.id)).map((q) => q.name),
    queues_updated: profile.queues.filter((q) => known.has(q.id)).map((q) => q.name),
    addons_to_offer: addonsToOffer(state, profile),
  };
}

export function importSettings(state: MockState, path: string): void {
  const profile = profileAt(state, path);
  const known = new Set(state.queues.map((q) => q.id));
  const added = profile.queues.filter((q) => !known.has(q.id)).length;
  const folder = importedFolder(profile);
  state.settings = structuredClone(profile.settings);
  state.settings.default_folder = folder.used;
  state.queues = structuredClone(profile.queues);
  state.settingsChanged();
  state.queuesChanged();
  state.emit({
    type: 'settings_imported',
    data: {
      folders: [folder],
      queues_added: added,
      queues_updated: profile.queues.length - added,
      addons_to_offer: addonsToOffer(state, profile),
    },
  });
}
