<script lang="ts">
  // One proxy profile: its name, type, server, port and login, with Test connection before it is saved (D73.1). A saved password is never shown; it stays only while the type, server, port and user name stay (D71.2).
  import {
    MAX_PROXY_HOST_CHARS,
    MAX_PROXY_NAME_CHARS,
    type ProxyKind,
    type ProxyProfileDraft,
    type ProxyProfileId,
    type ProxyProfileSettings,
    type ProxyTestResult,
  } from '@wirecatch/protocol';
  import {
    Button,
    Callout,
    Dialog,
    Field,
    Input,
    NumberInput,
    SegmentedControl,
    type FieldControl,
  } from '@wirecatch/ui';
  import {
    KIND_OPTIONS,
    MAX_PORT,
    proxyAddress,
    sameProxy,
    testResultText,
    usualPort,
  } from '$lib/proxy';
  import { getApp } from '$lib/state/app.svelte';

  let { id }: { id: ProxyProfileId | null } = $props();
  const app = getApp();
  const formId = $props.id();

  // A snapshot of the profile when the dialog opened; the form edits its own copy.
  const original = (() => (id === null ? undefined : app.proxies.byId(id)))();
  const opened = original?.settings;

  let name = $state(opened?.name ?? '');
  let kind = $state<ProxyKind>(opened?.kind ?? 'http');
  let host = $state(opened?.host ?? '');
  let port = $state<number | null>(opened?.port ?? null);
  let user = $state(opened?.user ?? '');
  /** What happens to the password: the saved one kept, the one typed, or none. */
  let password = $state<
    { change: 'keep' } | { change: 'set'; text: string } | { change: 'remove' }
  >({ change: 'keep' });
  let result = $state<ProxyTestResult | null>(null);
  let testing = $state(false);
  let busy = $state(false);

  const settings = $derived<ProxyProfileSettings>({
    name: name.trim(),
    kind,
    host: host.trim(),
    port: port ?? usualPort(kind),
    user: user.trim(),
  });
  const sameServer = $derived(opened !== undefined && sameProxy(opened, settings));
  const savedPassword = $derived(original?.has_password === true);
  /** The saved password stays: none typed or removed, and the proxy is the same one. */
  const keepsSaved = $derived(savedPassword && password.change === 'keep' && sameServer);
  const loginEntered = $derived(
    settings.user !== '' || keepsSaved || (password.change === 'set' && password.text !== ''),
  );
  const resultText = $derived(result ? testResultText(result, loginEntered) : null);

  const passwordHelp = $derived.by(() => {
    if (password.change === 'remove') return 'The saved password is removed when you save.';
    if (password.change === 'set') return 'Kept in this app only; it is never shown again.';
    if (keepsSaved) return 'A password is saved. Type a new one to replace it.';
    if (savedPassword) {
      return 'The saved password was for the old server or user name, so it is not kept. Enter it again if this one asks for it.';
    }
    return 'Only if the proxy asks for one. Kept in this app only.';
  });

  function draft(): ProxyProfileDraft {
    const d: ProxyProfileDraft = { settings: $state.snapshot(settings) };
    if (password.change === 'set') d.password = password.text;
    if (password.change === 'remove') d.password = null;
    return d;
  }

  /** A change the form makes; an earlier test no longer says anything about it. */
  function edit(change: () => void): void {
    change();
    result = null;
  }

  async function test() {
    if (testing) return;
    testing = true;
    result = null;
    try {
      result = (await app.proxies.test(original?.id ?? null, draft())) ?? null;
    } finally {
      testing = false;
    }
  }

  async function save() {
    if (busy) return;
    busy = true;
    try {
      if ((await app.proxies.save(original?.id ?? null, draft())) !== undefined) app.closeDialog();
    } finally {
      busy = false;
    }
  }
</script>

