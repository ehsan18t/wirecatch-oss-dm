<script lang="ts">
  // Network: the app's proxy, which every queue that follows it uses, the saved proxy profiles, and the hosts reached directly (D73). Each change applies to downloads that start after it.
  import { MAX_PROXY_HOST_CHARS, type ProxyUse } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Badge, Field, RadioGroup, Switch, TagInput, type SelectOption } from '@wirecatch/ui';
  import { lastTestLine, proxyAddress, queueProxy, shownName } from '$lib/proxy';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import ProxyProfileList from './ProxyProfileList.svelte';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const controls: Record<SettingIn<'network'>, Snippet<[Row]>> = {
    proxy: proxyChoice,
    proxy_profiles: profileList,
    proxy_exceptions: exceptions,
    ftp_direct: ftpDirect,
  };

  const SYSTEM = 'system';
  const DIRECT = 'direct';

  /** Every proxy to choose from, keyed as the radio group takes them. */
  const options = $derived<SelectOption[]>([
    {
      value: SYSTEM,
      label: 'Use the computer’s proxy',
      description: 'The proxy set in your network settings, or in the environment.',
    },
    { value: DIRECT, label: 'No proxy', description: 'Connect to every server directly.' },
    ...app.proxies.list.map((p) => ({
      value: String(p.id),
      label: shownName(p.settings),
      description: proxyAddress(p.settings),
    })),
  ]);

  /** The app's exceptions matter while it, or a queue that follows them, uses a proxy (D73.12). */
  const exceptionsMatter = $derived(
    settings.proxy.proxy.kind !== 'direct' ||
      app.queues.list.some(
        (q) =>
          q.settings.exceptions.source === 'follow_app' &&
          queueProxy(q, settings.proxy).kind !== 'direct',
      ),
  );

  function keyOf(proxy: ProxyUse): string {
    return proxy.kind === 'profile' ? String(proxy.id) : proxy.kind;
  }

  function proxyOf(key: string): ProxyUse {
    if (key === SYSTEM || key === DIRECT) return { kind: key };
    const profile = app.proxies.list.find((p) => String(p.id) === key);
    return profile ? { kind: 'profile', id: profile.id } : { kind: 'system' };
  }

  function normalizeException(text: string): string | null {
    const entry = text.trim().toLowerCase();
    if (entry === '' || entry.length > MAX_PROXY_HOST_CHARS || /[\s,;]/.test(entry)) return null;
    return entry;
  }
</script>

<SettingRows
  {items}
  {settings}
  {flash}
  {controls}
  stacked={['proxy', 'proxy_profiles', 'proxy_exceptions']}
/>

{#snippet proxyChoice({ def, s }: Row)}
  <RadioGroup
    label={def.label}
    hideLabel
    cards
    class="w-full"
    value={keyOf(s.proxy.proxy)}
    {options}
    onchange={(key: string) => save((d) => void (d.proxy.proxy = proxyOf(key)))}
  >
    {#snippet aside(option: SelectOption)}
      {const profile = app.proxies.list.find((p) => String(p.id) === option.value)}
      {#if profile}
        {const status = lastTestLine(profile.last_test, app.clock.now)}
        <Badge size="sm" dot tone={status.tone}>{status.text}</Badge>
      {/if}
    {/snippet}
  </RadioGroup>
{/snippet}

{#snippet profileList()}
  <ProxyProfileList />
{/snippet}

{#snippet exceptions({ def, s }: Row)}
  {#if exceptionsMatter}
    <div class="flex w-full flex-col gap-4">
      <Switch
        label="The hosts the computer’s proxy skips"
        description="Its bypass list: NO_PROXY, or the exceptions in your network settings."
        checked={s.proxy.exceptions.system}
        onchange={(on: boolean) => save((d) => void (d.proxy.exceptions.system = on))}
      />
      <Switch
        label="These hosts"
        description="Names cover the names under them. Also addresses, networks like 10.0.0.0/8, and <local> for names without a dot."
        checked={s.proxy.exceptions.use_hosts}
        onchange={(on: boolean) => save((d) => void (d.proxy.exceptions.use_hosts = on))}
      />
      {#if s.proxy.exceptions.use_hosts}
        <Field label={def.label} hideLabel>
          {#snippet children(control)}
            <TagInput
              {...control}
              label="Hosts to go directly to"
              placeholder="Add a host, like intranet.example.com"
              values={s.proxy.exceptions.hosts}
              normalize={normalizeException}
              onchange={(hosts: string[]) => save((d) => void (d.proxy.exceptions.hosts = hosts))}
            />
          {/snippet}
        </Field>
      {/if}
    </div>
  {:else}
    <p class="text-small text-ink-muted">
      Nothing goes through a proxy now, so there is nothing to go around.
    </p>
  {/if}
{/snippet}

{#snippet ftpDirect({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.proxy.ftp_direct}
    onchange={(on: boolean) => save((d) => void (d.proxy.ftp_direct = on))}
  />
{/snippet}
