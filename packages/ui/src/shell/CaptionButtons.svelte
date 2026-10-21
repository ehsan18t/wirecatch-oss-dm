<script lang="ts">
  // Windows 11's caption buttons: flush with the window's top edge, 46px wide, close turning red. The glyphs are Wirecatch's own "wire" set on a 12px grid: round-capped strokes, and maximize a loop of wire left open at one corner that closes under the pointer. Window calls go through the injected controls. For snap layouts the window can take the pointer over the maximize button: the button then reports where it is and draws the look it is told.
  import { cn } from '../lib/cn';
  import { tooltip } from '../attachments/tooltip.svelte';
  import { FOCUS_RING_INSET } from '../recipes/control';
  import type { CaptionAction, MaximizeState, WindowControls } from './window-controls';

  interface Props {
    controls: WindowControls;
    /** Hides minimize and maximize, for a small window that only closes (a download window may). */
    closeOnly?: boolean;
    /** Called when a window call fails, with what was being done. */
    onerror?: (action: CaptionAction, error: unknown) => void;
  }

  let { controls, closeOnly = false, onerror }: Props = $props();

  let maximized = $state(false);
  // While Windows' snap layouts hold the pointer over the maximize button, the OS says how it should look.
  let maximizeState = $state<MaximizeState>('rest');

  /** Starts a listener the window gives back a stop function for, and stops it with the effect, however late it arrives. */
  function listen(start: () => Promise<() => void> | undefined): () => void {
    let live = true;
    let stop: (() => void) | undefined;
    start()?.then(
      (unlisten) => {
        if (live) stop = unlisten;
        else unlisten();
      },
      (error: unknown) => onerror?.('maximize', error),
    );
    return () => {
      live = false;
      stop?.();
    };
  }

  $effect(() => {
    let live = true;
    const sync = () => {
      controls.isMaximized().then(
        (value) => {
          if (live) maximized = value;
        },
        (error: unknown) => onerror?.('maximize', error),
      );
    };
    sync();
    const stopResized = listen(() => controls.onResized(sync));
    const stopState = listen(() =>
      controls.onMaximizeState?.((state) => {
        maximizeState = state;
      }),
    );
    return () => {
      live = false;
      stopResized();
      stopState();
    };
  });

  /** Tells the window where the maximize button is, now and whenever it moves or changes size. */
  function reportPlace(button: HTMLElement) {
    const report = controls.setMaximizeButton?.bind(controls);
    if (!report) return;
    const send = () => {
      const { x, y, width, height } = button.getBoundingClientRect();
      report({ x, y, width, height });
    };
    send();
    const observer = new ResizeObserver(send);
    observer.observe(button);
    window.addEventListener('resize', send);
    return () => {
      observer.disconnect();
      window.removeEventListener('resize', send);
    };
  }

  function run(action: CaptionAction, call: () => Promise<void>) {
    call().catch((error: unknown) => onerror?.(action, error));
  }

  const GLYPH = 'size-3 fill-none stroke-current';

  const BUTTON = cn(
    'group flex h-full w-caption cursor-default items-center justify-center text-ink no-drag inactive:text-ink-subtle',
    FOCUS_RING_INSET,
  );
</script>

<div class="flex h-full shrink-0 items-stretch">
  {#if !closeOnly}
    <button
      type="button"
      class={cn(BUTTON, 'hover:bg-wash active:bg-wash-strong')}
      aria-label="Minimize"
      {@attach tooltip(() => 'Minimize')}
      onclick={() => run('minimize', () => controls.minimize())}
    >
      <svg
        class={GLYPH}
        viewBox="0 0 12 12"
        stroke-width="1.3"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        <path d="M2.5 6h7" />
      </svg>
    </button>
    <button
      type="button"
      class={cn(
        BUTTON,
        'hover:bg-wash active:bg-wash-strong',
        maximizeState === 'hover' && 'bg-wash',
        maximizeState === 'pressed' && 'bg-wash-strong',
      )}
      aria-label={maximized ? 'Restore' : 'Maximize'}
      data-maximize
      {@attach tooltip(() => (maximized ? 'Restore' : 'Maximize'))}
      {@attach reportPlace}
      onclick={() => run('maximize', () => controls.toggleMaximize())}
    >
      <svg
        class={GLYPH}
        viewBox="0 0 12 12"
        stroke-width="1.3"
        stroke-linecap="round"
        stroke-linejoin="round"
        aria-hidden="true"
      >
        {#if maximized}
          <rect x="1.5" y="3.5" width="7" height="7" rx="1.5" />
          <path d="M3.5 1.5H9A1.5 1.5 0 0 1 10.5 3v5.5" />
        {:else}
          <path
            d="M4.5 1.5H9A1.5 1.5 0 0 1 10.5 3v6A1.5 1.5 0 0 1 9 10.5H3A1.5 1.5 0 0 1 1.5 9V4.5"
          />
          <!-- The loop's open corner, which closes under the pointer. -->
          <path
            d="M1.5 4.5V3A1.5 1.5 0 0 1 3 1.5h1.5"
            class="opacity-0 transition-opacity duration-normal ease-out group-hover:opacity-100"
          />
        {/if}
      </svg>
    </button>
  {/if}
  <button
    type="button"
    class={cn(
      BUTTON,
      'hover:bg-caption-close hover:text-white active:bg-caption-close/pressed active:text-white/pressed',
    )}
    aria-label="Close"
    {@attach tooltip(() => 'Close')}
    onclick={() => run('close', () => controls.close())}
  >
    <svg
      class={GLYPH}
      viewBox="0 0 12 12"
      stroke-width="1.3"
      stroke-linecap="round"
      stroke-linejoin="round"
      aria-hidden="true"
    >
      <path d="M2.75 2.75l6.5 6.5M9.25 2.75l-6.5 6.5" />
    </svg>
  </button>
</div>
