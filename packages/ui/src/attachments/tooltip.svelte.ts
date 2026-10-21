import type { Attachment } from 'svelte/attachments';
import { placeFlyout, layoutSize, viewportSize, type Placement } from '../lib/flyout';
import { lower, raise } from '../lib/layer';
import { delay } from '../motion/motion';

const PLACEMENT: Placement = { side: 'above', align: 'center', gap: 8, inset: 4 };

/** What the tooltip says; a function is read each time it opens, so it can depend on the node (see `textIfCut`). */
export type TooltipText =
  string | null | undefined | ((node: HTMLElement) => string | null | undefined);

// The tip takes the pointer, so it can be hovered (WCAG 1.4.13): the pointer may move from the anchor onto it to read it.
const TIP_CLASS =
  'fixed inset-auto m-0 z-popover max-w-prose rounded-sm border border-line bg-overlay px-2 py-1 text-small text-ink shadow-flyout animate-pop-in';

/** `text` only while `target` (the node by default) is cut short, for a truncated label: cut at its end (`truncate`), or below its last line (`line-clamp-*`). */
export const textIfCut =
  (text: string, target: (node: HTMLElement) => Element | null = (node) => node) =>
  (node: HTMLElement): string | null => {
    const element = target(node);
    if (element === null) return null;
    const cut =
      element.scrollWidth > element.clientWidth || element.scrollHeight > element.clientHeight;
    return cut ? text : null;
  };

// Moving from one tooltip to the next within the delay shows the next at once, as Windows does. Kept per document, which owns it: each window has its own.
const lastHidden = new WeakMap<Document, number>();
// Tooltip ids count up per document too: `crypto.randomUUID` exists only in secure contexts, and the dev server for a device on the network is plain http.
const issued = new WeakMap<Document, number>();

function nextTooltipId(): string {
  const next = (issued.get(document) ?? 0) + 1;
  issued.set(document, next);
  return `wc-tooltip-${String(next)}`;
}

class Tooltip {
  text: TooltipText = null;
  readonly #node: HTMLElement;
  readonly #id = nextTooltipId();
  #tip: HTMLElement | null = null;
  #timer: ReturnType<typeof setTimeout> | undefined;
  /** A hide waiting out the linger, so the pointer can cross from the anchor to the tip. */
  #leaving: ReturnType<typeof setTimeout> | undefined;

  constructor(node: HTMLElement) {
    this.#node = node;
  }

