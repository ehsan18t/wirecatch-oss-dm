<script lang="ts">
  // The queue editor's "Proxy" card: the queue's proxy, the app's or its own, with New proxy, and the hosts it reaches directly: the app's, or chosen from the computer's bypass list, the app's hosts and its own (D73.2, D73.7).
  import {
    FOLLOW_APP,
    MAX_PROXY_HOST_CHARS,
    ownSetting,
    type ProxyProfileId,
    type ProxyUse,
    type QueueExceptions,
    type Setting,
  } from '@wirecatch/protocol';
  import { Checkbox, Field, Select, TagInput, type FieldControl } from '@wirecatch/ui';
  import { defaultProxySettings, proxyName, shownName } from '$lib/proxy';
  import { getApp } from '$lib/state/app.svelte';
  import ChoiceCard from './ChoiceCard.svelte';
  import ProxyProfileDialog from './ProxyProfileDialog.svelte';

  let {
    proxy,
    exceptions,
    onchange,
  }: {
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
  const NEW = 'new';

  const options = $derived([
    { value: FOLLOW, label: `Same as the app: ${proxyName(appProxy.proxy, app.proxies.list)}` },
    { value: SYSTEM, label: 'The computer’s proxy' },
    { value: DIRECT, label: 'No proxy' },
    ...app.proxies.list.map((p) => ({ value: String(p.id), label: shownName(p.settings) })),
    { value: NEW, label: 'New proxy…' },
  ]);

  function keyOf(setting: Setting<ProxyUse>): string {
    if (setting.source === 'follow_app') return FOLLOW;
    return setting.value.kind === 'profile' ? String(setting.value.id) : setting.value.kind;
  }

  function choose(key: string) {
    if (key === NEW) {
      creating = true;
      return;
    }
    const profile = app.proxies.list.find((p) => String(p.id) === key);
    if (key === FOLLOW) onchange(FOLLOW_APP, exceptions);
    else if (key === SYSTEM || key === DIRECT) onchange(ownSetting({ kind: key }), exceptions);
    else if (profile) onchange(ownSetting({ kind: 'profile', id: profile.id }), exceptions);
  }

  function created(id?: ProxyProfileId) {
    creating = false;
    if (id !== undefined) onchange(ownSetting({ kind: 'profile', id }), exceptions);
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
    <Field label="Proxy" description="Downloads already running keep the proxy they started with.">
      {#snippet children(control: FieldControl)}
        <Select {...control} value={keyOf(proxy)} {options} onchange={choose} />
      {/snippet}
    </Field>
  </div>

  {#if resulting.kind !== 'direct'}
    <ChoiceCard
      flush
      label="Go directly to"
      description={own
        ? 'Hosts this queue reaches without its proxy.'
        : 'The hosts the app goes directly to, as they change.'}
      value={own ? 'own' : 'app'}
      options={[
        { value: 'app', label: 'Same as the app' },
        { value: 'own', label: 'Choose' },
      ]}
      onchange={(next: 'app' | 'own') =>
        onchange(proxy, next === 'app' ? FOLLOW_APP : ownSetting(fromApp()))}
    />
    {#if own}
      <div class="flex flex-col gap-3 p-3">
        <Checkbox
          label="The hosts the computer’s proxy skips"
          checked={own.system}
          onchange={(on: boolean) => change({ system: on })}
        />
        <Checkbox
          label={appHostCount === 0
            ? 'The app’s hosts (none yet)'
            : `The app’s hosts (${String(appHostCount)})`}
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
            description="A name also covers the names under it. Also addresses, networks like 10.0.0.0/8, and <local> for names without a dot."
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
</div>

{#if creating}
  <ProxyProfileDialog id={null} onclose={created} />
{/if}
