<script lang="ts" module>
  import type { ShortcutHelp } from '$lib/keymap';

  /** Where a group of shortcuts works, as its heading says it. */
  const SCOPE_TITLE: Record<ShortcutHelp['scope'], string> = {
    window: 'Anywhere in the window',
    list: 'In the download list',
  };
</script>

<script lang="ts">
  // Every keyboard shortcut, grouped by where it works. Escape or the close button closes it.
  import { Dialog, Kbd } from '@wirecatch/ui';
  import { shortcutHelp } from '$lib/keymap';
  import { getApp } from '$lib/state/app.svelte';

  const app = getApp();
  const lines = shortcutHelp();
  const groups = (['window', 'list'] as const).map((scope) => ({
    scope,
    lines: lines.filter((l) => l.scope === scope),
  }));
</script>

<Dialog open title="Keyboard shortcuts" size="lg" onclose={() => app.closeDialog()}>
  <div class="flex flex-col gap-5 pb-1">
    {#each groups as group (group.scope)}
      <section class="flex flex-col gap-1.5">
        <h3 class="text-small font-semibold text-ink-muted">{SCOPE_TITLE[group.scope]}</h3>
        <dl class="flex flex-col divide-y divide-line rounded-md border border-line">
          {#each group.lines as line (line.what)}
            <div class="flex items-center justify-between gap-4 px-3 py-2">
              <dt class="text-ui text-ink">{line.what}</dt>
              <dd class="flex shrink-0 items-center gap-1.5 text-small text-ink-subtle">
                {#each line.keys as combo, i (combo.join('+'))}
                  {#if i > 0}<span>or</span>{/if}
                  <Kbd keys={combo} />
                {/each}
              </dd>
            </div>
          {/each}
        </dl>
      </section>
    {/each}
  </div>
</Dialog>
