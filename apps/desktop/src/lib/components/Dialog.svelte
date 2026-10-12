<script lang="ts">
  import X from '@lucide/svelte/icons/x';
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import IconButton from './IconButton.svelte';

  let {
    title,
    description,
    onclose,
    children,
    footer,
    width = 480,
  }: {
    title: string;
    description?: string;
    onclose: () => void;
    children: Snippet;
    footer?: Snippet;
    width?: number;
  } = $props();

  const uid = $props.id();
  let el = $state<HTMLDialogElement>();

  onMount(() => {
    const previous = document.activeElement;
    if (el) {
      if (typeof el.showModal === 'function') el.showModal();
      else el.setAttribute('open', '');
      // Focus the first field, or the main button, rather than the close button.
      const target =
        el.querySelector<HTMLElement>('[data-autofocus]') ??
        el.querySelector<HTMLElement>('input, select, textarea');
      target?.focus();
    }
    return () => {
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus();
    };
  });

  function oncancel(e: Event) {
    e.preventDefault();
    onclose();
  }

  // Browsers turn Escape into a cancel event; handling the key too keeps behavior identical
  // everywhere, including webviews and tests.
  function onkeydown(e: KeyboardEvent) {
    if (e.key === 'Tab') {
      keepFocusInside(e);
      return;
    }
    if (e.key !== 'Escape') return;
    e.preventDefault();
    e.stopPropagation();
    onclose();
  }

  // A modal dialog makes the page behind it inert, but Tab from its last control still leaves
  // the page (in a webview, to nowhere). Wrap around instead, both ways.
  function keepFocusInside(e: KeyboardEvent) {
    if (!el) return;
    const focusable = [
      ...el.querySelectorAll<HTMLElement>(
        'button, input, select, textarea, a[href], [tabindex]:not([tabindex="-1"])',
      ),
    ].filter((f) => !f.hasAttribute('disabled') && !f.closest('[hidden], [inert]'));
    const first = focusable[0];
    const last = focusable.at(-1);
    if (!first || !last) return;
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }

  function onclick(e: MouseEvent) {
    // A click on the dimmed area around the dialog closes it.
    if (e.target === el) onclose();
  }
</script>

<dialog
  bind:this={el}
  aria-labelledby="{uid}-title"
  aria-describedby={description ? `${uid}-desc` : undefined}
  style:--dialog-width="{width}px"
  {oncancel}
  {onkeydown}
  {onclick}
>
  <div class="panel">
    <header>
      <h2 id="{uid}-title">{title}</h2>
      <IconButton icon={X} size="md" label="Close" onclick={onclose} />
    </header>
    {#if description}
      <p id="{uid}-desc" class="description">{description}</p>
    {/if}
    <div class="body">
      {@render children()}
    </div>
    {#if footer}
      <footer>{@render footer()}</footer>
    {/if}
  </div>
</dialog>

<style>
  dialog {
    width: min(var(--dialog-width), calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    padding: 0;
    border: 1px solid var(--line);
    border-radius: var(--radius-l);
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--shadow);
  }

  dialog[open] {
    animation: rise 160ms var(--ease);
  }

  dialog::backdrop {
    background: var(--scrim);
  }

  .panel {
    display: flex;
    flex-direction: column;
    max-height: calc(100vh - 66px);
  }

  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 16px 16px 0 20px;
  }

  h2 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 620;
  }

  .description {
    margin: 4px 20px 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.5;
  }

  .body {
    padding: 14px 20px 4px;
    overflow-y: auto;
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
    padding: 14px 20px 18px;
  }

  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(6px);
    }
  }
</style>
