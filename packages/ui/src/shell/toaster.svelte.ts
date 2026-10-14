import { createContext } from 'svelte';
import { PausableTimers } from './toast-timers';

export type ToastTone = 'info' | 'success' | 'warning' | 'danger';

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: string;
  tone: ToastTone;
  message: string;
  /** What the toast is about, shown above the message: a file name. */
  title?: string | undefined;
  /** Milliseconds; 0 stays until dismissed. */
  duration: number;
  action?: ToastAction | undefined;
}

export interface ToastOptions {
  title?: string | undefined;
  duration?: number | undefined;
  /** One thing to do about it. A toast with an action stays longer; the same action must also be reachable elsewhere, as the toast goes away on its own. */
  action?: ToastAction | undefined;
}

/** How long a toast stays, by what it says; `action` for one that offers something to do. Longer for worse news, which takes longer to read and act on. */
export const TOAST_DURATION = {
  info: 4000,
  success: 4000,
  warning: 6000,
  danger: 8000,
  action: 10000,
} as const;

/** The most toasts on screen; a new one pushes the oldest out. */
export const MAX_TOASTS = 4;

/** What the live regions read: the newest toast only, so removing one never repeats an older one. */
export interface Announcement {
  id: string;
  text: string;
  assertive: boolean;
}

/**
 * The toast queue of one window. The app keeps one in its state and hands it to ToastRegion. Timers pause while the person hovers or focuses a toast (WCAG 2.2.1), and resume with the time they had left.
 */
export class Toaster {
  list = $state.raw<Toast[]>([]);
  announcement = $state.raw<Announcement | null>(null);
  #next = 0;
  /** The toast holding focus, kept when a new one would push it out. */
  #held: string | null = null;
  readonly #timers = new PausableTimers((id) => {
    this.dismiss(id);
  });

  show(tone: ToastTone, message: string, options: ToastOptions = {}): string {
    this.#next += 1;
    const id = `toast-${String(this.#next)}`;
    const duration =
      options.duration ?? (options.action ? TOAST_DURATION.action : TOAST_DURATION[tone]);
    while (this.list.length >= MAX_TOASTS) {
      const oldest = this.list.find((toast) => toast.id !== this.#held) ?? this.list[0];
      if (!oldest) break;
      this.dismiss(oldest.id);
    }
    const toast: Toast = {
      id,
      tone,
      message,
      title: options.title,
      duration,
      action: options.action,
    };
    this.list = [...this.list, toast];
    this.announcement = {
      id,
      text: options.title ? `${options.title}: ${message}` : message,
      assertive: tone === 'danger',
    };
    if (duration > 0) this.#timers.start(id, duration);
    return id;
  }

  info = (message: string, options?: ToastOptions) => this.show('info', message, options);
  success = (message: string, options?: ToastOptions) => this.show('success', message, options);
  warning = (message: string, options?: ToastOptions) => this.show('warning', message, options);
  danger = (message: string, options?: ToastOptions) => this.show('danger', message, options);

  /** Tells screen readers, politely, about a change the person made somewhere they are not reading (Space pausing several rows), without a toast on screen. */
  announce(text: string): void {
    this.#next += 1;
    this.announcement = { id: `announcement-${String(this.#next)}`, text, assertive: false };
  }

  dismiss(id: string): void {
    this.#timers.clear(id);
    if (this.#held === id) this.#held = null;
    this.list = this.list.filter((toast) => toast.id !== id);
  }

  /** Holds every countdown while the person reads or acts on a toast. */
  setPaused(paused: boolean): void {
    this.#timers.setPaused(paused);
  }

  hold(id: string | null): void {
    this.#held = id;
  }

  /** Stops every timer, for a window that is going away. */
  dispose(): void {
    this.#timers.dispose();
  }
}

const [getToaster, setToaster, hasToaster] = createContext<Toaster>();

/** Makes `toaster` the window's for every layer inside the calling component: a modal layer repeats its news inside itself, where screen readers still hear it. Call it once, while the window's root component starts. */
export function provideToaster(toaster: Toaster): void {
  setToaster(toaster);
}

/** The window's toaster, when the root component provided one. */
export const windowToaster = (): Toaster | null => (hasToaster() ? getToaster() : null);
