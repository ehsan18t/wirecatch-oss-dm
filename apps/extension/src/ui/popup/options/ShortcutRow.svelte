<script lang="ts">
  // The keyboard shortcut that sends a page's video or audio: the browser's own, which the app only places in the list (D70, D280).
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { Button, Kbd, SettingRow } from '@wirecatch/ui';

  interface Props {
    /** The shortcut; null when none is set. */
    shortcut: string | null;
    /** Opens the browser's page of extension shortcuts; null where no page can open it (Firefox). */
    openShortcuts: (() => void) | null;
  }

  let { shortcut, openShortcuts }: Props = $props();

  const keys = $derived(shortcut === null ? [] : shortcut.split('+'));
  const action = $derived(keys.length > 0 ? 'Change' : 'Set');
  const help = $derived(
    openShortcuts === null
      ? `Sends a page's video or audio to ${PRODUCT_NAME}. Set it in the add-ons manager, under Manage Extension Shortcuts.`
      : `Sends a page's video or audio to ${PRODUCT_NAME}.`,
  );
</script>

<SettingRow label="Keyboard shortcut" {help} labelled={false}>
  {#if keys.length > 0}
    <Kbd {keys} />
  {:else}
    <span class="text-ui text-ink-muted">Not set</span>
  {/if}
  {#if openShortcuts !== null}
    <Button size="sm" aria-label="{action} the keyboard shortcut" onclick={openShortcuts}>
      {action}
    </Button>
  {/if}
</SettingRow>
