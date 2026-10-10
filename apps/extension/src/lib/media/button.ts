/**
 * The "Download" button shown over video and audio on web pages. Runs in a content
 * script, so it is plain DOM with no framework, and it lives in a closed shadow root:
 * page styles cannot reach inside, and its styles cannot leak out. Everything it
 * needs to know about the tab comes from the background script through `deps`.
 */
import type { DownloadAnswer, ElementInfo } from './messages';

export interface ButtonDeps {
  /** Whether anything downloadable is known for this element (asked before showing). */
  check(info: ElementInfo): Promise<boolean>;
  /** Hands the element's media to the app. */
  download(request: ElementInfo & { title: string | null }): Promise<DownloadAnswer>;
}

export interface Box {
  top: number;
  left: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

/** Smaller videos are previews or ads: no button. */
export const MIN_VIDEO = { width: 160, height: 90 };
/** Distance from the video's top corner. */
const INSET = 8;
/** Height kept clear above a video's native controls. */
const CONTROLS_SPACE = 48;
/** How long the button stays fully visible after the last pointer movement or play. */
export const ACTIVE_MS = 3000;
/** A "nothing to download" answer is asked again after this long (media may show up once playback starts). */
const RECHECK_MS = 3000;

/**
 * Where the button goes, in viewport pixels, or null when it should not show:
 * the element is too small, out of view, or there is no room left above a video's
 * own controls. Over a video it sits in the top right corner, away from the native
 * controls at the bottom. An audio player is all controls, so the button sits just
 * above it (or below, when there is no room above).
 */
export function placeButton(
  target: Box,
  kind: { audio: boolean; controls: boolean },
  viewport: { width: number; height: number },
  size: { width: number; height: number },
): { top: number; left: number } | null {
  if (target.width <= 0 || target.height <= 0) return null;
  if (!kind.audio && (target.width < MIN_VIDEO.width || target.height < MIN_VIDEO.height)) return null;
  if (
    target.bottom <= 0 ||
    target.right <= 0 ||
    target.top >= viewport.height ||
    target.left >= viewport.width
  ) {
    return null;
  }
  let top: number;
  let left: number;
  if (kind.audio) {
    top = target.top - size.height - 4;
    if (top < 0) top = target.bottom + 4;
    left = target.right - size.width;
  } else {
    top = Math.max(target.top, 0) + INSET;
    const room = Math.min(target.bottom, viewport.height) - (kind.controls ? CONTROLS_SPACE : INSET);
    if (top + size.height > room) return null;
    left = Math.min(target.right, viewport.width) - size.width - INSET;
  }
  left = Math.max(0, Math.min(left, viewport.width - size.width));
  return { top: Math.round(top), left: Math.round(left) };
}

/** A muted, looping video without controls is decoration (a moving background), so playing alone does not show the button. */
export function isDecorative(el: HTMLMediaElement): boolean {
  // `defaultMuted` is the `muted` attribute, which only sets `muted` when the page's HTML is parsed.
  return el.tagName === 'VIDEO' && (el.muted || el.defaultMuted) && el.loop && !el.controls;
}

/** Video always qualifies; audio only when it shows its own player (`controls`). */
export function isCandidate(el: HTMLMediaElement): boolean {
  return el.tagName === 'VIDEO' || (el.tagName === 'AUDIO' && el.controls);
}

export function elementInfo(el: HTMLMediaElement): ElementInfo {
  const src = el.currentSrc || el.src || el.querySelector('source')?.src || '';
  return { src: src === '' ? null : src, audio: el.tagName === 'AUDIO' };
}

/** The element's own label, or the page title. */
export function mediaTitle(el: HTMLMediaElement, doc: Document): string | null {
  const own = el.getAttribute('aria-label') ?? el.getAttribute('title') ?? '';
  const title = own.trim() || doc.title.trim();
  return title === '' ? null : title;
}

/**
 * The host's own styles. Important rules in a shadow root's `:host` beat the page's
 * important rules (the inner context wins), so a page rule such as
 * `* { display: none !important }` cannot hide or move the button. The position is
 * set inline, since `:host` would also win over the inline style; hiding the
 * button detaches the host instead of styling it.
 */
const HOST_RULES = [
  'position:fixed',
  'z-index:2147483647',
  'display:block',
  'visibility:visible',
  'opacity:1',
  'pointer-events:auto',
  'margin:0',
  'padding:0',
  'border:0',
  'transform:none',
  'filter:none',
  'clip-path:none',
  'width:auto',
  'height:auto',
  'max-width:none',
  'max-height:none',
  'right:auto',
  'bottom:auto',
  'contain:none',
  'zoom:1',
]
  .map((rule) => `${rule}!important`)
  .join(';');

const SHADOW_CSS = `
:host{${HOST_RULES}}
.w{all:initial;position:relative;display:block}
.b{all:initial;box-sizing:border-box;display:inline-flex;align-items:center;gap:6px;height:32px;padding:0 12px 0 10px;border-radius:16px;background:rgba(17,19,24,.88);color:#fff;font:600 13px/1 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;letter-spacing:.01em;white-space:nowrap;cursor:pointer;box-shadow:0 2px 10px rgba(0,0,0,.35),inset 0 0 0 1px rgba(255,255,255,.2);transition:opacity .15s,background-color .15s}
.b:hover{background:#1d4ed8}
.b:focus-visible{outline:2px solid #fff;outline-offset:2px;box-shadow:0 0 0 5px #1d4ed8}
.b[aria-busy=true]{cursor:progress}
.b svg{width:16px;height:16px;fill:none;stroke:currentColor;stroke-width:2.2;stroke-linecap:round;stroke-linejoin:round}
.w.idle .b:not(:focus-visible){opacity:0;pointer-events:none}
.s{all:initial;position:absolute;top:38px;right:0;width:max-content;max-width:260px;font:12px/1.4 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;color:#fff;border-radius:6px}
.s:not(:empty){padding:6px 10px;background:rgba(17,19,24,.94);box-shadow:0 2px 10px rgba(0,0,0,.35)}
.s.err:not(:empty){background:#991b1b}
@media (prefers-reduced-motion:reduce){.b{transition:none}}
@media (forced-colors:active){.b,.s:not(:empty){border:1px solid CanvasText;forced-color-adjust:auto}}
`;

const SVG_NS = 'http://www.w3.org/2000/svg';

/** A download arrow, built without `innerHTML` so pages that enforce Trusted Types do not refuse it. */
function icon(doc: Document): SVGElement {
  const svg = doc.createElementNS(SVG_NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  svg.setAttribute('focusable', 'false');
  const path = doc.createElementNS(SVG_NS, 'path');
  path.setAttribute('d', 'M12 4v11m-5-5 5 5 5-5M5 20h14');
  svg.append(path);
  return svg;
}

/** Events that would otherwise reach the page (a player toggling pause on a click, a shortcut on a key). */
const CONTAINED = [
  'click',
  'dblclick',
  'pointerdown',
  'pointerup',
  'mousedown',
  'mouseup',
  'keydown',
  'keyup',
  'touchstart',
];

export type Shown = 'none' | 'shown' | 'idle';

export class MediaButton {
  /** The element the page sees: a bare custom element whose content is in a closed shadow root. */
  readonly host: HTMLElement;
  /** Inside the closed shadow root; exposed for tests (pages cannot reach content script objects). */
  readonly button: HTMLButtonElement;
  readonly status: HTMLElement;
  readonly #wrap: HTMLElement;
  readonly #label: HTMLElement;
  readonly #deps: ButtonDeps;
  readonly #doc: Document;
  readonly #now: () => number;
  readonly #tracked = new Set<HTMLMediaElement>();
  readonly #checks = new WeakMap<HTMLMediaElement, { src: string | null; show: boolean; at: number }>();
  readonly #asking = new WeakSet<HTMLMediaElement>();
  readonly #cleanups: Array<() => void> = [];
  #enabled = false;
  #hovered: HTMLMediaElement | null = null;
  #playing: HTMLMediaElement | null = null;
  #target: HTMLMediaElement | null = null;
  #lastActive = 0;
  #busy = false;
  #frame = 0;
  #pointer: { x: number; y: number } | null = null;
  #timer: ReturnType<typeof setTimeout> | null = null;
  #statusTimer: ReturnType<typeof setTimeout> | null = null;
  #state: Shown = 'none';

  constructor(deps: ButtonDeps, doc: Document = document, now: () => number = Date.now) {
    this.#deps = deps;
    this.#doc = doc;
    this.#now = now;
    this.host = doc.createElement('wirecatch-media');
    const root = this.host.attachShadow({ mode: 'closed' });
    const style = doc.createElement('style');
    style.textContent = SHADOW_CSS;
    this.#wrap = doc.createElement('div');
    this.#wrap.className = 'w';
    this.button = doc.createElement('button');
    this.button.type = 'button';
    this.button.className = 'b';
    this.button.setAttribute('aria-label', 'Download with Wirecatch');
    this.#label = doc.createElement('span');
    this.#label.textContent = 'Download';
    this.button.append(icon(doc), this.#label);
    this.status = doc.createElement('span');
    this.status.className = 's';
    this.status.setAttribute('role', 'status');
    this.#wrap.append(this.button, this.status);
    root.append(style, this.#wrap);
    for (const type of CONTAINED) this.#wrap.addEventListener(type, (e) => e.stopPropagation());
    this.button.addEventListener('click', () => void this.#download());
    this.button.addEventListener('focus', () => this.#update());
    this.button.addEventListener('blur', () => this.#update());

    for (const el of doc.querySelectorAll<HTMLMediaElement>('video, audio')) this.#tracked.add(el);
    this.#listen(doc, 'play', (e) => this.#onPlay(e), true);
    this.#listen(doc, 'pause', (e) => this.#onStop(e), true);
    this.#listen(doc, 'ended', (e) => this.#onStop(e), true);
    this.#listen(doc, 'loadedmetadata', (e) => this.#track(e.target), true);
    this.#listen(doc, 'pointermove', (e) => this.#onPointer(e as PointerEvent), true);
    this.#listen(doc, 'pointerout', (e) => {
      if ((e as PointerEvent).relatedTarget === null) this.#onPointerGone();
    });
    this.#listen(doc, 'focusin', (e) => this.#onFocus(e.target), true);
    this.#listen(doc, 'scroll', () => this.#schedule(), true);
    this.#listen(doc, 'fullscreenchange', () => this.#update());
    const view = doc.defaultView;
    if (view !== null) this.#listen(view, 'resize', () => this.#schedule());
  }

  /** What the button looks like right now: hidden, shown, or faded out but still reachable with Tab. */
  get state(): Shown {
    return this.#state;
  }

  /** Follows the "Show download button on videos" setting; off hides the button at once. */
  setEnabled(on: boolean): void {
    this.#enabled = on;
    this.#update();
  }

  /** Removes the button and every listener. */
  destroy(): void {
    for (const cleanup of this.#cleanups.splice(0)) cleanup();
    if (this.#timer !== null) clearTimeout(this.#timer);
    if (this.#statusTimer !== null) clearTimeout(this.#statusTimer);
    this.host.remove();
  }

  #listen(target: EventTarget, type: string, listener: (event: Event) => void, capture = false): void {
    const options = { capture, passive: true };
    target.addEventListener(type, listener, options);
    this.#cleanups.push(() => target.removeEventListener(type, listener, options));
  }

  #track(target: EventTarget | null): HTMLMediaElement | null {
    if (!(target instanceof HTMLMediaElement)) return null;
    this.#tracked.add(target);
    return target;
  }

  #onPlay(event: Event): void {
    const el = this.#track(event.target);
    if (el === null || !isCandidate(el) || isDecorative(el)) return;
    this.#playing = el;
    this.#lastActive = this.#now();
    this.#update();
  }

  #onStop(event: Event): void {
    if (event.target === this.#playing) {
      this.#playing = null;
      this.#update();
    }
  }

  #onPointer(event: PointerEvent): void {
    this.#pointer = { x: event.clientX, y: event.clientY };
    if (event.target === this.host) {
      this.#lastActive = this.#now();
      return;
    }
    this.#schedule();
  }

  #onPointerGone(): void {
    this.#pointer = null;
    this.#hovered = null;
    this.#update();
  }

  #onFocus(target: EventTarget | null): void {
    if (target instanceof HTMLMediaElement && isCandidate(target)) {
      this.#track(target);
      this.#hovered = target;
      this.#lastActive = this.#now();
      this.#update();
    }
  }

  /** Runs `#update` once in the next frame, however many events come in before it. */
  #schedule(): void {
    if (this.#frame !== 0) return;
    const view = this.#doc.defaultView;
    if (view === null) return;
    this.#frame = view.requestAnimationFrame(() => {
      this.#frame = 0;
      this.#hitTest();
      this.#update();
    });
  }

  /** Finds the media element under the pointer, even under a player's transparent overlay. */
  #hitTest(): void {
    const p = this.#pointer;
    if (p === null) return;
    let hit: HTMLMediaElement | null = null;
    for (const el of this.#tracked) {
      if (!el.isConnected) {
        this.#tracked.delete(el);
        continue;
      }
      if (!isCandidate(el)) continue;
      const r = el.getBoundingClientRect();
      if (p.x >= r.left && p.x <= r.right && p.y >= r.top && p.y <= r.bottom) hit = el;
    }
    if (hit !== null) this.#lastActive = this.#now();
    if (hit !== this.#hovered) this.#hovered = hit;
  }

  /** Whether the background knows something to download for this element; asks when it does not know yet. */
  #allowed(el: HTMLMediaElement): boolean {
    const { src } = elementInfo(el);
    const known = this.#checks.get(el);
    if (known !== undefined && known.src === src && (known.show || this.#now() - known.at < RECHECK_MS)) {
      return known.show;
    }
    if (!this.#asking.has(el)) {
      this.#asking.add(el);
      this.#deps
        .check(elementInfo(el))
        .catch(() => false)
        .then((show) => {
          this.#asking.delete(el);
          this.#checks.set(el, { src, show, at: this.#now() });
          this.#update();
        })
        .catch(() => undefined);
    }
    return known?.show ?? false;
  }

  #update(): void {
    const focused = this.#isFocused();
    // Keep the element the person is acting on while the button has focus or is busy.
    if (!focused && !this.#busy) this.#target = this.#hovered ?? this.#playing;
    const target = this.#target;
    let state: Shown = 'none';
    if (this.#enabled && target !== null && target.isConnected && this.#allowed(target)) {
      const active = this.#now() - this.#lastActive < ACTIVE_MS;
      state = target === this.#hovered || focused || this.#busy || active ? 'shown' : 'idle';
      if (state === 'shown' && !focused && !this.#busy && target !== this.#hovered) {
        this.#wakeAt(this.#lastActive + ACTIVE_MS);
      }
    }
    this.#render(state, target);
  }

  #isFocused(): boolean {
    return this.#doc.activeElement === this.host;
  }

  #wakeAt(at: number): void {
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#timer = setTimeout(
      () => {
        this.#timer = null;
        this.#update();
      },
      Math.max(0, at - this.#now()) + 20,
    );
  }

  #render(state: Shown, target: HTMLMediaElement | null): void {
    const fullscreen = this.#doc.fullscreenElement;
    // A media element shown full screen by itself cannot have anything drawn over it;
    // a player container shown full screen can, if the button moves inside it.
    if (target !== null && fullscreen !== null && (fullscreen === target || !fullscreen.contains(target))) {
      state = 'none';
    }
    let position: { top: number; left: number } | null = null;
    if (state !== 'none' && target !== null) {
      const view = this.#doc.defaultView;
      const width = this.button.offsetWidth || 112;
      const height = this.button.offsetHeight || 32;
      position = placeButton(
        target.getBoundingClientRect(),
        { audio: target.tagName === 'AUDIO', controls: target.controls },
        { width: view?.innerWidth ?? 0, height: view?.innerHeight ?? 0 },
        { width, height },
      );
    }
    if (position === null) {
      this.host.remove();
      this.#state = 'none';
      return;
    }
    // Outside the body, where page frameworks do not manage the children, unless a
    // player container is full screen: only its contents are drawn then.
    const parent = fullscreen ?? this.#doc.documentElement;
    this.host.style.cssText = `position:fixed!important;top:${position.top}px!important;left:${position.left}px!important`;
    if (this.host.parentNode !== parent) parent.append(this.host);
    this.#wrap.classList.toggle('idle', state === 'idle');
    this.#state = state;
  }

  async #download(): Promise<void> {
    const target = this.#target;
    if (this.#busy || target === null) return;
    this.#busy = true;
    this.button.setAttribute('aria-busy', 'true');
    this.#label.textContent = 'Sending…';
    this.#say('', false);
    let answer: DownloadAnswer;
    try {
      answer = await this.#deps.download({ ...elementInfo(target), title: mediaTitle(target, this.#doc) });
    } catch {
      // The extension was updated or reloaded since this page loaded.
      answer = { ok: false, message: 'Wirecatch could not be reached. Reload this page, then try again.' };
    }
    this.#busy = false;
    this.button.removeAttribute('aria-busy');
    this.#label.textContent = 'Download';
    this.#say(answer.message, !answer.ok);
    this.#lastActive = this.#now();
    this.#update();
  }

  #say(message: string, error: boolean): void {
    this.status.textContent = message;
    this.status.classList.toggle('err', error);
    if (this.#statusTimer !== null) clearTimeout(this.#statusTimer);
    this.#statusTimer =
      message === ''
        ? null
        : setTimeout(
            () => {
              this.#statusTimer = null;
              this.status.textContent = '';
            },
            error ? 8000 : 4000,
          );
  }
}
