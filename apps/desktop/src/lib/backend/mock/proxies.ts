// What the mock core does with proxy profiles (D73): save, test and delete them, and Copy from system.

import {
  MAX_PROXY_NAME_CHARS,
  type ProxyProfile,
  type ProxyProfileDraft,
  type ProxyProfileId,
  type ProxyProfileSettings,
  type ProxyTestResult,
  type ProxyUse,
} from '@wirecatch/protocol';
import { sameProxy, shownName } from '../../proxy';
import { BackendError } from '../types';
import type { MockState } from './state';

function invalid(message: string): BackendError {
  return new BackendError(message, 'invalid_request');
}

function find(state: MockState, id: ProxyProfileId): ProxyProfile {
  const profile = state.proxyProfiles.find((p) => p.id === id);
  if (!profile)
    throw new BackendError('That proxy no longer exists. It may have been deleted.', 'not_found');
  return profile;
}

/** The profile trimmed, or the core's refusal of one that cannot work or takes a saved name. */
function checked(
  settings: ProxyProfileSettings,
  others: readonly ProxyProfile[],
): ProxyProfileSettings {
  const profile = { ...settings, name: settings.name.trim(), host: settings.host.trim() };
  if (profile.name.length > MAX_PROXY_NAME_CHARS) {
    throw invalid(`Proxy names can be at most ${String(MAX_PROXY_NAME_CHARS)} characters.`);
  }
  if (profile.host === '') throw invalid('Enter the proxy server’s name or address.');
  if (!/^[\w.:-]+$/.test(profile.host) || profile.host.includes('://')) {
    throw invalid(
      `"${profile.host}" is not a proxy server name. Enter a host name or address only, without http:// or a port.`,
    );
  }
  if (profile.port === 0) throw invalid(`Enter the port of the proxy server ${profile.host}.`);
  const shown = shownName(profile).toLowerCase();
  if (others.some((p) => shownName(p.settings).toLowerCase() === shown)) {
    throw invalid(
      `A proxy named "${shownName(profile)}" is already saved. Give this one another name.`,
    );
  }
  return profile;
}

/** Whether the profile will have a password after `draft`, as the core's rule keeps one only with its proxy. */
function keepsPassword(draft: ProxyProfileDraft, saved: ProxyProfile | undefined): boolean {
  if (draft.password === undefined) {
    return saved !== undefined && saved.has_password && sameProxy(saved.settings, draft.settings);
  }
  if (draft.password === null || draft.password === '') return false;
  if (draft.settings.user.trim() === '') {
    throw invalid('Enter the user name that goes with the proxy password.');
  }
  return true;
}

/**
 * What the mock's test finds, by the proxy's host: one containing `refuse` turns the login away
 * unless a password is set, `slow` never answers, `nowhere` cannot be reached, and any other works.
 */
function testResult(settings: ProxyProfileSettings, hasPassword: boolean): ProxyTestResult {
  const host = settings.host;
  if (host.includes('refuse') && !hasPassword) return { outcome: 'login_refused' };
  if (host.includes('slow')) return { outcome: 'timed_out' };
  if (host.includes('nowhere')) {
    return { outcome: 'unreachable', reason: `could not connect to ${host}: connection refused` };
  }
  return { outcome: 'works' };
}

export function createProxyProfile(state: MockState, draft: ProxyProfileDraft): ProxyProfileId {
  const settings = checked(draft.settings, state.proxyProfiles);
  const hasPassword = keepsPassword({ ...draft, settings }, undefined);
  const id = state.newProxyId();
  // The core tests a new profile in the background; the mock's answer is at hand.
  state.proxyProfiles.push({
    id,
    settings,
    has_password: hasPassword,
    last_test: { at: state.now(), result: testResult(settings, hasPassword) },
  });
  state.proxyProfilesChanged();
  return id;
}

export function updateProxyProfile(
  state: MockState,
  id: ProxyProfileId,
  draft: ProxyProfileDraft,
): void {
  const saved = find(state, id);
  const settings = checked(
    draft.settings,
    state.proxyProfiles.filter((p) => p.id !== id),
  );
  const hasPassword = keepsPassword({ ...draft, settings }, saved);
  const retest = !sameProxy(saved.settings, settings) || draft.password !== undefined;
  Object.assign(saved, {
    settings,
    has_password: hasPassword,
    last_test: retest
      ? { at: state.now(), result: testResult(settings, hasPassword) }
      : saved.last_test,
  });
  state.proxyProfilesChanged();
}

export function deleteProxyProfile(
  state: MockState,
  id: ProxyProfileId,
  replacement: ProxyUse,
): void {
  find(state, id);
  if (replacement.kind === 'profile') {
    if (replacement.id === id) {
      throw invalid('Choose another proxy for what uses the one being deleted.');
    }
    find(state, replacement.id);
  }
  const uses = (proxy: ProxyUse) => proxy.kind === 'profile' && proxy.id === id;
  state.proxyProfiles = state.proxyProfiles.filter((p) => p.id !== id);
  state.proxyProfilesChanged();
  let queuesChanged = false;
  for (const queue of state.queues) {
    if (queue.settings.proxy.source === 'own' && uses(queue.settings.proxy.value)) {
      queue.settings.proxy = { source: 'own', value: replacement };
      queuesChanged = true;
    }
  }
  if (queuesChanged) state.queuesChanged();
  if (uses(state.settings.proxy.proxy)) {
    state.settings = {
      ...state.settings,
      proxy: { ...state.settings.proxy, proxy: replacement },
    };
    state.settingsChanged();
  }
}

export function testProxyProfile(
  state: MockState,
  id: ProxyProfileId | null,
  draft: ProxyProfileDraft,
): ProxyTestResult {
  const saved = id === null ? undefined : find(state, id);
  const settings = checked(draft.settings, []);
  const hasPassword = keepsPassword({ ...draft, settings }, saved);
  const result = testResult(settings, hasPassword);
  if (
    saved &&
    draft.password === undefined &&
    JSON.stringify(saved.settings) === JSON.stringify(settings)
  ) {
    saved.last_test = { at: state.now(), result };
    state.proxyProfilesChanged();
  }
  return result;
}

/** The proxy a believable office computer sets, saved once as a profile without a name. */
export function copySystemProxy(state: MockState): ProxyProfileId[] {
  const system: ProxyProfileSettings = {
    name: '',
    kind: 'http',
    host: 'proxy.corp.example',
    port: 3128,
    user: '',
  };
  const saved = state.proxyProfiles.find((p) => sameProxy(p.settings, system));
  if (saved) return [saved.id];
  return [createProxyProfile(state, { settings: system })];
}
