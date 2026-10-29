<script lang="ts">
  // The window's caption buttons: small rounded buttons set in from the edge, with Windows 11's minimize, maximize, restore and close shapes in a muted colour; only the one under the pointer fills, grey, or red for Close. Window calls go through the injected controls. For snap layouts the window can take the pointer over the maximize button: the button then reports where it is and draws the look it is told.
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

  const GLYPH = 'size-2.5 fill-none stroke-current';

  const BUTTON = cn(
    'flex h-7 w-8 cursor-default items-center justify-center rounded-md text-ink-muted no-drag inactive:text-ink-subtle',
    FOCUS_RING_INSET,
  );
  const PLAIN = 'hover:bg-wash hover:text-ink active:bg-wash-strong';
</script>

<div class="flex h-full shrink-0 items-center gap-1 pr-2">
  {#if !closeOnly}
    <button
      type="button"
      class={cn(BUTTON, PLAIN)}
      aria-label="Minimize"
      {@attach tooltip(() => 'Minimize')}
      onclick={() => run('minimize', () => controls.minimize())}
    >
      <svg
        class={GLYPH}
        viewBox="0 0 10 10"
        stroke-width="1.2"
        stroke-linecap="round"
        aria-hidden="true"
      >
        <path d="M1 5h8" />
      </svg>
    </button>
    <button
      type="button"
      class={cn(
        BUTTON,
        PLAIN,
        maximizeState === 'hover' && 'bg-wash text-ink',
        maximizeState === 'pressed' && 'bg-wash-strong text-ink',
      )}
      aria-label={maximized ? 'Restore' : 'Maximize'}
      data-maximize
      {@attach tooltip(() => (maximized ? 'Restore' : 'Maximize'))}
      {@attach reportPlace}
      onclick={() => run('maximize', () => controls.toggleMaximize())}
    >
      <svg
        class={GLYPH}
        viewBox="0 0 10 10"
        stroke-width="1.2"
        stroke-linecap="round"
        aria-hidden="true"
      >
        {#if maximized}
          <rect x="1" y="3" width="6" height="6" rx="1.5" />
          <path d="M3 1h4.5A1.5 1.5 0 0 1 9 2.5V7" />
        {:else}
          <rect x="1" y="1" width="8" height="8" rx="2" />
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
      viewBox="0 0 10 10"
      stroke-width="1.2"
      stroke-linecap="round"
      aria-hidden="true"
    >
      <path d="M1.5 1.5l7 7M8.5 1.5l-7 7" />
    </svg>
  </button>
</div>
