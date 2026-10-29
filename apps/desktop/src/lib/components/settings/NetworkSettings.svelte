<script lang="ts">
  // Network: the app's proxy, which every queue that follows it uses, the saved proxy servers, and the hosts reached directly (D73), each change applying to downloads that start after it; then the sites that may reach the person's own network (D39).
  import { MAX_PROXY_HOST_CHARS, normalizeSite, type ProxyUse } from '@wirecatch/protocol';
  import type { Snippet } from 'svelte';
  import { Checkbox, Field, Select, TagInput, type FieldControl } from '@wirecatch/ui';
  import { HOSTS_HINT, queueProxy, shownName } from '$lib/proxy';
  import type { SettingIn } from '$lib/settings-schema';
  import { getApp } from '$lib/state/app.svelte';
  import type { Row, SectionProps } from './row';
  import ProxyServerActions from './ProxyServerActions.svelte';
  import ProxyServerList from './ProxyServerList.svelte';
  import SettingRows from './SettingRows.svelte';
  import SettingToggle from './SettingToggle.svelte';

  let { items, settings, flash, save }: SectionProps = $props();

  const app = getApp();

  const controls: Record<SettingIn<'network'>, Snippet<[Row]>> = {
    proxy: proxyChoice,
    proxy_servers: profileList,
    proxy_exceptions: exceptions,
    ftp_bypass_proxy: ftpBypassProxy,
    local_sites: localSites,
  };

  const SYSTEM = 'system';
  const DIRECT = 'direct';

  /** Every proxy to choose from; the saved ones are listed, with their tests, under Proxy servers. */
  const options = $derived([
    { value: SYSTEM, label: 'System proxy' },
    { value: DIRECT, label: 'No proxy' },
    ...app.proxies.list.map((p) => ({ value: String(p.id), label: shownName(p.settings) })),
  ]);

  /** The app's exceptions matter while it, or a queue that follows them, uses a proxy (D73.12). */
  const exceptionsMatter = $derived(
    settings.proxy.proxy.kind !== 'direct' ||
      app.queues.list.some(
        (q) =>
          q.settings.exceptions.source === 'global' &&
          queueProxy(q, settings.proxy).kind !== 'direct',
      ),
  );

  function keyOf(proxy: ProxyUse): string {
    return proxy.kind === 'server' ? String(proxy.id) : proxy.kind;
  }

  function proxyOf(key: string): ProxyUse {
    if (key === SYSTEM || key === DIRECT) return { kind: key };
    const profile = app.proxies.list.find((p) => String(p.id) === key);
    return profile ? { kind: 'server', id: profile.id } : { kind: 'system' };
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
  stacked={['proxy_servers', 'proxy_exceptions', 'local_sites']}
  actions={{ proxy_servers: proxyActions }}
/>

{#snippet proxyActions()}<ProxyServerActions />{/snippet}

{#snippet proxyChoice({ s, control }: Row)}
  <Select
    {...control}
    class="w-72 max-w-full"
    value={keyOf(s.proxy.proxy)}
    {options}
    onchange={(key: string) => save((d) => void (d.proxy.proxy = proxyOf(key)))}
  />
{/snippet}

{#snippet profileList()}
  <ProxyServerList />
{/snippet}

{#snippet exceptions({ def, s }: Row)}
  {#if exceptionsMatter}
    <div class="flex w-full flex-col gap-3">
      <Checkbox
        label="System proxy exceptions"
        help="The bypass list from your system proxy settings or environment variables."
        checked={s.proxy.exceptions.system}
        onchange={(on: boolean) => save((d) => void (d.proxy.exceptions.system = on))}
      />
      <Checkbox
        label="Global hosts"
        checked={s.proxy.exceptions.use_hosts}
        onchange={(on: boolean) => save((d) => void (d.proxy.exceptions.use_hosts = on))}
      />
      {#if s.proxy.exceptions.use_hosts}
        <Field label={def.label} hideLabel description={HOSTS_HINT}>
          {#snippet children(control: FieldControl)}
            <TagInput
              {...control}
              label="Global hosts"
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
    <p class="text-small text-ink-muted">No proxy is in use, so there is nothing to bypass.</p>
  {/if}
{/snippet}

{#snippet ftpBypassProxy({ def, s, control }: Row)}
  <SettingToggle
    {control}
    label={def.label}
    checked={s.proxy.ftp_bypass_proxy}
    onchange={(on: boolean) => save((d) => void (d.proxy.ftp_bypass_proxy = on))}
  />
{/snippet}

{#snippet localSites({ def, s, control }: Row)}
  <TagInput
    {...control}
    label={def.label}
    placeholder="Add a site, like wiki.example.com"
    values={s.local_sites}
    normalize={normalizeSite}
    onchange={(sites: string[]) => save((d) => void (d.local_sites = sites))}
  />
{/snippet}
