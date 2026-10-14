<script lang="ts">
  // Windows 11's caption buttons: flush with the window's top edge, 46px wide, the glyphs drawn on a 10px grid, close turning red. Window calls go through the injected controls. For snap layouts the window can take the pointer over the maximize button: the button then reports where it is and draws the look it is told.
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

  const BUTTON = cn(
    'flex h-full w-caption cursor-default items-center justify-center text-ink no-drag inactive:text-ink-subtle',
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
        class="size-glyph"
        viewBox="0 0 10 10"
        fill="none"
        stroke="currentColor"
        aria-hidden="true"
      >
        <path d="M0 5.5h10" />
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
        class="size-glyph"
        viewBox="0 0 10 10"
        fill="none"
        stroke="currentColor"
        aria-hidden="true"
      >
        {#if maximized}
          <rect x="0.5" y="2.5" width="7" height="7" rx="1.5" />
          <path d="M2.5 0.5h5.5a1.5 1.5 0 0 1 1.5 1.5v5.5" />
        {:else}
          <rect x="0.5" y="0.5" width="9" height="9" rx="1.5" />
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
      class="size-glyph"
      viewBox="0 0 10 10"
      fill="none"
      stroke="currentColor"
      aria-hidden="true"
    >
      <path d="M0.5 0.5l9 9M9.5 0.5l-9 9" />
    </svg>
  </button>
</div>
