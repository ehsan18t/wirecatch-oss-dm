<script lang="ts">
  // Test a link: which queue the rules send a link to and why, a switched-off queue that would have matched it included (D72.5, D72.7). The core matches, so this and the downloads never disagree.
  import { tick } from 'svelte';
  import type { DraftRules, QueueId } from '@wirecatch/protocol';
  import { Button, cn, Field, Input, type FieldControl } from '@wirecatch/ui';
  import { trialLines } from '$lib/queue-reasons';
  import { getApp } from '$lib/state/app.svelte';

  let {
    draft,
    draftName = 'This queue',
    inRow = false,
  }: {
    /** One queue's rules as its form has them, tried in its place; none tries the saved rules. */
    draft?: (() => DraftRules) | undefined;
    /** The name of the queue `draft` stands for. */
    draftName?: string;
    /** On a settings row of its own, which shows the name: the field takes the row's width. */
    inRow?: boolean;
  } = $props();

  const app = getApp();
  const formId = $props.id();

  let url = $state('');
  let lines = $state<string[]>([]);
  let problem = $state('');
  /** The problem is the link's, not the rules'. */
  let linkWrong = $state(false);
  let busy = $state(false);
  let result = $state<HTMLElement | null>(null);

  // An answer is for the link and the rules it was given: a change to either takes it away.
  $effect(() => {
    void JSON.stringify(draft?.());
    clear();
  });

  function clear() {
    lines = [];
    problem = '';
  }

  function name(queue: QueueId | null): string {
    const drafted = draft?.();
    if (queue === null || queue === drafted?.queue) return draftName.trim() || 'This queue';
    return app.queues.byId(queue)?.settings.name ?? 'A deleted queue';
  }

  async function run() {
    const link = url.trim();
    if (!link || busy) return;
    busy = true;
    try {
      const found = await app.queues.tryLink({ url: link, draft: draft?.() ?? null });
      if ('problem' in found) {
        ({ problem } = found);
        linkWrong = found.inLink;
        lines = [];
      } else {
        problem = '';
        lines = trialLines(found, name, app.queues.defaultQueue?.settings.name ?? 'Main');
      }
    } finally {
      busy = false;
    }
    // The form may be scrolled so the answer is below the fold.
    await tick();
    result?.scrollIntoView({ block: 'nearest' });
  }
</script>

<div class={cn('flex flex-col gap-2', inRow ? 'w-full' : 'max-w-prose')}>
  <div class="flex items-end gap-2">
    <Field label="Test a link" hideLabel={inRow} class="min-w-0 flex-1">
      {#snippet children(control: FieldControl)}
        <Input
          {...control}
          type="url"
          value={url}
          invalid={linkWrong && problem !== ''}
          aria-describedby="{formId}-result"
          placeholder="https://example.com/file.zip"
          onchange={(value: string) => {
            url = value;
            clear();
          }}
          onkeydown={(e: KeyboardEvent) => {
            // Inside the queue form, Enter tries the link instead of saving the queue.
            if (e.key !== 'Enter') return;
            e.preventDefault();
            void run();
          }}
        />
      {/snippet}
    </Field>
    <Button loading={busy} disabled={url.trim() === ''} onclick={run}>Try</Button>
  </div>
  <div
    bind:this={result}
    id="{formId}-result"
    role="status"
    class="flex flex-col gap-0.5 text-small"
  >
    {#if problem}
      <p class="text-danger">{problem}</p>
    {:else}
      {#each lines as line, i (i)}
        <p class={i === 0 ? 'text-ink' : 'text-ink-muted'}>{line}</p>
      {/each}
    {/if}
  </div>
</div>
