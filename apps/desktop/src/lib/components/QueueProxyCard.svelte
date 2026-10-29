<script lang="ts">
  // The queue editor's "Proxy" card: the queue's proxy, the app's or its own, with New proxy, and the hosts it reaches directly: the app's, or chosen from the computer's bypass list, the app's hosts and its own (D73.2, D73.7).
  import Plus from '~icons/lucide/plus';
  import {
    GLOBAL,
    MAX_PROXY_HOST_CHARS,
    ownSetting,
    type ProxyServerId,
    type ProxyUse,
    type QueueExceptions,
    type Setting,
  } from '@wirecatch/protocol';
  import { Button, Checkbox, Field, Select, TagInput, type FieldControl } from '@wirecatch/ui';
  import { defaultProxySettings, proxyName, shownName } from '$lib/proxy';
  import { getApp } from '$lib/state/app.svelte';
  import ChoiceCard from './ChoiceCard.svelte';
  import ProxyServerDialog from './ProxyServerDialog.svelte';

  let {
    queueName,
    proxy,
    exceptions,
    onchange,
  }: {
    /** The queue's name as the form has it, for the new proxy dialog. */
    queueName: string;
    proxy: Setting<ProxyUse>;
    exceptions: Setting<QueueExceptions>;
    onchange: (proxy: Setting<ProxyUse>, exceptions: Setting<QueueExceptions>) => void;
  } = $props();

  const app = getApp();
  const appProxy = $derived(app.settings?.proxy ?? defaultProxySettings());
  /** The proxy its downloads go through, its own or the app's. */
  const resulting = $derived(proxy.source === 'own' ? proxy.value : appProxy.proxy);
  let creating = $state(false);

  const FOLLOW = 'follow';
  const SYSTEM = 'system';
  const DIRECT = 'direct';

  const appProxyName = $derived.by(() => {
    const name = proxyName(appProxy.proxy, app.proxies.list);
    return name;
  });

  const options = $derived([
    {
      value: FOLLOW,
      label: `Global proxy (${appProxyName})`,
    },
    { value: SYSTEM, label: 'System proxy' },
    { value: DIRECT, label: 'No proxy' },
    ...app.proxies.list.map((p) => ({ value: String(p.id), label: shownName(p.settings) })),
  ]);

  function keyOf(setting: Setting<ProxyUse>): string {
    if (setting.source === 'global') return FOLLOW;
    return setting.value.kind === 'server' ? String(setting.value.id) : setting.value.kind;
  }

  function choose(key: string) {
    const profile = app.proxies.list.find((p) => String(p.id) === key);
    if (key === FOLLOW) onchange(GLOBAL, exceptions);
    else if (key === SYSTEM || key === DIRECT) onchange(ownSetting({ kind: key }), exceptions);
    else if (profile) onchange(ownSetting({ kind: 'server', id: profile.id }), exceptions);
  }

  function created(id?: ProxyServerId) {
    creating = false;
    if (id !== undefined) onchange(ownSetting({ kind: 'server', id }), exceptions);
  }

  /** The lists a queue starts from when it stops following the app's: the app's own choice. */
  function fromApp(): QueueExceptions {
    return {
      system: appProxy.exceptions.system,
      app_hosts: appProxy.exceptions.use_hosts,
      use_hosts: false,
      hosts: [],
    };
  }

  const own = $derived(exceptions.source === 'own' ? exceptions.value : null);
  const change = (patch: Partial<QueueExceptions>) => {
    if (own) onchange(proxy, ownSetting({ ...own, ...patch }));
  };

  const appHostCount = $derived(appProxy.exceptions.hosts.length);

  function normalizeException(text: string): string | null {
    const entry = text.trim().toLowerCase();
    if (entry === '' || entry.length > MAX_PROXY_HOST_CHARS || /[\s,;]/.test(entry)) return null;
    return entry;
  }
</script>

<div class="flex flex-col divide-y divide-line rounded-md border border-line bg-layer">
  <div class="flex flex-col gap-1.5 p-3">
    <div class="flex flex-wrap items-end gap-2">
      <Field label="Proxy" class="min-w-56 flex-1">
        {#snippet children(control: FieldControl)}
          <Select {...control} value={keyOf(proxy)} {options} onchange={choose} />
        {/snippet}
      </Field>
      <Button icon={Plus} onclick={() => (creating = true)}>New proxy</Button>
    </div>
    {#if proxy.source === 'global'}
      <p class="text-small text-ink-muted">Follows the global proxy setting.</p>
    {/if}
  </div>

  {#if resulting.kind !== 'direct'}
    <ChoiceCard
      flush
      label="Bypass proxy for"
      description={own
        ? 'Hosts this queue connects to directly.'
        : 'Hosts this queue connects to directly. Follows the global list.'}
      value={own ? 'own' : 'app'}
      options={[
        { value: 'app', label: 'Same as global' },
        { value: 'own', label: 'Custom' },
      ]}
      onchange={(next: 'app' | 'own') =>
        onchange(proxy, next === 'app' ? GLOBAL : ownSetting(fromApp()))}
    />
    {#if own}
      <div class="flex flex-col gap-3 p-3">
        <Checkbox
          label="System proxy exceptions"
          checked={own.system}
          onchange={(on: boolean) => change({ system: on })}
        />
        <Checkbox
          label={appHostCount === 0
            ? 'Global bypass list (empty)'
            : `Global bypass list (${String(appHostCount)})`}
          checked={own.app_hosts}
          onchange={(on: boolean) => change({ app_hosts: on })}
        />
        <Checkbox
          label="These hosts"
          checked={own.use_hosts}
          onchange={(on: boolean) => change({ use_hosts: on })}
        />
        {#if own.use_hosts}
          <Field
            label="Hosts for this queue"
            hideLabel
            description="A domain also matches its subdomains. IP addresses, ranges like 10.0.0.0/8, and <local> (names without a dot) work too."
          >
            {#snippet children(control: FieldControl)}
              <TagInput
                {...control}
                label="Hosts for this queue"
                placeholder="Add a host, like intranet.example.com"
                values={own.hosts}
                normalize={normalizeException}
                onchange={(hosts: string[]) => change({ hosts })}
              />
            {/snippet}
          </Field>
        {/if}
      </div>
    {/if}
  {/if}
  <p class="px-3 py-2.5 text-small text-ink-muted">
    Downloads already running keep the connection they started with.
  </p>
</div>

{#if creating}
  <ProxyServerDialog id={null} forQueue={queueName.trim() || 'this queue'} onclose={created} />
{/if}