<Dialog
  open
  title={opened ? `Edit ${name.trim() || proxyAddress(opened)}` : 'New proxy'}
  description={opened
    ? 'Downloads already running keep the proxy they started with.'
    : 'Save a proxy server once; the app and each queue can then choose it.'}
  onclose={() => app.closeDialog()}
>
  <form
    id={formId}
    class="flex flex-col gap-5 pb-1"
    onsubmit={(e) => {
      e.preventDefault();
      void save();
    }}
  >
    <Field label="Name (optional)" description="Left empty, the proxy shows as its address.">
      {#snippet children(control: FieldControl)}
        <Input
          {...control}
          value={name}
          maxlength={MAX_PROXY_NAME_CHARS}
          placeholder="For example: Office"
          onchange={(value: string) => (name = value)}
        />
      {/snippet}
    </Field>

    <div class="flex flex-col gap-1.5">
      <span class="text-ui font-medium text-ink" aria-hidden="true">Type</span>
      <SegmentedControl
        label="Type"
        value={kind}
        options={KIND_OPTIONS}
        onchange={(next: ProxyKind) =>
          edit(() => {
            if (port === usualPort(kind)) port = usualPort(next);
            kind = next;
          })}
      />
    </div>

    <div class="flex flex-wrap items-start gap-3">
      <Field
        label="Server"
        class="min-w-48 flex-1"
        description="A host name or address, without http://."
      >
        {#snippet children(control: FieldControl)}
          <Input
            {...control}
            value={host}
            maxlength={MAX_PROXY_HOST_CHARS}
            placeholder="proxy.example.com"
            autocomplete="off"
            spellcheck={false}
            required
            onchange={(value: string) => edit(() => (host = value))}
          />
        {/snippet}
      </Field>
      <Field label="Port" class="w-32">
        {#snippet children(control: FieldControl)}
          <NumberInput
            {...control}
            value={port}
            placeholder={String(usualPort(kind))}
            min={1}
            max={MAX_PORT}
            optional
            onchange={(value: number | null) => edit(() => (port = value))}
          />
        {/snippet}
      </Field>
    </div>

    <div class="flex flex-wrap items-start gap-3">
      <Field label="User name (optional)" class="min-w-40 flex-1">
        {#snippet children(control: FieldControl)}
          <Input
            {...control}
            value={user}
            autocomplete="off"
            spellcheck={false}
            onchange={(value: string) => edit(() => (user = value))}
          />
        {/snippet}
      </Field>
      <Field label="Password" class="min-w-40 flex-1" description={passwordHelp}>
        {#snippet children(control: FieldControl)}
          <Input
            {...control}
            type="password"
            autocomplete="new-password"
            value={password.change === 'set' ? password.text : ''}
            placeholder={keepsSaved ? '••••••••' : ''}
            disabled={password.change === 'remove'}
            onchange={(text: string) =>
              edit(() => (password = text === '' ? { change: 'keep' } : { change: 'set', text }))}
          />
        {/snippet}
      </Field>
    </div>
    {#if savedPassword}
      <div class="-mt-3 flex justify-end">
        {#if password.change === 'remove'}
          <Button
            size="sm"
            variant="ghost"
            onclick={() => edit(() => (password = { change: 'keep' }))}
            >Keep the saved password</Button
          >
        {:else}
          <Button
            size="sm"
            variant="ghost"
            onclick={() => edit(() => (password = { change: 'remove' }))}
            >Remove the saved password</Button
          >
        {/if}
      </div>
    {/if}

    {#if resultText}
      <Callout tone={resultText.tone} live="polite">{resultText.text}</Callout>
    {/if}
  </form>

  {#snippet footer()}
    <Button class="mr-auto" variant="secondary" loading={testing} onclick={() => void test()}
      >Test connection</Button
    >
    <Button onclick={() => app.closeDialog()}>Cancel</Button>
    <Button type="submit" form={formId} variant="primary" loading={busy}>
      {opened ? 'Save' : 'Save proxy'}
    </Button>
  {/snippet}
</Dialog>
