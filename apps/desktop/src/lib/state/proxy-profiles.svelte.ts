// The saved proxy profiles (D73), as the core last sent them, and saving, testing and deleting them.

import type {
  ProxyProfile,
  ProxyProfileDraft,
  ProxyProfileId,
  ProxySettings,
  ProxyTestResult,
  ProxyUse,
  Queue,
} from '@wirecatch/protocol';
import type { Backend } from '../backend';
import { queueProxy, shownName } from '../proxy';
import type { Runner } from './runner';
import type { Toasts } from './toasts.svelte';

/** What the profiles need from the window they are shown in. */
export interface ProxyProfilesHost {
  backend: () => Backend;
  runner: Runner;
  toasts: Toasts;
  /** The app's proxy, once the settings have loaded. */
  app: () => ProxySettings | undefined;
  queues: () => readonly Queue[];
}

/** What uses a profile: the app's proxy, and the queues with it as their own. */
export interface ProfileUsers {
  app: boolean;
  queues: Queue[];
}

export class ProxyProfiles {
  list = $state<ProxyProfile[]>([]);
  readonly #host: ProxyProfilesHost;

  constructor(host: ProxyProfilesHost) {
    this.#host = host;
  }

  byId(id: ProxyProfileId): ProxyProfile | undefined {
    return this.list.find((p) => p.id === id);
  }

  /** What uses profile `id`: the app's proxy, and each queue whose own proxy it is. */
  usersOf(id: ProxyProfileId): ProfileUsers {
    const uses = (proxy: ProxyUse) => proxy.kind === 'profile' && proxy.id === id;
    const app = this.#host.app();
    return {
      app: app !== undefined && uses(app.proxy),
      queues: this.#host
        .queues()
        .filter((q) => q.settings.proxy.source === 'own' && uses(q.settings.proxy.value)),
    };
  }

  /** The queues whose downloads go through profile `id`, their own or the app's. */
  queuesThrough(id: ProxyProfileId): Queue[] {
    const app = this.#host.app();
    if (!app) return [];
    return this.#host.queues().filter((q) => {
      const proxy = queueProxy(q, app);
      return proxy.kind === 'profile' && proxy.id === id;
    });
  }

  /** Saves a profile, new when `id` is null; its id once saved. The core tests it in the background. */
  async save(
    id: ProxyProfileId | null,
    draft: ProxyProfileDraft,
  ): Promise<ProxyProfileId | undefined> {
    const backend = this.#host.backend();
    if (id === null) return this.#host.runner.run(() => backend.createProxyProfile(draft));
    const saved = await this.#host.runner.succeeds(() => backend.updateProxyProfile(id, draft));
    return saved ? id : undefined;
  }

  /** Sends one request through the profile as `draft` has it; undefined when it could not be tried. */
  test(id: ProxyProfileId | null, draft: ProxyProfileDraft): Promise<ProxyTestResult | undefined> {
    return this.#host.runner.run(() => this.#host.backend().testProxyProfile(id, draft));
  }

  /** Deletes profile `id`; what used it uses `replacement` instead. */
  async remove(id: ProxyProfileId, replacement: ProxyUse): Promise<boolean> {
    const name = this.byId(id);
    const ok = await this.#host.runner.succeeds(() =>
      this.#host.backend().deleteProxyProfile(id, replacement),
    );
    if (ok) this.#host.toasts.notify(`Deleted the proxy ${name ? shownName(name.settings) : ''}.`);
    return ok;
  }

  /** Saves a profile for each server the computer's proxy names, and says what it did. */
  async copyFromSystem(): Promise<void> {
    const before = this.list.map((p) => p.id);
    const ids = await this.#host.runner.run(() => this.#host.backend().copySystemProxy());
    if (ids === undefined) return;
    const added = ids.filter((id) => !before.includes(id)).length;
    let note: string;
    if (ids.length === 0) note = 'This computer sets no proxy to copy.';
    else if (added === 0) note = 'The computer’s proxy is already saved.';
    else
      note =
        added === 1
          ? 'Saved the computer’s proxy. Its password is not copied: edit it to enter one.'
          : `Saved the computer’s ${String(added)} proxies. Passwords are not copied: edit one to enter it.`;
    this.#host.toasts.notify(note);
  }
}
