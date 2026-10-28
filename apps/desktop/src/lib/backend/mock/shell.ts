// What a browser tab answers in place of the desktop shell: no window to control or frame to report, no system notifications or signals, and a fixed sample accent colour.
import type {
  ButtonArea,
  Reveal,
  SystemAppearance,
  WindowAction,
  WindowFrame,
} from '@wirecatch/protocol';
import type { ShellSignal, Unsubscribe } from '../types';
import { SAMPLE_SYSTEM_ACCENT } from './data';

export abstract class MockShell {
  /** Answers as the mock core answers every call, with its latency and errors. */
  protected abstract answer<T>(work: () => T): Promise<T>;

  showSetAsideList() {
    // The sample app has no folder to show; the call answers as the shell's would.
    return this.answer(() => undefined);
  }

  takeReveal() {
    // A browser tab gets no system notifications to click.
    return this.answer((): Reveal | null => null);
  }
  onReveal(_listener: (reveal: Reveal) => void) {
    return this.answer((): Unsubscribe => () => undefined);
  }

  onShellSignal(_listener: (signal: ShellSignal) => void) {
    // A browser tab has no desktop shell to signal anything.
    return this.answer((): Unsubscribe => () => undefined);
  }

  systemAppearance() {
    return this.answer((): SystemAppearance => ({ accent: SAMPLE_SYSTEM_ACCENT }));
  }

  onSystemAppearance(_listener: (appearance: SystemAppearance) => void) {
    // The sample system never changes its accent colour.
    return this.answer((): Unsubscribe => () => undefined);
  }

  controlWindow(_action: WindowAction) {
    // A browser tab cannot minimize, maximize or close itself.
    return this.answer(() => undefined);
  }

  windowFrame() {
    return this.answer((): WindowFrame => ({
      maximized: false,
      focused: true,
      maximize_hovered: false,
    }));
  }

  onWindowFrame(_listener: (frame: WindowFrame) => void) {
    return this.answer((): Unsubscribe => () => undefined);
  }

  setMaximizeButton(_area: ButtonArea | null) {
    // The page keeps the pointer over its own button: there are no snap layouts to offer.
    return this.answer(() => undefined);
  }
}
