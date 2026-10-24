/**
 * The "Download" button shown over video and audio on web pages. Runs in a content
 * script, so it is plain DOM with no framework, and it lives in a closed shadow root:
 * page styles cannot reach inside, and its styles cannot leak out. Everything it
 * needs to know about the tab comes from the background script through `deps`.
 */
import { BUTTON_HEIGHT, SHADOW_CSS, TYPICAL_BUTTON_WIDTH, icon } from './button-style';
import { DownloadableChecks } from './checks';
import { randomHostName } from './host-name';
import { LiveMessage } from './live-message';
import type { DownloadAnswer, ElementInfo } from './messages';
import {
  containsDeep,
  elementInfo,
  findMedia,
  isUsable,
  mediaAt,
  mediaTitle,
  playerMedia,
} from './page-media';
import { INSET, placeButton, type Position } from './placement';

export interface ButtonDeps {
  /** Whether anything downloadable is known for this element (asked before showing). */
  check: (info: ElementInfo) => Promise<boolean>;
  /** Hands the element's media to the app. */
  download: (request: ElementInfo & { title: string | null }) => Promise<DownloadAnswer>;
  /**
   * Whether an activation came from the person (`event.isTrusted`), not from a script.
   * Replaceable for tests only: jsdom marks every event as untrusted.
   */
  isTrusted?: (event: Event) => boolean;
}

/** Added to the wait for the button to fade, so the timer fires just after that moment, not before. */
const WAKE_SLACK_MS = 20;
/** How long the button stays fully visible after the last pointer movement or play. */
export const ACTIVE_MS = 3000;

/** Page-wide listeners capture: media events such as `play` do not bubble, and a page may stop others from bubbling. */
const CAPTURE = { capture: true };

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

/**
 * A media element shown full screen by itself cannot have anything drawn over it;
 * a player container shown full screen can, if the button moves inside it.
 */
function hiddenByFullscreen(target: HTMLMediaElement, fullscreen: Element | null): boolean {
  return fullscreen !== null && (fullscreen === target || !containsDeep(fullscreen, target));
}

export class MediaButton {
  /** The element the page sees: a bare custom element, named afresh each time, whose content is in a closed shadow root. */
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
  readonly #checks: DownloadableChecks;
  readonly #message: LiveMessage;
  /** Open shadow roots already searched for media, each once. */
  readonly #searched = new WeakSet<ShadowRoot>();
  /** Removes the listeners that watch the page; run when the setting goes off. */
  readonly #cleanups: (() => void)[] = [];
  #enabled = false;
  /** Whether the page is being watched: only while the setting is on. */
  #watching = false;
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
  #state: Shown = 'none';

