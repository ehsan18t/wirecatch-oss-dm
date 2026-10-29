<script lang="ts">
  // The frameless window's title bar. Every part of it that is not a control drags the window, and a double-click there maximizes it: Tauri's drag region covers the whole bar (`deep`), and the CSS app region with it; buttons, fields and links opt out by themselves (`drag` in tokens.css). It quiets while the window is not in front, as Windows does.
  import type { Snippet } from 'svelte';
  import { cn } from '../lib/cn';
  import { CHROME } from '../recipes/control';
  import CaptionButtons from './CaptionButtons.svelte';
  import type { CaptionAction, WindowControls } from './window-controls';

  interface Props {
    /** The window's name, shown beside the app icon. */
    title: string;
    /** The window calls; without them there are no caption buttons (a page in a browser). */
    controls?: WindowControls | undefined;
    /** Only a close button, for a small window. */
    closeOnly?: boolean;
    /** The app icon, before the title. */
    icon?: Snippet;
    /** Before the icon: a navigation toggle. */
    leading?: Snippet;
    /** Centred: a search field. */
    center?: Snippet;
    /** Before the caption buttons: window-wide actions. */
    trailing?: Snippet;
    onerror?: (action: CaptionAction, error: unknown) => void;
    class?: string | undefined;
  }

  let {
    title,
    controls,
    closeOnly = false,
    icon,
    leading,
    center,
    trailing,
    onerror,
    class: className,
  }: Props = $props();

  let inactive = $state(false);
</script>

<svelte:window onblur={() => (inactive = true)} onfocus={() => (inactive = false)} />

<header
  class={cn(
    'relative z-titlebar flex h-titlebar shrink-0 items-stretch text-ink drag',
    CHROME,
    className,
  )}
  data-inactive={inactive || undefined}
  data-tauri-drag-region="deep"
>
  {#if leading}
    <div class="flex items-center pl-1.5">{@render leading()}</div>
  {/if}
  <div class={cn('flex min-w-0 shrink items-center gap-2.5', leading ? 'pl-1.5' : 'pl-4')}>
    {#if icon}
      <span class="flex size-icon-md shrink-0 items-center justify-center inactive:opacity-quiet"
        >{@render icon()}</span
      >
    {/if}
    <span class="truncate text-small text-ink inactive:text-ink-subtle">{title}</span>
  </div>
  <div class="flex min-w-0 flex-1 items-center justify-center px-3">
    {#if center}
      <div class="w-full max-w-prose">{@render center()}</div>
    {/if}
  </div>
  {#if trailing}
    <div class="flex items-center gap-0.5 pr-1">{@render trailing()}</div>
  {/if}
  {#if controls}
    <CaptionButtons {controls} {closeOnly} {onerror} />
  {/if}
</header>
