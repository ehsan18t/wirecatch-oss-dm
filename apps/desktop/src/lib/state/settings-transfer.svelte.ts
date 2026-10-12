// Moving settings to and from a file: export, and an import the person sees before it applies.

import { PRODUCT_SLUG } from '@wirecatch/product';
import type { AppSettings, ImportPreview, ImportSummary, Queue } from '@wirecatch/protocol';
import type { Backend } from '../backend';
import type { Runner } from './runner';

/** The name a settings export suggests. */
const EXPORT_FILE_NAME = `${PRODUCT_SLUG}-settings.json`;

/** Where an import puts what it read back. */
export interface SettingsTransferHost {
  backend: () => Backend;
  runner: Runner;
  imported: (settings: AppSettings | undefined, queues: Queue[] | undefined) => void;
}

export class SettingsTransfer {
  /** What the last settings import changed, shown in the import dialog. */
  lastImport = $state<ImportSummary | null>(null);
  /**
   * A settings file chosen for import and what importing it would change. The import dialog
   * shows this and applies the file only when the person says so.
   */
  importPreview = $state<{ path: string; preview: ImportPreview } | null>(null);
  readonly #host: SettingsTransferHost;

  constructor(host: SettingsTransferHost) {
    this.#host = host;
  }

  async exportSettings(): Promise<string | null> {
    const { backend, runner } = this.#host;
    const path = await runner.run(() => backend().pickSaveFile(EXPORT_FILE_NAME));
    if (!path) return null;
    const ok = await runner.succeeds(() => backend().exportSettings(path));
    return ok ? path : null;
  }

  /**
   * Asks for a settings file and reads what importing it would change, without changing
   * anything yet: folders as they would be here, queues, add-ons. `importSettings` applies it.
   */
  async previewImport(): Promise<ImportPreview | null> {
    const { backend, runner } = this.#host;
    const path = await runner.run(() => backend().pickOpenFile());
    if (!path) return null;
    const preview = await runner.run(() => backend().previewImport(path));
    if (!preview) return null;
    this.importPreview = { path, preview };
    return preview;
  }

  /** Leaves the previewed settings file unimported. */
  cancelImport(): void {
    this.importPreview = null;
  }

  /** Applies the settings file previewed with `previewImport`; returns its path once done. */
  async importSettings(): Promise<string | null> {
    const { backend, runner } = this.#host;
    const path = this.importPreview?.path;
    if (!path) return null;
    // The core reports what changed with `settings_imported`.
    this.lastImport = null;
    const ok = await runner.succeeds(() => backend().importSettings(path));
    this.importPreview = null;
    if (!ok) return null;
    // The core also sends change events; reading back makes sure nothing is missed.
    const [settings, queues] = await Promise.all([
      runner.run(() => backend().getSettings()),
      runner.run(() => backend().listQueues()),
    ]);
    this.#host.imported(settings, queues);
    return path;
  }
}
