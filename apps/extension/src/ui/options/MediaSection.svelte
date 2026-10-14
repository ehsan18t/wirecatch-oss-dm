<script lang="ts">
  import type { ExtensionSettings } from '@wirecatch/protocol';
  import {
    Button,
    Kbd,
    SettingRow,
    SettingsSection,
    SettingSwitch,
    type FieldControl,
  } from '@wirecatch/ui';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS, type SettingKey } from '../settings';

  interface Props {
    title: string;
    model: PopupModel;
    settings: ExtensionSettings;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    shortcut: string | null;
    /** Opens the browser's page for extension shortcuts; null where a page cannot open it (Firefox). */
    openShortcuts: (() => void) | null;
    show: (key: SettingKey) => boolean;
  }

  let { title, model, settings, shortcut, openShortcuts, show }: Props = $props();

  const keys = $derived(shortcut === null ? [] : shortcut.split('+'));
  const shortcutHelp = $derived(
    openShortcuts === null
      ? `${SETTINGS.mediaShortcut.description} Change it in your browser's extension shortcuts.`
      : SETTINGS.mediaShortcut.description,
  );
</script>

<SettingsSection {title}>
  {#if show('mediaButton')}
    <SettingSwitch
      id="media-button"
      label={SETTINGS.mediaButton.label}
      description={SETTINGS.mediaButton.description}
      checked={settings.show_media_button}
      disabled={!model.editable}
      onchange={(checked: boolean) => void model.update({ show_media_button: checked })}
    />
  {/if}
  {#if show('mediaShortcut')}
    <SettingRow labelled={false} label={SETTINGS.mediaShortcut.label} description={shortcutHelp}>
      {#snippet children(control: FieldControl)}
        {#if keys.length > 0}
          <Kbd {keys} />
        {:else}
          <span class="text-ui text-ink-muted">Not set</span>
        {/if}
        {#if openShortcuts !== null}
          <Button
            size="sm"
            aria-describedby={control['aria-describedby']}
            onclick={() => {
              openShortcuts();
            }}>Change shortcut</Button
          >
        {/if}
      {/snippet}
    </SettingRow>
  {/if}
</SettingsSection>
