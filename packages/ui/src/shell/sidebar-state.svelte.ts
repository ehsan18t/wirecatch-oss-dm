import { createContext } from 'svelte';
import { MediaQuery } from 'svelte/reactivity';
import { token } from '../lib/css-token';

/**
 * How the sidebar shows, after Windows' navigation view: docked with labels in a wide window (unless the person collapsed it), docked as an icon rail otherwise, and in a narrow window the toggle opens it as a drawer over the content.
 */
export type SidebarMode = 'expanded' | 'rail';

export interface SidebarOptions {
  /** Starts collapsed to the rail; the app keeps the person's choice in its settings. */
  collapsed?: boolean;
  /** The window width from which the sidebar docks with its labels, as a CSS length; `--breakpoint-nav` by default. */
  breakpoint?: string;
}

/** A writable `$derived` built on this starts false, and turns false again whenever `dependency` changes. */
const closedOnChange = (_dependency: unknown): boolean => false;

export class SidebarState {
  /** The person's choice for a wide window. */
  collapsed = $state(false);
  /** Null where the stylesheet is not there to give the breakpoint (unit tests): the window then counts as wide. */
  readonly #wide: MediaQuery | null;

  constructor({ collapsed = false, breakpoint }: SidebarOptions = {}) {
    this.collapsed = collapsed;
    const width = breakpoint ?? token('--breakpoint-nav');
    this.#wide = width === '' ? null : new MediaQuery(`(min-width: ${width})`, true);
  }

  /** Whether the window is wide enough to dock the sidebar with its labels. */
  get wide(): boolean {
    return this.#wide?.current ?? true;
  }

  /** Open over the content, in a narrow window. Closed whenever the window crosses the breakpoint, so widening and narrowing again never reopens it and takes focus. */
  drawerOpen = $derived(closedOnChange(this.wide));

  get mode(): SidebarMode {
    return this.wide && !this.collapsed ? 'expanded' : 'rail';
  }

  /** Labels are showing: docked expanded, or open as a drawer. */
  get showLabels(): boolean {
    return this.mode === 'expanded' || this.drawerOpen;
  }

  toggle(): void {
    if (this.wide) {
      this.collapsed = !this.collapsed;
      this.drawerOpen = false;
    } else {
      this.drawerOpen = !this.drawerOpen;
    }
  }

  closeDrawer(): void {
    this.drawerOpen = false;
  }
}

/** What a sidebar's items read from it. */
export interface SidebarContext {
  readonly showLabels: boolean;
  /** Called by the item that has become the current one, so the marker can travel from the last. */
  activated: (item: HTMLElement) => void;
  /** Called after choosing an item, so a drawer closes. */
  chose: () => void;
}

export const [getSidebarContext, setSidebarContext] = createContext<SidebarContext>();
