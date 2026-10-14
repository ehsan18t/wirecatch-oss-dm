<script lang="ts">
  // The app these settings belong to: whether it is connected, and the button that opens it or tries again. A "Copy diagnostics" row belongs here too (W5's EXT-7).
  import { Button, SettingRow, SettingsSection, type FieldControl } from '@wirecatch/ui';
  import { describeProblem, type ConnectionStatus } from '../../lib/native/status';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import { SETTINGS, type SettingKey } from '../settings';

  interface Props {
    title: string;
    model: PopupModel;
    status: ConnectionStatus;
    show: (key: SettingKey) => boolean;
    /** False while a notice on the page already offers the same button. */
    actions?: boolean;
  }

  let { title, model, status, show, actions = true }: Props = $props();

  const summary = $derived.by(() => {
    switch (status.state) {
      case 'connected':
        return `Connected, version ${status.appVersion}.`;
      case 'connecting':
        return 'Connecting to Wirecatch.';
      case 'unavailable':
        return describeProblem(status.problem.kind).title;
    }
  });
  /** Open the app when there is one to open; otherwise the only thing to do is to try again. */
  const opens = $derived(
    status.state === 'connected' ||
      (status.state === 'unavailable' && status.problem.kind === 'app_closed'),
  );
</script>

<SettingsSection {title}>
  {#if show('app')}
    <SettingRow labelled={false} label={SETTINGS.app.label} description={summary}>
      {#snippet children(control: FieldControl)}
        {#if actions}
          <Button
            size="sm"
            variant={opens ? 'secondary' : 'primary'}
            disabled={status.state === 'connecting'}
            aria-describedby={control['aria-describedby']}
            onclick={() => {
              if (opens) void model.openApp();
              else model.reconnect();
            }}>{opens ? 'Open Wirecatch' : 'Try again'}</Button
          >
        {/if}
      {/snippet}
    </SettingRow>
  {/if}
</SettingsSection>
