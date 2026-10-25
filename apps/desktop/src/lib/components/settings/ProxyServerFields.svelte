<script lang="ts">
  // One manual proxy's fields: its type where it may choose one, server, port, user name and password. The password is never shown: the form only knows whether one is saved.
  import { MAX_PROXY_HOST_CHARS, type ProxyKind, type ProxyRole } from '@wirecatch/protocol';
  import { Button, Field, Input, NumberInput, Select } from '@wirecatch/ui';
  import { KIND_OPTIONS, MAX_PORT, usualPort } from '$lib/proxy';
  import type { ProxyForm } from '$lib/state/proxy-form.svelte';

  let {
    form,
    role,
    name,
    kinds = false,
  }: {
    form: ProxyForm;
    role: ProxyRole;
    /** The heading of the fields. */
    name: string;
    /** Whether the proxy's type can be chosen; the SOCKS one is always SOCKS5. */
    kinds?: boolean;
  } = $props();

  const entered = $derived(form.draft[role]);
  /** A saved password the person has not touched. */
  const kept = $derived(form.keepsSaved(role));
  /** A port of 0 is not entered yet. */
  const port = $derived(entered.port === 0 ? null : entered.port);
</script>

<fieldset class="flex flex-col gap-3">
  <legend class="mb-2 text-ui font-medium text-ink">{name}</legend>
  <div class="flex flex-wrap items-start gap-3">
    {#if kinds}
      <Field label="Type" class="w-32">
        {#snippet children(control)}
          <Select
            {...control}
            value={entered.kind}
            options={KIND_OPTIONS}
            onchange={(kind: ProxyKind) => {
              form.edit((d) => {
                d[role].kind = kind;
              });
            }}
          />
        {/snippet}
      </Field>
    {/if}
    <Field label="Server" class="min-w-48 flex-1">
      {#snippet children(control)}
        <Input
          {...control}
          value={entered.host}
          placeholder="proxy.example.com"
          spellcheck="false"
          autocomplete="off"
          maxlength={MAX_PROXY_HOST_CHARS}
          onchange={(host: string) => {
            form.edit((d) => {
              d[role].host = host;
            });
          }}
        />
      {/snippet}
    </Field>
    <Field
      label="Port"
      description="Often {usualPort(role === 'socks' ? 'socks5' : entered.kind)}"
      class="w-28"
    >
      {#snippet children(control)}
        <NumberInput
          {...control}
          min={1}
          max={MAX_PORT}
          optional
          value={port}
          onchange={(value: number | null) => {
            form.edit((d) => {
              d[role].port = value ?? 0;
            });
          }}
        />
      {/snippet}
    </Field>
  </div>
  <div class="flex flex-wrap items-start gap-3">
    <Field label="User name" description="Only if the proxy asks for one." class="min-w-40 flex-1">
      {#snippet children(control)}
        <Input
          {...control}
          value={entered.user}
          autocomplete="off"
          spellcheck="false"
          onchange={(user: string) => {
            form.edit((d) => {
              d[role].user = user;
            });
          }}
        />
      {/snippet}
    </Field>
    <Field
      label="Password"
      description={kept
        ? 'Saved. Type a new one to replace it.'
        : 'Kept on this computer, never in exported settings.'}
      class="min-w-40 flex-1"
    >
      {#snippet children(control)}
        <Input
          {...control}
          type="password"
          autocomplete="new-password"
          value={form.passwords[role] ?? ''}
          placeholder={kept ? '••••••••' : ''}
          onchange={(password: string) => {
            form.setPassword(role, password);
          }}
        >
          {#snippet trailing()}
            {#if kept}
              <Button
                variant="ghost"
                size="sm"
                aria-label="Forget the saved password for {name}"
                onclick={() => {
                  form.setPassword(role, null);
                }}>Forget</Button
              >
            {/if}
          {/snippet}
        </Input>
      {/snippet}
    </Field>
  </div>
</fieldset>
