/**
 * The "Download" button shown over video and audio on web pages. Runs in a content
 * script, so it is plain DOM with no framework, and it lives in a closed shadow root:
 * page styles cannot reach inside, and its styles cannot leak out. Everything it
 * needs to know about the tab comes from the background script through `deps`.
 */
import { MAX_TITLE_LENGTH, isWebSrc, type DownloadAnswer, type ElementInfo } from './messages';

export interface ButtonDeps {
  /** Whether anything downloadable is known for this element (asked before showing). */
  check(info: ElementInfo): Promise<boolean>;
  /** Hands the element's media to the app. */
  download(request: ElementInfo & { title: string | null }): Promise<DownloadAnswer>;
  /**
   * Whether an activation came from the person (`event.isTrusted`), not from a script.
   * Replaceable for tests only: jsdom marks every event as untrusted.
   */
  isTrusted?(event: Event): boolean;
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

/** A muted, looping video without controls is decoration (a moving background): no button, playing or hovered. */
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
  // `blob:`, `data:` and `mediasource:` addresses mean nothing outside the page: not sent.
  return { src: isWebSrc(src) ? src : null, audio: el.tagName === 'AUDIO' };
}

/** Most elements looked at when searching a page, its open shadow roots included, for media. */
const SEARCH_BUDGET = 20_000;

/**
 * The video and audio elements under `root`, including inside open shadow roots
 * (closed ones cannot be seen). Stops after `SEARCH_BUDGET` elements on huge pages.
 */
export function findMedia(root: ParentNode): HTMLMediaElement[] {
  const found: HTMLMediaElement[] = [];
  const roots: ParentNode[] = [root];
  let seen = 0;
  for (let next = roots.shift(); next !== undefined && seen < SEARCH_BUDGET; next = roots.shift()) {
    for (const el of next.querySelectorAll('*')) {
      seen += 1;
      if (el instanceof HTMLMediaElement) found.push(el);
      if (el.shadowRoot !== null) roots.push(el.shadowRoot);
      if (seen >= SEARCH_BUDGET) break;
    }
  }
  return found;
}

/** Like `container.contains(el)`, but also through shadow roots. */
function containsDeep(container: Node, el: Node): boolean {
  for (let node: Node | null = el; node !== null;) {
    if (container.contains(node)) return true;
    const root = node.getRootNode();
    node = root instanceof ShadowRoot ? root.host : null;
  }
  return false;
}

