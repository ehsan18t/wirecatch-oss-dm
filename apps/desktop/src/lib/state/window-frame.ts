// The window calls a frameless title bar makes (D36), over the backend: the caption buttons, whether the window is maximized, and on Windows 11 where the maximize button is, so the shell can offer the snap layouts over it and say when the pointer is there.

import type { WindowFrame } from '@wirecatch/protocol';
import type { ButtonRect, MaximizeState, WindowControls } from '@wirecatch/ui';
import type { Backend, Unsubscribe } from '../backend';

/** Told each new state of the window's frame. */
type FrameListener = (frame: WindowFrame) => void;

/**
 * The title bar's route to its window. The backend may still be loading when the title bar first asks (the window draws before the core answers), so each call waits for it.
 */
export class WindowFrameControls implements WindowControls {
  readonly #backend: () => Promise<Backend>;

  /** Takes the function that gives the backend once it has loaded. */
  constructor(backend: () => Promise<Backend>) {
    this.#backend = backend;
  }

  async minimize(): Promise<void> {
    await (await this.#backend()).controlWindow('minimize');
  }

  async toggleMaximize(): Promise<void> {
    await (await this.#backend()).controlWindow('toggle_maximize');
  }

  async close(): Promise<void> {
    await (await this.#backend()).controlWindow('close');
  }

  async isMaximized(): Promise<boolean> {
    return (await (await this.#backend()).windowFrame()).maximized;
  }

  /** The shell sends a new frame on every resize, among other changes; only a change of maximized calls `listener`. */
  onResized(listener: () => void): Promise<() => void> {
    let maximized: boolean | null = null;
    return this.#follow((frame) => {
      if (frame.maximized === maximized) return;
      maximized = frame.maximized;
      listener();
    });
  }

  setMaximizeButton(rect: ButtonRect): void {
    const { x, y, width, height } = rect;
    // Snap layouts are an extra: the button works without them, so a failure is not the person's to see.
    void this.#backend()
      .then((backend) => backend.setMaximizeButton({ x, y, width, height }))
      .catch(() => undefined);
  }

  onMaximizeState(listener: (state: MaximizeState) => void): Promise<() => void> {
    return this.#follow((frame) => {
      listener(frame.maximize_hovered ? 'hover' : 'rest');
    });
  }

  async #follow(listener: FrameListener): Promise<Unsubscribe> {
    return (await this.#backend()).onWindowFrame(listener);
  }
}
