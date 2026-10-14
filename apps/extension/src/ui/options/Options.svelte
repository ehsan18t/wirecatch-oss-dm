<script lang="ts">
  // The options page: every setting the extension edits in the app, in sections, with a search over their names, descriptions and the words people may look for them by.
  import { PRODUCT_NAME } from '@wirecatch/product';
  import {
    AppMark,
    Button,
    Callout,
    EmptyState,
    Input,
    matchesSearch,
    provideToaster,
    Toaster,
    ToastRegion,
  } from '@wirecatch/ui';
  import { untrack } from 'svelte';
  import Search from '~icons/lucide/search';
  import SearchX from '~icons/lucide/search-x';
  import Unplug from '~icons/lucide/unplug';
  import type { PopupModel } from '../../lib/popup/model.svelte';
  import AccessNotice from '../common/AccessNotice.svelte';
  import ConnectionBadge from '../common/ConnectionBadge.svelte';
  import ConnectionNotice from '../common/ConnectionNotice.svelte';
  import { cachedNote, waitingNote } from '../connection';
  import { OPTION_SECTIONS, searchable, type OptionSection, type SettingKey } from '../settings';
  import CaptureSection from './CaptureSection.svelte';
  import ConnectionSection from './ConnectionSection.svelte';
  import FiltersSection from './FiltersSection.svelte';
  import MediaSection from './MediaSection.svelte';
  import SitesSection from './SitesSection.svelte';

  interface Props {
    model: PopupModel;
    /** False when the person revoked the extension's access to websites (possible in Firefox). */
    siteAccess?: boolean;
    /** Asks the browser to grant website access again; resolves with the new state. */
    requestSiteAccess?: () => Promise<boolean>;
    /** The keyboard shortcut that downloads a page's media; null when none is set. */
    mediaShortcut?: string | null;
    /** Opens the browser's page for extension shortcuts; null where a page cannot open it. */
    openShortcuts?: (() => void) | null;
  }

  let {
    model,
    siteAccess = true,
    requestSiteAccess,
    mediaShortcut = null,
    openShortcuts = null,
  }: Props = $props();

  const toaster = new Toaster();
  provideToaster(toaster);

  // The outcome of an action, such as a change the app refused, comes as a toast: the page has no fixed place for it.
  $effect(() => {
    const feedback = model.feedback;
    if (feedback === null) return;
    untrack(() => toaster.show(feedback.tone === 'error' ? 'danger' : 'success', feedback.text));
  });

  let accessGranted = $state<boolean | null>(null);
  const hasSiteAccess = $derived(accessGranted ?? siteAccess);

  async function allowAccess(): Promise<void> {
    if (requestSiteAccess === undefined) return;
    accessGranted = await requestSiteAccess();
  }

  let query = $state('');
  let searchBox = $state<HTMLInputElement | null>(null);
  const show = (key: SettingKey): boolean => matchesSearch(searchable(key), query);
  const shown = (section: OptionSection): boolean => section.settings.some(show);

  function clearSearch(): void {
    query = '';
    searchBox?.focus();
  }

  const snapshot = $derived(model.snapshot);
  const status = $derived(snapshot?.status ?? { state: 'connecting' as const });
  const problem = $derived(status.state === 'unavailable' ? status.problem : null);
  const cached = $derived(cachedNote(snapshot));
  const settings = $derived(model.settings);
  /** The sections the search finds, whether or not their settings are here yet. */
  const matched = $derived(OPTION_SECTIONS.filter(shown));
  const sections = $derived(matched.filter((s) => settings !== null || s.id === 'connection'));
  /** Settings the search finds that cannot show before Wirecatch first answers: the page says it is waiting, never that nothing matches. */
  const waiting = $derived(settings === null && matched.some((s) => s.id !== 'connection'));
</script>

<svelte:head><title>{PRODUCT_NAME} settings</title></svelte:head>

<div class="mx-auto flex w-full max-w-dialog-lg flex-col gap-6 px-6 pt-10 pb-16">
  <header class="flex flex-col gap-4">
    <div class="flex flex-wrap items-center gap-x-3 gap-y-2">
      <AppMark size="2xl" />
      <h1 class="font-display text-title font-semibold tracking-display text-ink">
        {PRODUCT_NAME} settings
      </h1>
      <div class="ml-auto">
        <ConnectionBadge {status} />
      </div>
    </div>
    <p class="text-body text-ink-muted">
      {PRODUCT_NAME} keeps these settings, so a change here applies in every browser connected to it.
    </p>
    <Input
      type="search"
      size="lg"
      icon={Search}
      label="Search settings"
      placeholder="Search settings"
      value={query}
      bind:ref={searchBox}
      onchange={(value: string) => {
        query = value;
      }}
      onkeydown={(event: KeyboardEvent) => {
        if (event.key === 'Escape' && query !== '') {
          event.preventDefault();
          clearSearch();
        }
      }}
    />
  </header>

  <main class="flex flex-col gap-6">
    {#if problem !== null}
      <ConnectionNotice
        {problem}
        note={cached}
        onopen={() => void model.openApp()}
        onretry={() => {
          model.reconnect();
        }}
      />
    {/if}
    {#if !hasSiteAccess}
      <AccessNotice onallow={allowAccess} />
    {/if}
    {#if cached !== null && problem === null}
      <Callout tone="neutral">{cached}</Callout>
    {/if}

    {#if waiting}
      <EmptyState icon={Unplug} tone="neutral" title={waitingNote(status)} />
    {/if}

    {#each sections as section (section.id)}
      {#if section.id === 'connection'}
        <!-- While the notice above offers what to do, the row only says how things stand. -->
        <ConnectionSection
          title={section.title}
          {model}
          {status}
          {show}
          actions={problem === null}
        />
      {:else if settings !== null}
        {#if section.id === 'capture'}
          <CaptureSection
            title={section.title}
            {model}
            {settings}
            queues={snapshot?.queues ?? []}
            {show}
          />
        {:else if section.id === 'filters'}
          <FiltersSection title={section.title} {model} {settings} {show} />
        {:else if section.id === 'sites'}
          <SitesSection title={section.title} {model} {settings} {show} />
        {:else if section.id === 'media'}
          <MediaSection
            title={section.title}
            {model}
            {settings}
            shortcut={mediaShortcut}
            {openShortcuts}
            {show}
          />
        {/if}
      {/if}
    {/each}
    {#if matched.length === 0}
      <EmptyState
        icon={SearchX}
        tone="neutral"
        title="No settings match “{query}”"
        description="Try another word, such as queue, size or site."
      >
        {#snippet actions()}
          <Button onclick={clearSearch}>Clear search</Button>
        {/snippet}
      </EmptyState>
    {/if}
  </main>
</div>

<ToastRegion {toaster} />