  read(): string | null | undefined {
    return typeof this.text === 'function' ? this.text(this.#node) : this.text;
  }

  /** Puts a changed text into an open tooltip, or closes it when there is none any more. */
  update(): void {
    const content = this.read();
    if (!content) this.hide();
    else if (this.#tip) this.#tip.textContent = content;
  }

  readonly schedule = (): void => {
    if (!this.read()) return;
    if (this.#tip) {
      this.stay();
      return;
    }
    this.hide();
    const wait = delay('tooltip');
    const warm = performance.now() - (lastHidden.get(document) ?? -Infinity) < wait;
    if (warm) this.show();
    else this.#timer = setTimeout(() => this.show(), wait);
  };

  readonly showOnFocus = (): void => {
    if (this.#node.matches(':focus-visible') || this.#node.querySelector(':focus-visible')) {
      this.schedule();
    }
  };

  readonly hideOnFocusOut = (event: FocusEvent): void => {
    if (!(event.relatedTarget instanceof Node && this.#node.contains(event.relatedTarget))) {
      this.hide();
    }
  };

  /** Capture phase, stopped: the first Escape closes the tooltip, not the dialog or field under it. */
  readonly hideOnEscape = (event: KeyboardEvent): void => {
    if (event.key !== 'Escape' || !this.#tip) return;
    event.preventDefault();
    event.stopPropagation();
    this.hide();
  };

  /** The pointer left the anchor or the tip: hide after the linger, unless it reaches the other first. */
  readonly leave = (): void => {
    clearTimeout(this.#timer);
    if (!this.#tip) return;
    clearTimeout(this.#leaving);
    this.#leaving = setTimeout(this.hide, delay('linger'));
  };

  readonly stay = (): void => {
    clearTimeout(this.#leaving);
  };

  readonly hide = (): void => {
    clearTimeout(this.#timer);
    clearTimeout(this.#leaving);
    if (!this.#tip) return;
    lower(this.#tip);
    this.#tip.remove();
    this.#tip = null;
    lastHidden.set(document, performance.now());
    if (this.#node.getAttribute('aria-describedby') === this.#id) {
      this.#node.removeAttribute('aria-describedby');
    }
    this.#listen(false);
  };

  readonly position = (): void => {
    if (!this.#tip) return;
    const box = this.#node.getBoundingClientRect();
    const { top, left } = placeFlyout(box, layoutSize(this.#tip), viewportSize(), PLACEMENT);
    this.#tip.style.top = `${String(top)}px`;
    this.#tip.style.left = `${String(left)}px`;
  };

  show(): void {
    const content = this.read();
    if (!content) return;
    const tip = document.createElement('div');
    tip.id = this.#id;
    tip.setAttribute('role', 'tooltip');
    tip.setAttribute('popover', 'manual');
    tip.className = TIP_CLASS;
    tip.textContent = content;
    tip.addEventListener('pointerenter', this.stay);
    tip.addEventListener('pointerleave', this.leave);
    // Inside the anchor's dialog, if it has one: outside a modal dialog's subtree the page is inert.
    (this.#node.closest('dialog') ?? document.body).append(tip);
    this.#tip = tip;
    raise(tip);
    this.position();
    // Describes the control only when it adds to its name; a tooltip that repeats the label stays quiet.
    const name = this.#node.getAttribute('aria-label') ?? this.#node.textContent.trim();
    if (content !== name && !this.#node.hasAttribute('aria-describedby')) {
      this.#node.setAttribute('aria-describedby', this.#id);
    }
    this.#listen(true);
  }

  readonly hideIfHidden = (): void => {
    if (document.hidden) this.hide();
  };

  /** What interrupts a hover without a mouseleave. */
  #listen(on: boolean): void {
    if (on) {
      window.addEventListener('scroll', this.hide, true);
      window.addEventListener('resize', this.position);
      document.addEventListener('visibilitychange', this.hideIfHidden);
      document.addEventListener('keydown', this.hideOnEscape, true);
    } else {
      window.removeEventListener('scroll', this.hide, true);
      window.removeEventListener('resize', this.position);
      document.removeEventListener('visibilitychange', this.hideIfHidden);
      document.removeEventListener('keydown', this.hideOnEscape, true);
    }
  }

  /** Follows the anchor's hover and focus. */
  watch(): () => void {
    const node = this.#node;
    node.addEventListener('mouseenter', this.schedule);
    node.addEventListener('mouseleave', this.leave);
    node.addEventListener('pointerdown', this.hide);
    node.addEventListener('click', this.hide);
    node.addEventListener('focusin', this.showOnFocus);
    node.addEventListener('focusout', this.hideOnFocusOut);
    return () => {
      node.removeEventListener('mouseenter', this.schedule);
      node.removeEventListener('mouseleave', this.leave);
      node.removeEventListener('pointerdown', this.hide);
      node.removeEventListener('click', this.hide);
      node.removeEventListener('focusin', this.showOnFocus);
      node.removeEventListener('focusout', this.hideOnFocusOut);
      this.hide();
    };
  }
}

/**
 * A tooltip on hover and keyboard focus, after the tooltip delay. Takes a getter, so a changed text updates the open tooltip in place: `{@attach tooltip(() => label)}`.
 */
export const tooltip =
  (getText: () => TooltipText): Attachment<HTMLElement> =>
  (node) => {
    const tip = new Tooltip(node);
    $effect(() => {
      tip.text = getText();
      tip.update();
    });
    return tip.watch();
  };
