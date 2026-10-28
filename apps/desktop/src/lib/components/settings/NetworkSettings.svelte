<script lang="ts">
  // Network: the app's proxy, which every queue that follows it uses, and the hosts it reaches directly (D73). Each change applies to downloads that start after it.
  import { MAX_PROXY_HOST_CHARS, type ProxyUse } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Field, RadioGroup, Switch, TagInput } from '@wirecatch/ui';
  import { proxyAddress, shownName } from '$lib/proxy';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import SettingRows from './SettingRows.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const controls: Record<SettingIn<'network'>, Snippet<[Row]>> = {
    proxy: proxyChoice,
  };

  const SYSTEM = 'system';
  const DIRECT = 'direct';

  /** Every proxy to choose from, keyed as the radio group takes them. */
  const options = $derived([
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

<SettingRows {items} {settings} {flash} {controls} stacked={['proxy']} />

{#snippet proxyChoice({ def, s }: Row)}
  <div class="flex w-full flex-col gap-5">
    <RadioGroup
      label={def.label}
      hideLabel
      cards
      value={keyOf(s.proxy.proxy)}
      {options}
      onchange={(key: string) => save((d) => void (d.proxy.proxy = proxyOf(key)))}
    />

    {#if s.proxy.proxy.kind !== 'direct'}
      <div class="flex flex-col gap-4 rounded-md border border-line p-4">
        <Switch
          label="Go directly to the hosts the computer’s proxy skips"
          description="Its bypass list: NO_PROXY, or the exceptions in your network settings."
          checked={s.proxy.exceptions.system}
          onchange={(on: boolean) => save((d) => void (d.proxy.exceptions.system = on))}
        />
        <Switch
          label="Go directly to these hosts"
          checked={s.proxy.exceptions.use_hosts}
          onchange={(on: boolean) => save((d) => void (d.proxy.exceptions.use_hosts = on))}
        />
        {#if s.proxy.exceptions.use_hosts}
          <Field
            label="Hosts"
            description="Names cover the names under them. Also addresses, networks like 10.0.0.0/8, and <local> for names without a dot."
          >
            {#snippet children(control)}
              <TagInput
                {...control}
                label="Hosts"
                placeholder="Add a host, like intranet.example.com"
                values={s.proxy.exceptions.hosts}
                normalize={normalizeException}
                onchange={(hosts: string[]) => save((d) => void (d.proxy.exceptions.hosts = hosts))}
              />
            {/snippet}
          </Field>
        {/if}
        <Switch
          label="Download FTP links directly"
          description="Only a SOCKS5 proxy can carry FTP. Off: FTP links with another kind of proxy fail rather than go around it."
          checked={s.proxy.ftp_direct}
          onchange={(on: boolean) => save((d) => void (d.proxy.ftp_direct = on))}
        />
      </div>
    {/if}
  </div>
{/snippet}
