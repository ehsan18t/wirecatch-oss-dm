<script lang="ts">
  // One of the app's options, drawn with the popup's own controls (D279). A kind this version does not know shows its name and a way to the app's settings, so a newer app never breaks the popup.
  import { PRODUCT_NAME } from '@wirecatch/product';
  import { Button, SettingRow, SettingSwitch } from '@wirecatch/ui';
  import type { ParsedOption } from '../../../lib/options';
  import type { PopupModel } from '../../../lib/popup/model.svelte';
  import ChoiceOption from './ChoiceOption.svelte';
  import NumberOption from './NumberOption.svelte';
  import PageSiteRow from './PageSiteRow.svelte';
  import ShortcutRow from './ShortcutRow.svelte';

  interface Props {
    model: PopupModel;
    option: ParsedOption;
    /** Address of the active tab, for the page's own row. */
    currentUrl: string | null;
    /** The popup is in a private window. */
    privateTab: boolean;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    shortcut: string | null;
    /** Opens the browser's page of extension shortcuts; null where no page can open it (Firefox). */
    openShortcuts: (() => void) | null;
  }

  let { model, option, currentUrl, privateTab, shortcut, openShortcuts }: Props = $props();

  const editable = $derived(model.editable);
</script>

{#if option.kind === 'toggle'}
  <SettingSwitch
    id="option-{option.id}"
    label={option.label}
    help={option.help ?? undefined}
    checked={option.value}
    disabled={!editable || option.disabled !== null}
    onchange={(on: boolean) => void model.setOption(option.id, { set: on })}
  />
{:else if option.kind === 'choice' || option.kind === 'choices'}
  <ChoiceOption {model} {option} />
{:else if option.kind === 'number' || option.kind === 'limit'}
  <NumberOption {model} {option} />
{:else if option.kind === 'action'}
  <SettingRow label={option.label} help={option.help ?? undefined} labelled={false}>
    <Button
      size="sm"
      disabled={!editable || option.disabled !== null}
      onclick={() => void model.invokeAction(option.id, null)}
    >
      {option.label}
    </Button>
  </SettingRow>
{:else if option.kind === 'note'}
  <p class="px-4 py-3 text-small text-ink-muted">{option.label}</p>
{:else if option.kind === 'browser'}
  {#if option.row === 'shortcut'}
    <ShortcutRow {shortcut} {openShortcuts} />
  {:else}
    <PageSiteRow {model} {currentUrl} {privateTab} />
  {/if}
{:else}
  <SettingRow label={option.label} labelled={false}>
    <Button size="sm" onclick={() => void model.openSettings()}>Open in {PRODUCT_NAME}</Button>
  </SettingRow>
{/if}
