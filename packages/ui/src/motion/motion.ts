// Motion presets. Timing lives once, as tokens in tokens.css; these read them, so motion Svelte runs matches motion CSS runs. CSS (`animate-*`, `duration-*`, `ease-*`) covers entrances and hover; these presets cover what Svelte must hold for its exit or measure.
import { flip, type AnimationConfig } from 'svelte/animate';
import { prefersReducedMotion } from 'svelte/motion';
import { slide, type EasingFunction, type TransitionConfig } from 'svelte/transition';
import { lengthToken, timeToken, token } from '../lib/css-token';
import { cubicBezier, parseCubicBezier } from './easing';

export type Speed = 'fast' | 'normal' | 'slow' | 'slower';
export type Delay = 'reveal' | 'settle' | 'tooltip' | 'linger';
export type Curve = 'out' | 'in' | 'in-out' | 'spring';
export type Distance = 'sm' | 'md' | 'lg' | 'panel';
export type Side = 'above' | 'below' | 'left' | 'right';

/** Every token name the presets read, for the test that holds them to tokens.css. */
export const MOTION_TOKENS = {
  speeds: ['fast', 'normal', 'slow', 'slower'],
  delays: ['reveal', 'settle', 'tooltip', 'linger'],
  curves: ['out', 'in', 'in-out', 'spring'],
  distances: ['sm', 'md', 'lg', 'panel'],
} as const satisfies {
  speeds: readonly Speed[];
  delays: readonly Delay[];
  curves: readonly Curve[];
  distances: readonly Distance[];
};

const LINEAR: EasingFunction = (t) => t;

/** Whether the person asked for less motion. Svelte's transitions run in JS, which base.css's reduced-motion rule cannot reach, so every preset asks this. */
export function reducedMotion(): boolean {
  return prefersReducedMotion.current;
}

/** A duration token in milliseconds; 0 under reduced motion. */
export function duration(speed: Speed): number {
  return reducedMotion() ? 0 : timeToken(`--transition-duration-${speed}`);
}

/** An intent or anti-flicker wait in milliseconds; not shortened by reduced motion, as nothing moves during it. */
export function delay(name: Delay): number {
  return timeToken(`--transition-delay-${name}`);
}

export function distance(size: Distance): number {
  return lengthToken(`--motion-distance-${size}`);
}

// A memo, not state: a curve is fixed for the life of the page, and each window runs its own copy of this module.
const curves = new Map<Curve, EasingFunction>();

/** An easing token as a function; linear where the stylesheet is not loaded. */
export function easing(curve: Curve): EasingFunction {
  const cached = curves.get(curve);
  if (cached) return cached;
  const points = parseCubicBezier(token(`--ease-${curve}`));
  if (!points) return LINEAR;
  const fn = cubicBezier(points);
  curves.set(curve, fn);
  return fn;
}

interface MotionParams {
  speed?: Speed;
}

export function fade(_node: Element, { speed = 'normal' }: MotionParams = {}): TransitionConfig {
  return { duration: duration(speed), easing: easing('out'), css: (t) => `opacity: ${t}` };
}

/** Fades in while travelling from `from` to its place: toasts, panels, rows. */
export function shift(
  _node: Element,
  { speed = 'slow', from = 'below', by = 'md' }: MotionParams & { from?: Side; by?: Distance } = {},
): TransitionConfig {
  const offset = distance(by) * (from === 'below' || from === 'right' ? 1 : -1);
  const horizontal = from === 'left' || from === 'right';
  return {
    duration: duration(speed),
    easing: easing('out'),
    // `translate`, not `transform`: `animate:` moves a leaving item through `transform`.
    css: (t, u) =>
      `opacity: ${t}; translate: ${horizontal ? `${u * offset}px 0` : `0 ${u * offset}px`}`,
  };
}

/** Flyouts and popovers: grow from slightly smaller toward their anchor; set `transform-origin` on the node. */
export function pop(_node: Element, { speed = 'normal' }: MotionParams = {}): TransitionConfig {
  const from = 1 - (parseFloat(token('--motion-scale-delta')) || 0);
  return {
    duration: duration(speed),
    easing: easing('out'),
    css: (t) => `opacity: ${t}; scale: ${from + (1 - from) * t}`,
  };
}

/** Dialogs settle from slightly larger, as Windows' content dialogs do. */
export function settle(_node: Element, { speed = 'slow' }: MotionParams = {}): TransitionConfig {
  const from = 1 + (parseFloat(token('--motion-scale-delta')) || 0);
  return {
    duration: duration(speed),
    easing: easing('out'),
    css: (t) => `opacity: ${t}; scale: ${from + (1 - from) * t}`,
  };
}

/** Height (or width) reveal with a fade, for inline panels that push what follows. */
export function expand(
  node: Element,
  { speed = 'normal', axis = 'y' }: MotionParams & { axis?: 'x' | 'y' } = {},
): TransitionConfig {
  const base = slide(node, { duration: duration(speed), easing: easing('out'), axis });
  return { ...base, css: (t, u) => `${base.css?.(t, u) ?? ''}; opacity: ${t}` };
}

/** Slides `to`'s pseudo-element (a selection pill or underline) over from where `from`'s sat; only the control that changed animates. Along `x` it also stretches to the new width, as a segment pill does; along `y` it only travels, as a list's marker does. */
export function glide(
  from: HTMLElement,
  to: HTMLElement,
  { pseudoElement = '::before', axis = 'x' }: { pseudoElement?: string; axis?: 'x' | 'y' } = {},
): void {
  const time = duration('normal');
  if (time === 0) return;
  const transform =
    axis === 'x'
      ? `translateX(${String(from.offsetLeft - to.offsetLeft)}px) scaleX(${String(to.offsetWidth === 0 ? 1 : from.offsetWidth / to.offsetWidth)})`
      : `translateY(${String(from.offsetTop - to.offsetTop)}px)`;
  to.animate(
    { transform: [transform, 'none'] },
    { duration: time, easing: token('--ease-out') || 'ease-out', pseudoElement },
  );
}

/** `animate:` for keyed lists whose siblings move when one is added or removed. */
export function reflow(node: Element, rects: { from: DOMRect; to: DOMRect }): AnimationConfig {
  return flip(node, rects, { duration: duration('normal'), easing: easing('out') });
}
