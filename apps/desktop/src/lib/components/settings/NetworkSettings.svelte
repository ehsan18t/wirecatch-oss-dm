<script lang="ts">
  // Network: the proxy everything the app sends goes through (D71). Unlike the other settings it is saved as a whole with Save, after an optional Test connection, so a proxy half typed never breaks the downloads meanwhile; once saved while downloads run, it asks whether to restart them.
  import { MAX_PROXY_HOST_CHARS, type ProxyMode } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Button, Callout, Dialog, Field, RadioGroup, Switch, TagInput } from '@wirecatch/ui';
  import { MODE_OPTIONS, PER_PROTOCOL, testResultText } from '$lib/proxy';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import ProxyServerFields from './ProxyServerFields.svelte';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash }: SectionProps = $props();

  const app = getApp();
  const form = app.proxy;
  const draft = $derived(form.draft);

  const controls: Record<SettingIn<'network'>, Snippet<[Row]>> = {
    proxy: proxyForm,
  };

  const running = $derived(app.counts.status.active - app.counts.joining);
  /** Whether any proxy in use was given a user name or password, for the words of a refused login. */
  const loginEntered = $derived(
    (draft.per_protocol ? PER_PROTOCOL.map((p) => p.role) : (['all'] as const)).some(
      (role) => draft[role].user.trim() !== '' || form.hasPassword(role),
    ),
  );
  const result = $derived(form.result ? testResultText(form.result, loginEntered) : null);

  function normalizeException(text: string): string | null {
    const entry = text.trim().toLowerCase();
    if (entry === '' || entry.length > MAX_PROXY_HOST_CHARS || /[\s,;]/.test(entry)) return null;
    return entry;
  }
</script>

<SettingRows {items} {settings} {flash} {controls} stacked={['proxy']} />

{#snippet proxyForm({ def }: Row)}
  <div class="flex w-full flex-col gap-5">
    <RadioGroup
      label={def.label}
      hideLabel
      cards
      value={draft.mode}
      options={MODE_OPTIONS}
      onchange={(mode: ProxyMode) => form.edit((d) => void (d.mode = mode))}
    />

    {#if draft.mode === 'manual'}
      <div class="flex flex-col gap-5 rounded-md border border-line p-4">
        <div class="flex flex-wrap items-center justify-between gap-3">
          <Switch
            label="A different proxy for each protocol"
            checked={draft.per_protocol}
            onchange={(on: boolean) => form.edit((d) => void (d.per_protocol = on))}
          />
          <Button size="sm" variant="secondary" onclick={() => void form.copyFromSystem()}
            >Copy from system</Button
          >
        </div>

        {#if draft.per_protocol}
          {#each PER_PROTOCOL as { role, label } (role)}
            <div class="border-t border-line pt-4 first:border-t-0 first:pt-0">
              <ProxyServerFields {form} {role} name={label} kinds={role !== 'socks'} />
            </div>
          {/each}
        {:else}
          <ProxyServerFields {form} role="all" name="Proxy server" kinds />
        {/if}

        <Field
          label="Go directly to"
          description="Names cover the names under them. Also addresses, networks like 10.0.0.0/8, and <local> for names without a dot."
        >
          {#snippet children(control)}
            <TagInput
              {...control}
              label="Go directly to"
              placeholder="Add a host, like intranet.example.com"
              values={draft.exceptions}
              normalize={normalizeException}
              onchange={(hosts: string[]) => form.edit((d) => void (d.exceptions = hosts))}
            />
          {/snippet}
        </Field>

        <Switch
          label="Download FTP links directly"
          description="Only a SOCKS5 proxy can carry FTP. Off: FTP links with another kind of proxy fail rather than go around it."
          checked={draft.ftp_direct}
          onchange={(on: boolean) => form.edit((d) => void (d.ftp_direct = on))}
        />
      </div>
    {/if}

    {#if result}
      <Callout tone={result.tone} live="polite">{result.text}</Callout>
    {/if}

    <div class="flex flex-wrap items-center justify-end gap-2">
      <span class="mr-auto text-small text-ink-muted" role="status">
        {form.notice ?? (form.dirty ? 'Not saved yet.' : '')}
      </span>
      {#if form.dirty}
        <Button variant="ghost" onclick={() => form.reset()}>Discard</Button>
      {/if}
      <Button variant="secondary" loading={form.testing} onclick={() => void form.test()}
        >Test connection</Button
      >
      <Button
        variant="primary"
        disabled={!form.dirty}
        loading={form.saving}
        onclick={() => void form.save()}>Save</Button
      >
    </div>
  </div>
{/snippet}

<Dialog
  open={form.askingRestart}
  title="Restart the running downloads?"
  description={running === 1
    ? 'One download still uses the old proxy. Restarted, it continues on the new one and keeps what it has downloaded. Left running, it switches the next time it starts.'
    : `${String(running)} downloads still use the old proxy. Restarted, they continue on the new one and keep what they have downloaded. Left running, they switch the next time they start.`}
  size="sm"
  onclose={() => (form.askingRestart = false)}
>
  {#snippet footer()}
    <Button variant="secondary" onclick={() => (form.askingRestart = false)}
      >Keep them running</Button
    >
    <Button variant="primary" onclick={() => void form.restartRunning()}>Restart them</Button>
  {/snippet}
</Dialog>
