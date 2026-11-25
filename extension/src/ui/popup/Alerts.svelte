<script lang="ts">
  // What happened in the background that the person should know about, newest first, each with Dismiss.
  import { Button, Callout } from '@wirecatch/ui';
  import type { Alert } from '../../lib/popup-port';
  import { buttonsIn, RemovalFocus } from '../removal-focus.svelte';

  interface Props {
    alerts: readonly Alert[];
    ondismiss: (id: string) => void;
    /** Where focus goes once the last alert is dismissed. */
    fallback: () => HTMLElement | null;
  }

  let { alerts, ondismiss, fallback }: Props = $props();

  const uid = $props.id();
  let list = $state<HTMLUListElement | null>(null);
  const focus = new RemovalFocus({
    has: (id) => alerts.some((n) => n.id === id),
    buttons: () => buttonsIn(list),
    fallback: () => fallback(),
  });

  function dismiss(id: string, index: number): void {
    focus.removing(id, index);
    ondismiss(id);
  }
</script>

{#if alerts.length > 0}
  <ul aria-label="Alerts" class="flex flex-col gap-2" bind:this={list}>
    {#each alerts as alert, index (alert.id)}
      {const messageId = $derived(`${uid}-alert-${String(index)}`)}
      <li>
        <Callout tone={alert.tone === 'error' ? 'danger' : 'warning'}>
          <p id={messageId} class="text-ink">{alert.message}</p>
          {#snippet actions()}
            <Button
              variant="ghost"
              size="sm"
              aria-describedby={messageId}
              onclick={() => {
                dismiss(alert.id, index);
              }}>Dismiss</Button
            >
          {/snippet}
        </Callout>
      </li>
    {/each}
  </ul>
{/if}