/** The element's own label, or the page title. */
export function mediaTitle(el: HTMLMediaElement, doc: Document): string | null {
  const own = el.getAttribute('aria-label') ?? el.getAttribute('title') ?? '';
  const title = own.trim() || doc.title.trim();
  return title === '' ? null : title.slice(0, MAX_TITLE_LENGTH);
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
  // The host's box never takes the pointer; only the visible button does (see `.b`),
  // so the faded-out button cannot swallow clicks meant for the player's controls.
  'pointer-events:none',
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

export const SHADOW_CSS = `
:host{${HOST_RULES}}
.w{all:initial;position:relative;display:block;pointer-events:none}
.b{all:initial;box-sizing:border-box;display:inline-flex;align-items:center;gap:6px;height:32px;padding:0 12px 0 10px;border-radius:16px;background:rgba(17,19,24,.88);color:#fff;font:600 13px/1 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;letter-spacing:.01em;white-space:nowrap;cursor:pointer;box-shadow:0 2px 10px rgba(0,0,0,.35),inset 0 0 0 1px rgba(255,255,255,.2);transition:opacity .15s,background-color .15s;pointer-events:auto}
.b:hover{background:#1d4ed8}
.b:focus-visible{outline:2px solid #fff;outline-offset:2px;box-shadow:0 0 0 5px #1d4ed8}
.b[aria-busy=true]{cursor:progress}
.b svg{width:16px;height:16px;fill:none;stroke:currentColor;stroke-width:2.2;stroke-linecap:round;stroke-linejoin:round}
.w.idle .b:not(:focus-visible){opacity:0;pointer-events:none}
.s{all:initial;position:absolute;top:38px;right:0;width:max-content;max-width:260px;font:12px/1.4 system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;color:#fff;border-radius:6px}
.s:not(:empty){padding:6px 10px;background:rgba(17,19,24,.94);box-shadow:0 2px 10px rgba(0,0,0,.35)}
.s.err:not(:empty){background:#991b1b}
.w.msg .b{visibility:hidden}
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

/** `message`: the button is gone but its last message stays readable (and announced) until it expires. */
export type Shown = 'none' | 'shown' | 'idle' | 'message';

/** How long a message stays, long enough for screen readers to announce it. */
export const MESSAGE_MS = { ok: 4000, error: 8000 };

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
  /** True while the pointer is on the button itself, which may sit outside the element (audio). */
  #overButton = false;
  /** Where focus came from when Tab moved it to the button, and goes back to. */
  #returnTo: HTMLElement | null = null;
  /** The position last written to the host, so an unchanged one is not written again. */
  #style = '';
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
    // Only a real click or key press, never one a page script made up.
    this.button.addEventListener('click', (event) => {
      if (deps.isTrusted?.(event) ?? event.isTrusted) void this.#download();
    });
    this.button.addEventListener('focus', () => this.#update());
    this.button.addEventListener('blur', () => this.#update());
    this.button.addEventListener('keydown', (e) => this.#onButtonKey(e));

    for (const el of findMedia(doc)) this.#track(el);
    this.#listen(doc, 'play', (e) => this.#onPlay(e), true);
    this.#listen(doc, 'pause', (e) => this.#onStop(e), true);
    this.#listen(doc, 'ended', (e) => this.#onStop(e), true);
    this.#listen(doc, 'loadedmetadata', (e) => this.#track(e.target), true);
    this.#listen(doc, 'pointermove', (e) => this.#onPointer(e as PointerEvent), true);
    this.#listen(doc, 'pointerout', (e) => {
      if ((e as PointerEvent).relatedTarget === null) this.#onPointerGone();
    });
    // `composedPath()[0]` is the element itself, also inside a page's open shadow root.
    this.#listen(doc, 'focusin', (e) => this.#onFocus(e.composedPath()[0] ?? e.target), true);
    // Not passive: it may cancel Tab to move focus to the button.
    this.#listen(doc, 'keydown', (e) => this.#onKey(e as KeyboardEvent), true, false);
    this.#listen(doc, 'scroll', () => this.#schedule(), true);
    this.#listen(doc, 'fullscreenchange', () => this.#update());
    const view = doc.defaultView;
    if (view !== null) this.#listen(view, 'resize', () => this.#schedule());
  }

  /** What the button looks like right now: hidden, shown, or faded out but still reachable with Tab. */
  get state(): Shown {
    return this.#state;
  }

  /**
   * Shows the answer to the keyboard shortcut. Over the button when it is showing;
   * otherwise in the top right corner, until the message expires.
   */
  announce(message: string, error: boolean): void {
    if (this.#state !== 'shown' && this.#state !== 'idle') {
      const view = this.#doc.defaultView;
      const parent = this.#doc.fullscreenElement ?? this.#doc.documentElement;
      this.#position({ top: 8, left: Math.max(0, (view?.innerWidth ?? 0) - 128) });
      if (this.host.parentNode !== parent) parent.append(this.host);
      this.#wrap.classList.add('msg');
      this.#state = 'message';
    }
    // The live region is in the page first, then gets its text, so it is announced.
    const say = (): void => this.#say(message, error);
    const view = this.#doc.defaultView;
    if (view === null) say();
    else view.requestAnimationFrame(say);
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

  #listen(
    target: EventTarget,
    type: string,
    listener: (event: Event) => void,
    capture = false,
    passive = true,
  ): void {
    const options = { capture, passive };
    target.addEventListener(type, listener, options);
    this.#cleanups.push(() => target.removeEventListener(type, listener, options));
  }

  #track(target: EventTarget | null): HTMLMediaElement | null {
    if (!(target instanceof HTMLMediaElement)) return null;
    if (!this.#tracked.has(target) && target.getRootNode() !== this.#doc) {
      // Media events do not leave a shadow root, so listen on the element itself.
      this.#listen(target, 'play', (e) => this.#onPlay(e));
      this.#listen(target, 'pause', (e) => this.#onStop(e));
      this.#listen(target, 'ended', (e) => this.#onStop(e));
    }
    this.#tracked.add(target);
    return target;
  }

  /** Media under the pointer inside open shadow roots, which the page-level search may not have seen yet. */
  #findAt(x: number, y: number): void {
    const queue: Array<Document | ShadowRoot> = [this.#doc];
    for (let i = 0, root = queue[0]; root !== undefined && i < 20; i += 1, root = queue[i]) {
      if (typeof root.elementsFromPoint !== 'function') return;
      for (const el of root.elementsFromPoint(x, y)) {
        if (el instanceof HTMLMediaElement) this.#track(el);
        else if (el.shadowRoot !== null && !queue.includes(el.shadowRoot)) queue.push(el.shadowRoot);
      }
    }
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
    this.#overButton = event.target === this.host;
    if (this.#overButton) this.#lastActive = this.#now();
    this.#schedule();
  }

  #onPointerGone(): void {
    this.#pointer = null;
    this.#overButton = false;
    this.#hovered = null;
    this.#update();
  }

  #onFocus(target: EventTarget | null): void {
    const el = this.#mediaFor(target);
    if (el !== null) {
      this.#hovered = el;
      this.#lastActive = this.#now();
      this.#update();
    }
  }

  /**
   * The media element a focused element stands for: the element itself, or the one
   * inside a focused player container about its size. A large container (the page's
   * main area, say) is not a player, so focus there does not count.
   */
  #mediaFor(target: EventTarget | null): HTMLMediaElement | null {
    if (!(target instanceof Element) || target === this.#doc.body || target === this.#doc.documentElement) {
      return null;
    }
    const usable = (el: HTMLMediaElement): boolean => isCandidate(el) && !isDecorative(el);
    if (target instanceof HTMLMediaElement) return usable(target) ? this.#track(target) : null;
    const box = target.getBoundingClientRect();
    // A player's media may not have been seen yet (added without any event reaching the page).
    if (box.width * box.height > 0) for (const el of findMedia(target)) this.#track(el);
    for (const el of this.#tracked) {
      if (!usable(el) || !containsDeep(target, el)) continue;
      const r = el.getBoundingClientRect();
      if (box.width * box.height <= 1.5 * r.width * r.height) return el;
    }
    return null;
  }

  /**
   * The button sits outside the page's body, so it would come last with Tab. Tab on
   * the media element (or its player) moves to the button instead, and Shift+Tab or
   * Escape on the button goes back; Tab on the button goes back too and lets the
   * browser carry on from there, so the button sits right after the player.
   */
  #onKey(event: KeyboardEvent): void {
    if (event.key !== 'Tab' || event.shiftKey || event.altKey || event.ctrlKey || event.metaKey) return;
    const focused = event.composedPath()[0] ?? event.target;
    const reachable = this.#state === 'shown' || this.#state === 'idle';
    if (!reachable || this.#target === null || this.#mediaFor(focused) !== this.#target) return;
    if (!(focused instanceof HTMLElement)) return;
    event.preventDefault();
    this.#returnTo = focused;
    this.button.focus();
  }

  #onButtonKey(event: KeyboardEvent): void {
    const back = event.key === 'Escape' || (event.key === 'Tab' && event.shiftKey);
    const onward = event.key === 'Tab' && !event.shiftKey;
    const to = this.#returnTo;
    if ((!back && !onward) || to === null || !to.isConnected) return;
    // Onward: focus the player again without cancelling the key, so the browser moves on from it.
    if (back) event.preventDefault();
    this.#returnTo = null;
    to.focus();
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
    // On the button, the element it belongs to stays the one in use.
    if (p === null || this.#overButton) return;
    let hit = this.#trackedAt(p.x, p.y);
    if (hit === null) {
      // Only when no known element is there: look inside open shadow roots under the pointer.
      this.#findAt(p.x, p.y);
      hit = this.#trackedAt(p.x, p.y);
    }
    if (hit !== null) this.#lastActive = this.#now();
    this.#hovered = hit;
  }

  #trackedAt(x: number, y: number): HTMLMediaElement | null {
    let hit: HTMLMediaElement | null = null;
    for (const el of this.#tracked) {
      if (!el.isConnected) {
        this.#tracked.delete(el);
        continue;
      }
      // A moving background is not something to download, hovered or not.
      if (!isCandidate(el) || isDecorative(el)) continue;
      const r = el.getBoundingClientRect();
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) hit = el;
    }
    return hit;
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
    if (
      target !== null &&
      fullscreen !== null &&
      (fullscreen === target || !containsDeep(fullscreen, target))
    ) {
      state = 'none';
    }
    const place = (el: HTMLMediaElement): { top: number; left: number } | null => {
      const view = this.#doc.defaultView;
      return placeButton(
        el.getBoundingClientRect(),
        { audio: el.tagName === 'AUDIO', controls: el.controls },
        { width: view?.innerWidth ?? 0, height: view?.innerHeight ?? 0 },
        // Measured once the button is in the page; a typical size until then.
        { width: this.button.offsetWidth || 112, height: this.button.offsetHeight || 32 },
      );
    };
    let position = state === 'none' || target === null ? null : place(target);
    if (position === null || target === null) {
      if (this.status.textContent !== '' && this.host.isConnected) {
        // Keep the message (a live region) in place until it has been read; it expires on its own.
        this.#wrap.classList.add('msg');
        this.#state = 'message';
        return;
      }
      this.host.remove();
      this.#state = 'none';
      return;
    }
    this.#wrap.classList.remove('msg');
    // Outside the body, where page frameworks do not manage the children, unless a
    // player container is full screen: only its contents are drawn then.
    const parent = fullscreen ?? this.#doc.documentElement;
    const attached = this.host.parentNode === parent;
    this.#position(position);
    if (!attached) {
      parent.append(this.host);
      // Placed again with the button's real size, now that it can be measured.
      position = place(target) ?? position;
      this.#position(position);
    }
    this.#wrap.classList.toggle('idle', state === 'idle');
    this.#state = state;
  }

  #position(position: { top: number; left: number }): void {
    const style = `position:fixed!important;top:${position.top}px!important;left:${position.left}px!important`;
    // Scrolling and pointer moves call this every frame: only touch the page's DOM on a change.
    if (style === this.#style) return;
    this.#style = style;
    this.host.style.cssText = style;
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
              this.#update();
            },
            error ? MESSAGE_MS.error : MESSAGE_MS.ok,
          );
  }
}
