/** Where the media button goes over a video or next to an audio player: pure geometry, in viewport pixels. */

export interface Box {
  top: number;
  left: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface Position {
  top: number;
  left: number;
  /** Sitting on top of the video, its bottom on the video's top edge, drawn as a tab of it. */
  attached: boolean;
}

/** Smaller videos are previews or ads: no button. */
export const MIN_VIDEO: Size = { width: 160, height: 90 };
/** Distance from the video's top right corner when the button sits inside it. */
export const INSET = 8;
/** Distance from the video's right edge when the button sits on top of it: clear of a player's rounded corner. */
const TAB_MARGIN = 12;
/** Height kept clear above a video's native controls. */
const CONTROLS_SPACE = 48;
/** Space between an audio player and the button above or below it. */
const AUDIO_GAP = 4;

/**
 * Where the button goes, in viewport pixels, or null when it should not show:
 * the element is too small, out of view, or there is no room left above a video's
 * own controls. On a video it sits on top of its top right corner, touching it, as a
 * tab of the player; with no room above it goes inside that corner, away from the
 * native controls at the bottom. An audio player is all controls, so the button sits
 * just above it (or below, when there is no room above).
 */
export function placeButton(
  target: Box,
  kind: { audio: boolean; controls: boolean; covered?: boolean },
  viewport: Size,
  size: Size,
): Position | null {
  if (!isVisible(target, viewport)) return null;
  if (!kind.audio && (target.width < MIN_VIDEO.width || target.height < MIN_VIDEO.height)) {
    return null;
  }
  const place = kind.audio ? besideAudio(target, size) : overVideo(target, kind, viewport, size);
  if (place === null) return null;
  const left = Math.max(0, Math.min(place.left, viewport.width - size.width));
  return { top: Math.round(place.top), left: Math.round(left), attached: place.attached };
}

/** True when the element has an area and some of it is inside the viewport. */
function isVisible(target: Box, viewport: Size): boolean {
  return (
    target.width > 0 &&
    target.height > 0 &&
    target.bottom > 0 &&
    target.right > 0 &&
    target.top < viewport.height &&
    target.left < viewport.width
  );
}

/** Just above an audio player, right-aligned with it, or below it when there is no room above. */
function besideAudio(target: Box, size: Size): Position {
  const above = target.top - size.height - AUDIO_GAP;
  return {
    top: above < 0 ? target.bottom + AUDIO_GAP : above,
    left: target.right - size.width,
    attached: false,
  };
}

/**
 * On top of a video's top right corner, else inside it, or null when its own controls leave no
 * room there. A video whose top a page's fixed bar covers gets the button inside, below the bar.
 */
function overVideo(
  target: Box,
  kind: { controls: boolean; covered?: boolean },
  viewport: Size,
  size: Size,
): Position | null {
  const right = Math.min(target.right, viewport.width);
  const above = target.top - size.height;
  if (above >= 0 && kind.covered !== true) {
    return { top: above, left: right - size.width - TAB_MARGIN, attached: true };
  }
  const top = Math.max(target.top, 0) + INSET;
  const room = Math.min(target.bottom, viewport.height) - (kind.controls ? CONTROLS_SPACE : INSET);
  if (top + size.height > room) return null;
  return { top, left: right - size.width - INSET, attached: false };
}
