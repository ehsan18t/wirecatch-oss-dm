/** A rectangle in CSS pixels from the top-left of the page, which in a frameless window is the window's own top-left. */
export interface ButtonRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

/** How the maximize button looks while the OS has the pointer over it. */
export type MaximizeState = 'rest' | 'hover' | 'pressed';

/**
 * What the title bar needs from the window it sits in. The desktop app implements it with Tauri's window API (in its backend layer); this package never imports Tauri, and the extension has no title bar.
 */
export interface WindowControls {
  minimize: () => Promise<void>;
  /** Maximizes the window, or restores it when it is maximized. */
  toggleMaximize: () => Promise<void>;
  close: () => Promise<void>;
  isMaximized: () => Promise<boolean>;
  /** Calls `listener` after every resize, maximizing and restoring included; resolves to a function that stops listening. */
  onResized: (listener: () => void) => Promise<() => void>;
  /**
   * Told where the maximize button is, each time it moves or changes size, so Windows 11's snap layouts can open over it (the desktop app places its own hit area there). The app scales the CSS pixels to the window's.
   */
  setMaximizeButton?: (rect: ButtonRect) => void;
  /**
   * Calls `listener` with the maximize button's look while the OS holds the pointer over it: the button cannot see the pointer itself then, so it draws what it is told. Resolves to a function that stops listening.
   */
  onMaximizeState?: (listener: (state: MaximizeState) => void) => Promise<() => void>;
}

export type CaptionAction = 'minimize' | 'maximize' | 'close';
