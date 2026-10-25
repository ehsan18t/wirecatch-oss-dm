// The proxy setting being edited (D71): unlike other settings it is saved as a whole with Save,
// after an optional Test connection, and once saved while downloads run it asks whether to
// restart them.

import type {
  ProxyPasswords,
  ProxyRole,
  ProxySettings,
  ProxyTestResult,
} from '@wirecatch/protocol';
import type { Backend } from '../backend';
import { defaultProxySettings, sameProxy } from '../proxy';
import type { Runner } from './runner';

/** What the form needs from the window it is shown in. */
export interface ProxyFormHost {
  backend: () => Backend;
  runner: Runner;
  /** The setting as saved, without passwords. */
  saved: () => ProxySettings | null;
  /** Downloads connecting or downloading now. */
  running: () => number;
}

const ROLES: readonly ProxyRole[] = ['all', 'http', 'https', 'ftp', 'socks'];

function same(a: ProxySettings, b: ProxySettings): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

export class ProxyForm {
  /** The setting as edited. */
  draft = $state<ProxySettings>(defaultProxySettings());
  /** Passwords typed or removed: a role left out keeps its saved one, `null` removes it. */
  passwords = $state<ProxyPasswords>({});
  /** What the last test found, until the setting changes. */
  result = $state<ProxyTestResult | null>(null);
  /** What the last save or copy did, said next to the buttons rather than in a note over them. */
  notice = $state<string | null>(null);
  testing = $state(false);
  saving = $state(false);
  /** Save waits for the person to say whether the running downloads restart. */
  askingRestart = $state(false);

  /** The saved setting the draft started from. */
  #base = $state<ProxySettings>(defaultProxySettings());
  readonly #host: ProxyFormHost;

  /** Starts from the default setting; `savedChanged` takes the saved one once it has loaded. */
  constructor(host: ProxyFormHost) {
    this.#host = host;
  }

  /** Whether anything differs from the saved setting. */
  get dirty(): boolean {
    return !same(this.draft, this.#base) || Object.keys(this.passwords).length > 0;
  }

  /** Starts again from the saved setting. */
  reset(): void {
    const saved = $state.snapshot(this.#host.saved()) ?? defaultProxySettings();
    this.#base = saved;
    this.draft = structuredClone(saved);
    this.passwords = {};
    this.result = null;
    this.notice = null;
  }

  /** The saved setting changed elsewhere (another window, an import): follow it unless edited. */
  savedChanged(): void {
    if (!this.dirty && !this.saving) this.reset();
  }

  /** Changes the draft; an earlier test no longer says anything about it. */
  edit(change: (draft: ProxySettings) => void): void {
    change(this.draft);
    this.result = null;
    this.notice = null;
  }

  /** Whether `role` will have a password: a typed one, or the saved one kept. */
  hasPassword(role: ProxyRole): boolean {
    const typed = this.passwords[role];
    if (typed === undefined) return this.keepsSaved(role);
    return typed !== null && typed !== '';
  }

  /** Whether the saved password of `role` stays: none was typed, and its proxy is the same one, since a password goes only to the proxy it was given for. */
  keepsSaved(role: ProxyRole): boolean {
    return (
      this.passwords[role] === undefined &&
      this.#base[role].has_password &&
      sameProxy(this.#base[role], this.draft[role])
    );
  }

  setPassword(role: ProxyRole, password: string | null): void {
    this.passwords = { ...this.passwords, [role]: password };
    this.result = null;
    this.notice = null;
  }

  async test(): Promise<void> {
    if (this.testing) return;
    this.testing = true;
    this.result = null;
    this.notice = null;
    try {
      const result = await this.#host.runner.run(() =>
        this.#host.backend().testProxy({
          settings: $state.snapshot(this.draft),
          passwords: $state.snapshot(this.passwords),
        }),
      );
      if (result) this.result = result;
    } finally {
      this.testing = false;
    }
  }

  /** Saves the draft; once it is saved, asks whether to restart the downloads running now. */
  async save(): Promise<void> {
    if (this.saving) return;
    this.saving = true;
    try {
      const ok = await this.#host.runner.succeeds(() =>
        this.#host.backend().setProxy({
          settings: $state.snapshot(this.draft),
          passwords: $state.snapshot(this.passwords),
          restart_running: false,
        }),
      );
      if (!ok) return;
      // What was saved, as the core will send it back: with the passwords it now keeps marked.
      const saved = structuredClone($state.snapshot(this.draft));
      for (const role of ROLES) saved[role].has_password = this.hasPassword(role);
      this.#base = saved;
      this.draft = structuredClone(saved);
      this.passwords = {};
      this.notice = 'Saved.';
      if (this.#host.running() > 0) this.askingRestart = true;
    } finally {
      this.saving = false;
    }
  }

  /** Restarts the running downloads on the setting just saved, which they then follow. */
  async restartRunning(): Promise<void> {
    this.askingRestart = false;
    const ok = await this.#host.runner.succeeds(() =>
      this.#host.backend().setProxy({
        settings: $state.snapshot(this.#base),
        passwords: {},
        restart_running: true,
      }),
    );
    if (ok) this.notice = 'Saved. The running downloads restarted on it.';
  }

  /** Fills the draft with the computer's proxy, to change from there. */
  async copyFromSystem(): Promise<void> {
    const found = await this.#host.runner.run(() => this.#host.backend().systemProxy());
    if (found === undefined) return;
    if (found === null) {
      this.notice = 'This computer sets no proxy to copy.';
      return;
    }
    this.edit((d) => {
      Object.assign(d, found, { mode: 'manual', ftp_direct: d.ftp_direct });
    });
    this.passwords = {};
    this.notice =
      'Copied from this computer’s settings. Passwords are not copied: enter one if the proxy asks for it.';
  }
}
