// Proxies (D73) as the windows show them: profiles and their parts in words, which proxy a queue uses, and what a test found.

import {
  FOLLOW_APP,
  type ProxyTestRun,
  type ProxyKind,
  type ProxyProfile,
  type ProxyProfileSettings,
  type ProxySettings,
  type ProxyTestResult,
  type ProxyUse,
  type Queue,
  type QueueSettings,
} from '@wirecatch/protocol';
import { whenSaid } from './format';

/** The app's proxy on a fresh install: the computer's, with its bypass list. */
export function defaultProxySettings(): ProxySettings {
  return {
    proxy: { kind: 'system' },
    exceptions: { system: true, use_hosts: true, hosts: [] },
    ftp_direct: false,
  };
}

/** A new queue's proxy and exceptions: the app's. */
export const FOLLOWS_APP_PROXY = {
  proxy: FOLLOW_APP,
  exceptions: FOLLOW_APP,
} as const satisfies Pick<QueueSettings, 'proxy' | 'exceptions'>;

/** `host:port`, with an IPv6 address in brackets, as the core writes it. */
export function proxyAddress(profile: ProxyProfileSettings): string {
  const host = profile.host.includes(':') ? `[${profile.host}]` : profile.host;
  return `${host}:${String(profile.port)}`;
}

/** A profile's name, or its address when it has none, as the core shows it. */
export function shownName(profile: ProxyProfileSettings): string {
  const name = profile.name.trim();
  return name === '' ? proxyAddress(profile) : name;
}

/** Whether `a` and `b` name the same proxy and login, so a password saved for one is the other's, as the core decides. */
export function sameProxy(a: ProxyProfileSettings, b: ProxyProfileSettings): boolean {
  return (
    a.kind === b.kind &&
    a.host.trim().toLowerCase() === b.host.trim().toLowerCase() &&
    a.port === b.port &&
    a.user === b.user
  );
}

/** The proxy `queue`'s downloads go through: its own, or the app's when it follows it. */
export function queueProxy(queue: Queue, app: ProxySettings): ProxyUse {
  return queue.settings.proxy.source === 'own' ? queue.settings.proxy.value : app.proxy;
}

/** The profile `proxy` names, if it names one that is saved. */
export function profileOf(
  proxy: ProxyUse,
  profiles: readonly ProxyProfile[],
): ProxyProfile | undefined {
  return proxy.kind === 'profile' ? profiles.find((p) => p.id === proxy.id) : undefined;
}

/** `proxy` in words. */
export function proxyName(proxy: ProxyUse, profiles: readonly ProxyProfile[]): string {
  switch (proxy.kind) {
    case 'system':
      return 'The computer’s proxy';
    case 'direct':
      return 'No proxy';
    case 'profile': {
      const profile = profileOf(proxy, profiles);
      return profile ? shownName(profile.settings) : 'A deleted proxy';
    }
  }
}

const TEST_OUTCOME: Record<ProxyTestResult['outcome'], string> = {
  works: 'Works',
  login_refused: 'Login refused',
  unreachable: 'Could not be reached',
  timed_out: 'Did not answer in time',
};

/** A profile's last test in a few words, with when it ran: "Works, tested today at 09:30." */
export function lastTestLine(
  test: ProxyTestRun | null,
  now: Date,
): { tone: 'success' | 'danger' | 'warning' | 'neutral'; text: string } {
  if (!test) return { tone: 'neutral', text: 'Not tested yet.' };
  const text = `${TEST_OUTCOME[test.result.outcome]}, tested ${whenSaid(test.at, now)}.`;
  const outcome = test.result.outcome;
  if (outcome === 'works') return { tone: 'success', text };
  return { tone: outcome === 'timed_out' ? 'warning' : 'danger', text };
}

export const KIND_OPTIONS: readonly { value: ProxyKind; label: string }[] = [
  { value: 'http', label: 'HTTP' },
  { value: 'https', label: 'HTTPS' },
  { value: 'socks5', label: 'SOCKS5' },
];

/** The highest port a server can listen on. */
export const MAX_PORT = 65_535;
/** The port SOCKS proxies listen on (RFC 1928). */
const SOCKS_PORT = 1080;
/** The port HTTP proxies most often listen on. */
const HTTP_PROXY_PORT = 8080;

/** The port a proxy of `kind` most often listens on, for a port not entered yet. */
export function usualPort(kind: ProxyKind): number {
  return kind === 'socks5' ? SOCKS_PORT : HTTP_PROXY_PORT;
}

/** What a test found, in words, and how it reads; `loginEntered` says whether a login was given to refuse. */
export function testResultText(
  result: ProxyTestResult,
  loginEntered: boolean,
): {
  tone: 'success' | 'danger' | 'warning';
  text: string;
} {
  switch (result.outcome) {
    case 'works':
      return { tone: 'success', text: 'The proxy works. A test request went through it.' };
    case 'login_refused':
      return {
        tone: 'danger',
        text: loginEntered
          ? 'The proxy refused the login. Check the user name and password.'
          : 'The proxy needs a login. Enter its user name and password.',
      };
    case 'unreachable':
      return {
        tone: 'danger',
        text: `The proxy could not be reached: ${result.reason.replace(/\.$/, '')}.`,
      };
    case 'timed_out':
      return {
        tone: 'warning',
        text: 'The proxy did not answer in time. Check its address and port.',
      };
  }
}
