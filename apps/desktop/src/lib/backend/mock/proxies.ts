// What the mock core does with proxy servers (D73): save, test and delete them, and Copy from system.

import {
  MAX_PROXY_NAME_CHARS,
  type ProxyServer,
  type ProxyServerDraft,
  type ProxyServerId,
  type ProxyServerSettings,
  type ProxyTestResult,
  type ProxyUse,
} from '@wirecatch/protocol';
import { sameProxy, shownName } from '../../proxy';
import { BackendError } from '../types';
import type { MockState } from './state';

function invalid(message: string): BackendError {
  return new BackendError(message, 'invalid_request');
}

function find(state: MockState, id: ProxyServerId): ProxyServer {
  const profile = state.proxyServers.find((p) => p.id === id);
  if (!profile)
    throw new BackendError('That proxy no longer exists. It may have been deleted.', 'not_found');
  return profile;
}

/** The profile trimmed, or the core's refusal of one that cannot work or takes a saved name. */
function checked(
  settings: ProxyServerSettings,
  others: readonly ProxyServer[],
): ProxyServerSettings {
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
function keepsPassword(draft: ProxyServerDraft, saved: ProxyServer | undefined): boolean {
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
function testResult(settings: ProxyServerSettings, hasPassword: boolean): ProxyTestResult {
  const host = settings.host;
  if (host.includes('refuse') && !hasPassword) return { outcome: 'login_refused' };
  if (host.includes('slow')) return { outcome: 'timed_out' };
  if (host.includes('nowhere')) {
    return { outcome: 'unreachable', reason: `could not connect to ${host}: connection refused` };
  }
  return { outcome: 'works' };
}

export function createProxyServer(state: MockState, draft: ProxyServerDraft): ProxyServerId {
  const settings = checked(draft.settings, state.proxyServers);
  const hasPassword = keepsPassword({ ...draft, settings }, undefined);
  const id = state.newProxyId();
  // The core tests a new profile in the background; the mock's answer is at hand.
  state.proxyServers.push({
    id,
    settings,
    has_password: hasPassword,
    last_test: { at: state.now(), result: testResult(settings, hasPassword) },
  });
  state.proxyServersChanged();
  return id;
}

export function updateProxyServer(
  state: MockState,
  id: ProxyServerId,
  draft: ProxyServerDraft,
): void {
  const saved = find(state, id);
  const settings = checked(
    draft.settings,
    state.proxyServers.filter((p) => p.id !== id),
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
  state.proxyServersChanged();
}

export function deleteProxyServer(
  state: MockState,
  id: ProxyServerId,
  replacement: ProxyUse,
): void {
  find(state, id);
  if (replacement.kind === 'server') {
    if (replacement.id === id) {
      throw invalid('Choose another proxy for what uses the one being deleted.');
    }
    find(state, replacement.id);
  }
  const uses = (proxy: ProxyUse) => proxy.kind === 'server' && proxy.id === id;
  state.proxyServers = state.proxyServers.filter((p) => p.id !== id);
  state.proxyServersChanged();
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

export function testProxyServer(
  state: MockState,
  id: ProxyServerId | null,
  draft: ProxyServerDraft,
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
    state.proxyServersChanged();
  }
  return result;
}

/** The proxy a believable office computer sets, saved once as a profile without a name. */
export function copySystemProxy(state: MockState): ProxyServerId[] {
  const system: ProxyServerSettings = {
    name: '',
    kind: 'http',
    host: 'proxy.corp.example',
    port: 3128,
    user: '',
  };
  const saved = state.proxyServers.find((p) => sameProxy(p.settings, system));
  if (saved) return [saved.id];
  return [createProxyServer(state, { settings: system })];
}
