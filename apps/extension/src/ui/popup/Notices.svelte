<script lang="ts">
  // What happened in the background that the person should know about, newest first, each with Dismiss.
  import { Button, Callout } from '@wirecatch/ui';
  import type { Notice } from '../../lib/messages';
  import { buttonsIn, RemovalFocus } from '../removal-focus.svelte';

  interface Props {
    notices: readonly Notice[];
    ondismiss: (id: string) => void;
    /** Where focus goes once the last notice is dismissed. */
    fallback: () => HTMLElement | null;
  }

  let { notices, ondismiss, fallback }: Props = $props();

  const uid = $props.id();
  let list = $state<HTMLUListElement | null>(null);
  const focus = new RemovalFocus({
    has: (id) => notices.some((n) => n.id === id),
    buttons: () => buttonsIn(list),
    fallback: () => fallback(),
  });

  function dismiss(id: string, index: number): void {
    focus.removing(id, index);
    ondismiss(id);
  }
</script>

{#if notices.length > 0}
  <ul aria-label="Alerts" class="flex flex-col gap-2" bind:this={list}>
    {#each notices as notice, index (notice.id)}
      {const messageId = $derived(`${uid}-notice-${String(index)}`)}
      <li>
        <Callout tone={notice.tone === 'error' ? 'danger' : 'warning'}>
          <p id={messageId} class="text-ink">{notice.message}</p>
          {#snippet actions()}
            <Button
              variant="ghost"
              size="sm"
              aria-describedby={messageId}
              onclick={() => {
                dismiss(notice.id, index);
              }}>Dismiss</Button
            >
          {/snippet}
        </Callout>
      </li>
    {/each}
  </ul>
{/if}
