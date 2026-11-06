/** Where the media button goes on a video or an audio player: pure geometry, in viewport pixels. */

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

/** Which side of the media's edge the button, a tab of the player, sits on. */
export type Edge = 'above' | 'below';

export interface Position {
  top: number;
  left: number;
  edge: Edge;
}

/** Smaller videos are previews or ads: no button. */
export const MIN_VIDEO: Size = { width: 160, height: 90 };
/** Distance from the media's right edge: clear of a player's rounded corner. */
const TAB_MARGIN = 12;
/** Height kept clear above a video's native controls. */
const CONTROLS_SPACE = 48;

/**
 * Where the button goes, in viewport pixels, or null when it should not show: the element is too
 * small, out of view, or a video's own controls leave no room. The button is always a tab of the
 * player, 12 pixels in from its right edge: on top of its top edge, touching it, or, with no room
 * above or a page's fixed bar over the video's top, hanging from that edge (inside a video, under
 * an audio player).
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
  const place = kind.audio ? onAudio(target, size) : onVideo(target, kind, viewport, size);
  if (place === null) return null;
  const left = Math.max(0, Math.min(place.left, viewport.width - size.width));
  return { top: Math.round(place.top), left: Math.round(left), edge: place.edge };
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

/** On top of an audio player, or under it when there is no room above: the player is all controls. */
function onAudio(target: Box, size: Size): Position {
  const above = target.top - size.height;
  const left = target.right - size.width - TAB_MARGIN;
  return above < 0
    ? { top: target.bottom, left, edge: 'below' }
    : { top: above, left, edge: 'above' };
}

/** On top of a video's top edge, else hanging from it inside the video, or null when its own controls leave no room there. */
function onVideo(
  target: Box,
  kind: { controls: boolean; covered?: boolean },
  viewport: Size,
  size: Size,
): Position | null {
  const left = Math.min(target.right, viewport.width) - size.width - TAB_MARGIN;
  const above = target.top - size.height;
  if (above >= 0 && kind.covered !== true) return { top: above, left, edge: 'above' };
  const top = Math.max(target.top, 0);
  const room = Math.min(target.bottom, viewport.height) - (kind.controls ? CONTROLS_SPACE : 0);
  if (top + size.height > room) return null;
  return { top, left, edge: 'below' };
}
