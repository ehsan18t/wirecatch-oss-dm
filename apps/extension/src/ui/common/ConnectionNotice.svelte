<script lang="ts">
  // What keeps the extension from the app, what to do about it, and the button that does it: open a closed app, or try again.
  import { Button, Callout } from '@wirecatch/ui';
  import { describeProblem, type Problem } from '../../lib/native/status';

  interface Props {
    problem: Problem;
    /** The heading the title takes in the page's outline. */
    heading?: 'h2' | 'h3';
    /** What the page shows meanwhile, such as the last saved settings: one line here rather than a notice of its own. */
    note?: string | null;
    onopen: () => void;
    onretry: () => void;
  }

  let { problem, heading = 'h2', note = null, onopen, onretry }: Props = $props();

  const text = $derived(describeProblem(problem.kind));
  // A closed app is a state to know about, not a fault.
  const closed = $derived(problem.kind === 'app_closed');
</script>

<Callout tone={closed ? 'info' : 'warning'}>
  <svelte:element this={heading} class="font-semibold text-ink">{text.title}</svelte:element>
  <p>{text.fix}</p>
  {#if problem.detail}
    <p class="mt-1 text-small wrap-break-word">Details: {problem.detail}</p>
  {/if}
  {#if note !== null}
    <p class="mt-1 text-small">{note}</p>
  {/if}
  <div class="mt-2.5 flex flex-wrap gap-2">
    {#if closed}
      <Button size="sm" variant="primary" onclick={onopen}>Open Wirecatch</Button>
    {:else}
      <Button size="sm" onclick={onretry}>Try again</Button>
    {/if}
  </div>
</Callout>
