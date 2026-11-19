<script lang="ts">
  // The app's option groups (D280): a group without a title is a card that does not fold, one with a title folds under a line that says what it is set to, and opens folded when the app says so. Opening one is a view of the moment, not a setting (D61).
  import { Card, Disclosure } from '@wirecatch/ui';
  import type { ParsedGroup } from '../../lib/options';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import OptionRow from './options/OptionRow.svelte';

  interface Props {
    model: PopupModel;
    groups: readonly ParsedGroup[];
    currentUrl: string | null;
    privateTab: boolean;
    shortcut: string | null;
    openShortcuts: (() => void) | null;
  }

  let { model, groups, currentUrl, privateTab, shortcut, openShortcuts }: Props = $props();
</script>

{#each groups as group (group.id)}
  {#if group.title === null}
    <Card padding="none" class="divide-y divide-line">
      {#each group.options as option (option.id)}
        <OptionRow {model} {option} {currentUrl} {privateTab} {shortcut} {openShortcuts} />
      {/each}
    </Card>
  {:else}
    <Disclosure
      variant="card"
      label={group.title}
      summary={group.summary ?? undefined}
      open={!group.folded}
    >
      {#each group.options as option (option.id)}
        <OptionRow {model} {option} {currentUrl} {privateTab} {shortcut} {openShortcuts} />
      {/each}
    </Disclosure>
  {/if}
{/each}
