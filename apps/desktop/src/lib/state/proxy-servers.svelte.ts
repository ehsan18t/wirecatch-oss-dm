// The saved proxy servers (D73), as the core last sent them, and saving, testing and deleting them.

import type {
  ProxyServer,
  ProxyServerDraft,
  ProxyServerId,
  ProxySettings,
  ProxyTestResult,
  ProxyUse,
  Queue,
} from '@wirecatch/protocol';
import type { Backend } from '../backend';
import { shownName } from '../proxy';
import type { Runner } from './runner';
import type { Toasts } from './toasts.svelte';

/** What the profiles need from the window they are shown in. */
export interface ProxyServersHost {
  backend: () => Backend;
  runner: Runner;
  toasts: Toasts;
  /** The app's proxy, once the settings have loaded. */
  app: () => ProxySettings | undefined;
  queues: () => readonly Queue[];
}

/** What uses a profile: the app's proxy with the queues that follow it, and the queues with it as their own. */
export interface ServerUsers {
  app: boolean;
  /** The queues that follow the app, while the app uses it. */
  following: Queue[];
  /** The queues with it as their own proxy. */
  queues: Queue[];
}

/** "Used by the global setting (Main, Overnight) and Work files", or "Not used yet". */
export function usersLine(users: ServerUsers): string {
  const names = (queues: readonly Queue[]) => queues.map((q) => q.settings.name).join(', ');
  const parts: string[] = [];
  if (users.app) {
    parts.push(
      users.following.length === 0
        ? 'the global setting'
        : `the global setting (${names(users.following)})`,
    );
  }
  if (users.queues.length > 0) parts.push(names(users.queues));
  return parts.length === 0 ? 'Not used yet' : `Used by ${parts.join(' and ')}`;
}

export class ProxyServers {
  list = $state<ProxyServer[]>([]);
  readonly #host: ProxyServersHost;

  constructor(host: ProxyServersHost) {
    this.#host = host;
  }

  byId(id: ProxyServerId): ProxyServer | undefined {
    return this.list.find((p) => p.id === id);
  }

  /** What uses profile `id`: the app's proxy, and each queue whose own proxy it is. */
  usersOf(id: ProxyServerId): ServerUsers {
    const uses = (proxy: ProxyUse) => proxy.kind === 'server' && proxy.id === id;
    const app = this.#host.app();
    const byApp = app !== undefined && uses(app.proxy);
    const queues = this.#host.queues();
    return {
      app: byApp,
      following: byApp ? queues.filter((q) => q.settings.proxy.source === 'global') : [],
      queues: queues.filter(
        (q) => q.settings.proxy.source === 'own' && uses(q.settings.proxy.value),
      ),
    };
  }

  /** Saves a profile, new when `id` is null; its id once saved. The core tests it in the background. */
  async save(
    id: ProxyServerId | null,
    draft: ProxyServerDraft,
  ): Promise<ProxyServerId | undefined> {
    const backend = this.#host.backend();
    if (id === null) return this.#host.runner.run(() => backend.createProxyServer(draft));
    const saved = await this.#host.runner.succeeds(() => backend.updateProxyServer(id, draft));
    return saved ? id : undefined;
  }

  /** Sends one request through the profile as `draft` has it; undefined when it could not be tried. */
  test(id: ProxyServerId | null, draft: ProxyServerDraft): Promise<ProxyTestResult | undefined> {
    return this.#host.runner.run(() => this.#host.backend().testProxyServer(id, draft));
  }

  /** Deletes profile `id`; what used it uses `replacement` instead. */
  async remove(id: ProxyServerId, replacement: ProxyUse): Promise<boolean> {
    const name = this.byId(id);
    const ok = await this.#host.runner.succeeds(() =>
      this.#host.backend().deleteProxyServer(id, replacement),
    );
    if (ok) this.#host.toasts.notify(`Deleted “${name ? shownName(name.settings) : 'the proxy'}”.`);
    return ok;
  }

  /** Saves a profile for each server the computer's proxy names, and says what it did. */
  async copyFromSystem(): Promise<void> {
    const before = this.list.map((p) => p.id);
    const ids = await this.#host.runner.run(() => this.#host.backend().copySystemProxy());
    if (ids === undefined) return;
    const added = ids.filter((id) => !before.includes(id));
    if (ids.length === 0) {
      this.#host.toasts.notify('No system proxy is set, so there is nothing to copy.');
      return;
    }
    if (added.length === 0) {
      this.#host.toasts.notify('The system proxy is already saved.');
      return;
    }
    // A user name comes along; its password stays with the computer's setting.
    const login = added.some((id) => (this.byId(id)?.settings.user ?? '') !== '');
    const saved =
      added.length === 1
        ? 'Saved the system proxy.'
        : `Saved ${String(added.length)} system proxies.`;
    this.#host.toasts.notify(
      login ? `${saved} Passwords aren’t copied; edit the proxy to add one.` : saved,
    );
  }
}
