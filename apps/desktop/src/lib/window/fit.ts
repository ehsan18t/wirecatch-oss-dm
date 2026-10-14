// A download window is only as tall as what it shows (D23): its frame measures the height its
// content needs and reports it to the page, which asks the desktop shell to resize the window.
// Without a reporter (the sample app's windows inside the page, tests) nothing is resized.
import { createContext } from 'svelte';

/** Receives the height, in CSS pixels, the window's content needs. */
export type FitWindow = (height: number) => void;

const [fitContext, setFitContext, hasFitContext] = createContext<FitWindow>();

/** Lets every window frame below this component report the height it needs. */
export function setFitWindow(fit: FitWindow): void {
  setFitContext(fit);
}

/** The reporter set by the page, or null when the window is not resized to fit. */
export function getFitWindow(): FitWindow | null {
  return hasFitContext() ? fitContext() : null;
}

/** What a window frame measures to tell the height it needs. */
export interface FrameParts {
  /** The whole frame: the title, the body and the buttons. */
  frame: number;
  /** The scrolling body as it is laid out now. */
  body: number;
  /** The content of the body, without its padding. */
  content: number;
  /** The body's padding, top and bottom together. */
  padding: number;
}

/**
 * The height a frame needs to show everything without scrolling, rounded up to whole pixels:
 * everything around the body as it is, and the body as tall as its content. Measuring around the
 * body, rather than adding up its neighbours, counts every border and gap in the frame.
 */
export function neededHeight(parts: FrameParts): number {
  const total = parts.frame - parts.body + parts.content + parts.padding;
  return Number.isFinite(total) && total > 0 ? Math.ceil(total) : 0;
}

/**
 * Calls `fit` with the needed height at most once per animation frame, and only when it
 * changed, so a view that settles over a few frames resizes the window once or twice, not on
 * every layout pass.
 */
export function fitReporter(
  fit: FitWindow,
  schedule: (run: () => void) => unknown = (run) => requestAnimationFrame(run),
): (height: number) => void {
  let last = 0;
  let wanted = 0;
  let waiting = false;
  return (height) => {
    wanted = height;
    if (waiting) return;
    waiting = true;
    schedule(() => {
      waiting = false;
      if (wanted > 0 && wanted !== last) {
        last = wanted;
        fit(wanted);
      }
    });
  };
}
