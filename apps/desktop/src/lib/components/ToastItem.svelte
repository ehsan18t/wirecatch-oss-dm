<script lang="ts">
  import CircleAlert from '@lucide/svelte/icons/circle-alert';
  import X from '@lucide/svelte/icons/x';
  import { folderOf, progressFraction } from '$lib/format';
  import { getApp, type Toast } from '$lib/state/app.svelte';
  import Button from './Button.svelte';
  import Icon from './Icon.svelte';
  import IconButton from './IconButton.svelte';

  let { toast }: { toast: Toast } = $props();
  const app = getApp();

  const LIFETIME = { info: 4000, error: 9000, capture: 12000 } as const;

  let hovered = $state(false);
  let focused = $state(false);
  const row = $derived(
    toast.downloadId === undefined ? undefined : app.downloads.get(toast.downloadId),
  );
  const fraction = $derived(row ? progressFraction(row.done, row.total) : null);
  const ask = $derived(app.settings?.extension.ask_before_download ?? true);

  // Count down only while the person is not looking at or using the note.
  $effect(() => {
    if (hovered || focused) return;
    const timer = setTimeout(() => app.toasts.dismiss(toast.id), LIFETIME[toast.kind]);
    return () => clearTimeout(timer);
  });
</script>

<div
  class="toast {toast.kind}"
  role={toast.kind === 'error' ? 'alert' : undefined}
  onmouseenter={() => (hovered = true)}
  onmouseleave={() => (hovered = false)}
  onfocusin={() => (focused = true)}
  onfocusout={() => (focused = false)}
>
  {#if toast.kind === 'capture' && row}
    <div class="body">
      <p class="title">Downloading <strong>{row.file_name}</strong></p>
      <p class="sub">to {folderOf(row.path)}</p>
      <div class="actions">
        <Button small onclick={() => app.chooseFolderFor(row.id)}>Change</Button>
        <Button small onclick={() => app.openFolder(row.id)}>Open folder</Button>
        <Button variant="ghost" small onclick={() => app.setAskBeforeDownload(!ask)}>
          {ask ? 'Start without asking' : 'Show a window first next time'}
        </Button>
      </div>
    </div>
    <span class="wire" aria-hidden="true">
      <span
        class="fill"
        style:transform="scaleX({row.status === 'completed' ? 1 : (fraction ?? 0)})"
      ></span>
    </span>
  {:else}
    <div class="body line">
      {#if toast.kind === 'error'}<Icon icon={CircleAlert} size="md" />{/if}
      <p>{toast.message}</p>
    </div>
  {/if}
  <IconButton class="close" icon={X} label="Dismiss" onclick={() => app.toasts.dismiss(toast.id)} />
</div>

<style>
  .toast {
    position: relative;
    display: flex;
    align-items: flex-start;
    gap: 8px;
    width: 360px;
    max-width: calc(100vw - 32px);
    padding: 12px 8px 12px 14px;
    overflow: hidden;
    border: 1px solid var(--line);
    border-radius: var(--radius-l);
    background: var(--surface);
    box-shadow: var(--shadow);
    animation: arrive 180ms var(--ease);
  }

  .toast.error {
    border-color: color-mix(in srgb, var(--danger) 45%, var(--line));
  }

  .body {
    flex: 1;
    min-width: 0;
  }

  .line {
    display: flex;
    gap: 8px;
  }

  .line :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--danger);
  }

  p {
    margin: 0;
    line-height: 1.45;
  }

  .title {
    overflow-wrap: anywhere;
  }

  .sub {
    color: var(--ink-2);
    font-size: var(--text-sm);
    overflow-wrap: anywhere;
  }

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }

  .toast > :global(.close) {
    flex: none;
    margin-top: -4px;
  }

  .wire {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    height: 3px;
    background: var(--wire-track);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 240ms linear;
  }

  @keyframes arrive {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
</style>
