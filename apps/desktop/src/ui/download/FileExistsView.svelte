<script lang="ts">
  // A download whose file is already in its folder (D88): replace it, keep both, or cancel, and whether to remember the choice. Keep both is the main button: it never loses a file.
  import type { FileExistsChoice, FileExistsQuestion } from '@wirecatch/protocol';
  import { formatBytes } from '@wirecatch/units';
  import { Button, Checkbox, textIfCut, tooltip } from '@wirecatch/ui';
  import { whenSaid } from '$lib/format';
  import type { DownloadWindowState } from '$lib/window/download-window.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win, question }: { win: DownloadWindowState; question: FileExistsQuestion } = $props();

  let remember = $state(false);
  const saved = $derived(
    question.saved_at === null ? '' : `, saved ${whenSaid(question.saved_at, win.clock.now)}`,
  );

  function answer(choice: FileExistsChoice) {
    void win.answerFileExists(choice, remember);
  }
</script>

<WindowFrame title="This file is already in the folder" error={win.error}>
  <div class="flex flex-col gap-3 text-ui text-ink">
    <div class="flex flex-col gap-1">
      <p class="text-small text-ink-muted">In the folder now</p>
      <div class="min-w-0 rounded-control border border-line bg-layer px-3 py-2">
        <p class="truncate font-semibold" {@attach tooltip(() => textIfCut(question.file_name))}>
          {question.file_name}
        </p>
        <p class="truncate text-ink-muted" {@attach tooltip(() => textIfCut(question.folder))}>
          {question.folder}
        </p>
        <p class="text-ink-muted">{formatBytes(question.size)}{saved}</p>
      </div>
    </div>
    <p>
      A file with the same name and size is already there, so it is probably this download. Replace
      overwrites it; Keep both saves this one under a new name.
    </p>
    <div class="border-t border-line pt-4">
      <Checkbox
        checked={remember}
        label="Remember my choice"
        help="Replace or Keep both is saved in Settings > Saving files and used from then on. Cancel is never remembered."
        onchange={(on: boolean) => (remember = on)}
      />
    </div>
  </div>

  {#snippet footer()}
    <Button variant="ghost" disabled={win.busy} onclick={() => answer('cancel')}>Cancel</Button>
    <Button disabled={win.busy} onclick={() => answer('replace')}>Replace</Button>
    <Button variant="primary" data-autofocus loading={win.busy} onclick={() => answer('keep_both')}
      >Keep both</Button
    >
  {/snippet}
</WindowFrame>