  constructor(deps: ButtonDeps, doc: Document = document, now: () => number = Date.now) {
    this.#deps = deps;
    this.#doc = doc;
    this.#now = now;
    this.host = doc.createElement(randomHostName());
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
    this.#checks = new DownloadableChecks(
      (info) => deps.check(info),
      now,
      () => this.#update(),
    );
    this.#message = new LiveMessage(this.status, () => this.#update());
    root.append(style, this.#wrap);
    for (const type of CONTAINED) this.#wrap.addEventListener(type, (e) => e.stopPropagation());
    // Only a real click or key press, never one a page script made up.
    this.button.addEventListener('click', (event) => {
      if (deps.isTrusted?.(event) ?? event.isTrusted) void this.#download();
    });
    this.button.addEventListener('focus', () => this.#update());
    this.button.addEventListener('blur', () => this.#update());
    this.button.addEventListener('keydown', (e) => this.#onButtonKey(e));
  }

  /**
   * Starts watching the page: finds its media, and follows playback, the pointer, focus and scrolling. Only while the setting is on, because the script runs in every frame of every page and this work (a page-wide search, layout reads on pointer moves) would otherwise cost every page, for everyone, also with the button turned off.
   */
  #watch(): void {
    const doc = this.#doc;
    for (const el of findMedia(doc)) {
      const tracked = this.#track(el);
      // Already playing when the setting came on: no `play` event will say so.
      if (tracked !== null && !tracked.paused && isUsable(tracked)) {
        this.#playing = tracked;
        this.#lastActive = this.#now();
      }
    }
    this.#listen(doc, 'play', (e) => this.#onPlay(e), CAPTURE);
    this.#listen(doc, 'pause', (e) => this.#onStop(e), CAPTURE);
    this.#listen(doc, 'ended', (e) => this.#onStop(e), CAPTURE);
    this.#listen(doc, 'loadedmetadata', (e) => this.#track(e.target), CAPTURE);
    this.#listen(doc, 'pointerover', (e) => this.#discover(e), CAPTURE);
    this.#listen(doc, 'pointermove', (e) => this.#onPointer(e), CAPTURE);
    this.#listen(doc, 'pointerout', (e) => {
      if (e instanceof PointerEvent && e.relatedTarget === null) this.#onPointerGone();
    });
    // `composedPath()[0]` is the element itself, also inside a page's open shadow root.
    this.#listen(doc, 'focusin', (e) => this.#onFocus(e.composedPath()[0] ?? e.target), CAPTURE);
    // Not passive: it may cancel Tab to move focus to the button.
    this.#listen(doc, 'keydown', (e) => e instanceof KeyboardEvent && this.#onKey(e), {
      capture: true,
      passive: false,
    });
    this.#listen(doc, 'scroll', () => this.#schedule(), CAPTURE);
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
      // Where the button would sit over a video filling the window.
      const left = (view?.innerWidth ?? 0) - TYPICAL_BUTTON_WIDTH - 2 * INSET;
      this.#position({ top: INSET, left: Math.max(0, left) });
      if (this.host.parentNode !== parent) parent.append(this.host);
      this.#wrap.classList.add('msg');
      this.#state = 'message';
    }
    // The live region is in the page first, then gets its text, so it is announced.
    const say = (): void => this.#message.say(message, error);
    const view = this.#doc.defaultView;
    if (view === null) say();
    else view.requestAnimationFrame(say);
  }

  /** Follows the "Show download button on videos" setting; off hides the button at once and stops watching the page. */
  setEnabled(on: boolean): void {
    this.#enabled = on;
    if (on && !this.#watching) {
      this.#watching = true;
      this.#watch();
    } else if (!on && this.#watching) {
      this.#unwatch();
    }
    this.#update();
  }

  /** Stops watching the page and forgets its media. */
  #unwatch(): void {
    this.#watching = false;
    for (const cleanup of this.#cleanups.splice(0)) cleanup();
    this.#tracked.clear();
    this.#hovered = null;
    this.#playing = null;
    this.#pointer = null;
    if (this.#frame !== 0) {
      this.#doc.defaultView?.cancelAnimationFrame(this.#frame);
      this.#frame = 0;
    }
  }

  /** Removes the button and every listener. */
  destroy(): void {
    this.#unwatch();
    if (this.#timer !== null) clearTimeout(this.#timer);
    this.#message.stop();
    this.host.remove();
  }

  /** Adds a listener that `#unwatch` removes again; passive and in the bubbling phase unless `options` says otherwise. */
  #listen(
    target: EventTarget,
    type: string,
    listener: (event: Event) => void,
    { capture = false, passive = true }: { capture?: boolean; passive?: boolean } = {},
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

  /**
   * Media the pointer comes near that the first search could not see: inside an open shadow root added later (a player built after the page loaded). Each root on the pointer's way in is searched once, when the pointer first enters it, so a page without media pays nothing per move.
   */
  #discover(event: Event): void {
    for (const node of event.composedPath()) {
      if (node instanceof HTMLMediaElement) this.#track(node);
      else if (node instanceof ShadowRoot && !this.#searched.has(node)) {
        this.#searched.add(node);
        for (const el of findMedia(node)) this.#track(el);
      }
    }
  }

  #onPlay(event: Event): void {
    const el = this.#track(event.target);
    if (el === null || !isUsable(el)) return;
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

  #onPointer(event: Event): void {
    if (!(event instanceof PointerEvent)) return;
    this.#pointer = { x: event.clientX, y: event.clientY };
    this.#overButton = event.target === this.host;
    if (this.#overButton) this.#lastActive = this.#now();
    // Nothing to hover and nothing showing: no frame, no layout read.
    if (this.#tracked.size === 0 && this.#state === 'none') return;
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
    if (
      !(target instanceof Element) ||
      target === this.#doc.body ||
      target === this.#doc.documentElement
    ) {
      return null;
    }
    if (target instanceof HTMLMediaElement) return isUsable(target) ? this.#track(target) : null;
    const box = target.getBoundingClientRect();
    // A player's media may not have been seen yet (added without any event reaching the page).
    if (box.width * box.height > 0) for (const el of findMedia(target)) this.#track(el);
    return playerMedia(target, this.#tracked);
  }

  /**
   * The button sits outside the page's body, so it would come last with Tab. Tab on
   * the media element (or its player) moves to the button instead, and Shift+Tab or
   * Escape on the button goes back; Tab on the button goes back too and lets the
   * browser carry on from there, so the button sits right after the player.
   */
  #onKey(event: KeyboardEvent): void {
    if (event.key !== 'Tab' || event.shiftKey || event.altKey || event.ctrlKey || event.metaKey)
      return;
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
    if ((!back && !onward) || !to?.isConnected) return;
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
    const hit = mediaAt(this.#tracked, p.x, p.y);
    if (hit !== null) this.#lastActive = this.#now();
    this.#hovered = hit;
  }

  #update(): void {
    // Keep the element the person is acting on while the button has focus or is busy.
    const holding = this.#isFocused() || this.#busy;
    if (!holding) this.#target = this.#hovered ?? this.#playing;
    const target = this.#target;
    const wanted =
      target !== null && this.#enabled && target.isConnected && this.#checks.allowed(target);
    this.#render(wanted ? this.#visibility(target, holding) : 'none', target);
  }

  /**
   * Shown while the pointer is on the element or the person is acting on the button,
   * and for `ACTIVE_MS` after the last activity; faded out (`idle`) after that.
   */
  #visibility(target: HTMLMediaElement, holding: boolean): Shown {
    const hovered = target === this.#hovered;
    const active = this.#now() - this.#lastActive < ACTIVE_MS;
    if (!hovered && !holding && !active) return 'idle';
    // Only recent activity keeps it shown: fade it out once that is over.
    if (!hovered && !holding) this.#wakeAt(this.#lastActive + ACTIVE_MS);
    return 'shown';
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
      Math.max(0, at - this.#now()) + WAKE_SLACK_MS,
    );
  }

  #render(wanted: Shown, target: HTMLMediaElement | null): void {
    const fullscreen = this.#doc.fullscreenElement;
    const state = target !== null && hiddenByFullscreen(target, fullscreen) ? 'none' : wanted;
    const position = state === 'none' || target === null ? null : this.#place(target);
    if (position === null || target === null) {
      this.#hide();
      return;
    }
    this.#show(target, position, fullscreen);
    this.#wrap.classList.toggle('idle', state === 'idle');
    this.#state = state;
  }

  #place(el: HTMLMediaElement): Position | null {
    const view = this.#doc.defaultView;
    return placeButton(
      el.getBoundingClientRect(),
      { audio: el.tagName === 'AUDIO', controls: el.controls },
      { width: view?.innerWidth ?? 0, height: view?.innerHeight ?? 0 },
      // Measured once the button is in the page; a typical size until then.
      {
        width: this.button.offsetWidth || TYPICAL_BUTTON_WIDTH,
        height: this.button.offsetHeight || BUTTON_HEIGHT,
      },
    );
  }

  #hide(): void {
    if (this.#message.showing && this.host.isConnected) {
      // Keep the message (a live region) in place until it has been read; it expires on its own.
      this.#wrap.classList.add('msg');
      this.#state = 'message';
      return;
    }
    this.host.remove();
    this.#state = 'none';
  }

  #show(target: HTMLMediaElement, position: Position, fullscreen: Element | null): void {
    this.#wrap.classList.remove('msg');
    // Outside the body, where page frameworks do not manage the children, unless a
    // player container is full screen: only its contents are drawn then.
    const parent = fullscreen ?? this.#doc.documentElement;
    const attached = this.host.parentNode === parent;
    this.#position(position);
    if (!attached) {
      parent.append(this.host);
      // Placed again with the button's real size, now that it can be measured.
      this.#position(this.#place(target) ?? position);
    }
  }

  #position(position: Position): void {
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
    this.#message.say('', false);
    let answer: DownloadAnswer;
    try {
      answer = await this.#deps.download({
        ...elementInfo(target),
        title: mediaTitle(target, this.#doc),
      });
    } catch {
      // The extension was updated or reloaded since this page loaded.
      answer = {
        ok: false,
        message: 'Wirecatch could not be reached. Reload this page, then try again.',
      };
    }
    this.#busy = false;
    this.button.removeAttribute('aria-busy');
    this.#label.textContent = 'Download';
    this.#message.say(answer.message, !answer.ok);
    this.#lastActive = this.#now();
    this.#update();
  }
}
