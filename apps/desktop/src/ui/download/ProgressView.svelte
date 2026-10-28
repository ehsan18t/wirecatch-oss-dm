<script lang="ts">
  // One download's progress, in its own window (D23): name, status and why it waits, progress, speed, time left, connections, any error with its fix, and Pause or Resume, Cancel and Hide. Once finished: Open and Show in folder, and the window closes by itself after a few seconds unless the pointer is over it or it has the keyboard focus, or finished downloads are told in a window (D52): then this one stays, as that window would.
  import FolderOpen from '~icons/lucide/folder-open';
  import type { DownloadSummary, FixAction } from '@wirecatch/protocol';
  import { formatBytes, formatSpeed } from '@wirecatch/units';
  import { Button, Callout, cn, Field, type FieldControl, Input, ProgressBar } from '@wirecatch/ui';
  import {
    extractUrls,
    folderOf,
    formatEta,
    formatPercent,
    formatSize,
    mediaLine,
    PROCESSING_LABEL,
    statusDetail,
    statusShort,
  } from '$lib/format';
  import { reasonPhrase } from '$lib/queue-reasons';
  import { track } from '$lib/state/reactivity';
  import { CLOSE_AFTER_MS, type DownloadWindowState } from '$lib/window/download-window.svelte';
  import CategoryIcon from '../CategoryIcon.svelte';
  import ErrorNotice from '../details/ErrorNotice.svelte';
  import { rowProgress, STATUS_TEXT } from '../status';
  import WindowFrame from './WindowFrame.svelte';

  let { win, row }: { win: DownloadWindowState; row: DownloadSummary } = $props();
  const uid = $props.id();

  type Ask = 'cancel' | 'link';
  /** Where the download stands, as far as a question about it goes: "Cancel this download?" means nothing once it finished, a fresh link nothing once it runs again. */
  const stage = $derived(
    row.status === 'completed' || row.status === 'failed' || row.status === 'cancelled'
      ? row.status
      : 'unfinished',
  );
  /** An inline question shown over the buttons: cancel, or a fresh link. It goes when the download moves to another stage, so its answer never acts on a download it was not asked about. */
  let asking = $derived.by<Ask | null>(() => {
    track(stage);
    return null;
  });
  /** What opened the inline question, which takes the focus back when it closes. */
  let opener: HTMLElement | null = null;
  let link = $state('');
  let linkError = $state<string | null>(null);

  const running = $derived(
    row.status === 'downloading' ||
      row.status === 'connecting' ||
      row.status === 'queued' ||
      row.status === 'processing',
  );
  const finished = $derived(row.status === 'completed');
  const percent = $derived(finished ? '' : formatPercent(row.done, row.total));
  const detail = $derived(statusDetail(row, win.queueName(row.queue)));
  const progress = $derived(rowProgress(row));
  /** "1.2 GB of 4.7 GB, 25%", or the size once finished, or what is known so far. */
  const doneText = $derived.by(() => {
    if (finished || row.total === null) return formatSize(row.done, row.total, row.status);
    const of = `${formatBytes(row.done)} of ${formatBytes(row.total)}`;
    return percent ? `${of}, ${percent}` : of;
  });
  const barLabel = $derived(
    row.status === 'processing' && row.processing
      ? PROCESSING_LABEL[row.processing.step]
      : 'Downloaded',
  );

  // Closes by itself once finished, unless the person is looking at it or using it.
  let hovered = $state(false);
  let focused = $state(false);
  // Opening a program waits for the shell's own warning, which takes the focus from the window.
  let opening = $state(false);
  let root = $state<HTMLDivElement>();
  function updateFocus() {
    focused = document.hasFocus() && !!root && root.contains(document.activeElement);
  }
  /** Told in a window, a finished download stays in this one, which takes that window's place. */
  const staysOpen = $derived(win.settings?.finish_notice === 'window');
  $effect(() => {
    if (!finished || staysOpen || hovered || focused || opening || asking !== null) return;
    const timer = setTimeout(() => win.close(), CLOSE_AFTER_MS);
    return () => clearTimeout(timer);
  });

  function ask(question: Ask) {
    opener = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    asking = question;
  }

  function stopAsking() {
    if (opener?.isConnected && document.activeElement?.closest('[data-asking]')) opener.focus();
    opener = null;
    asking = null;
  }

  /** The footer's main button follows the status, one button throughout, so the focus stays on it from Pause to Resume. A failure with an error offers its own fix instead. */
  const main = $derived.by(() => {
    if (running) return { label: 'Pause', act: () => win.progress.pause() };
    if (row.status === 'failed') {
      return row.error ? null : { label: 'Try again', act: () => win.progress.retry() };
    }
    return {
      label: row.status === 'cancelled' ? 'Start over' : 'Resume',
      act: () => win.progress.resume(),
    };
  });

  async function open() {
    opening = true;
    try {
      await win.progress.openFile();
    } finally {
      opening = false;
    }
  }

  async function saveLink() {
    const found = extractUrls(link)[0];
    if (!found) {
      linkError = 'Paste the full new link, starting with https://.';
      return;
    }
    if (await win.progress.refreshLink(found)) {
      stopAsking();
      link = '';
    }
  }

  /** An inline question takes the focus when it appears. */
  function focusNow(element: HTMLElement) {
    element.focus();
  }

  function onkeydown(event: KeyboardEvent) {
    // Escape closes an inline question first, before it closes the window.
    if (event.key === 'Escape' && asking !== null) {
      event.preventDefault();
      event.stopPropagation();
      stopAsking();
    }
  }

  const ASK = 'flex flex-col gap-3 rounded-md border border-line-strong bg-wash p-3';
</script>

<svelte:window onfocus={updateFocus} onblur={updateFocus} />

<!-- svelte-ignore a11y_no_static_element_interactions (the root watches pointer and focus inside the window so a finished one does not close under the person, and takes Escape for its inline questions first) -->
<div
  bind:this={root}
  class="h-full"
  data-progress-window
  onmouseenter={() => (hovered = true)}
  onmouseleave={() => (hovered = false)}
  onfocusin={updateFocus}
  onfocusout={() => setTimeout(updateFocus)}
  {onkeydown}
>
  <WindowFrame title={row.file_name} error={win.error}>
    <div class="flex flex-col gap-4">
      <section aria-label="Progress" class="flex flex-col gap-3">
        <div class="flex items-center gap-3">
          <CategoryIcon category={row.category} />
          <div class="flex min-w-0 flex-col">
            <p class={cn('text-ui font-semibold', STATUS_TEXT[row.status])} role="status">
              {statusShort(row)}
            </p>
            <p class="text-small text-ink-muted tabular-nums">{doneText}</p>
          </div>
        </div>
        {#if progress}
          <ProgressBar
            value={progress.value}
            state={progress.state}
            label={barLabel}
            valueText={progress.valueText}
          />
        {/if}
      </section>

      {#if row.status === 'failed' && row.error}
        <ErrorNotice
          {row}
          live
          onfix={(fix: FixAction) => void win.progress.fix(fix, () => ask('link'))}
          onretry={() => void win.progress.retry()}
        />
      {:else if detail}
        <Callout tone={row.waiting?.type === 'server_refused' ? 'warning' : 'info'}
          >{detail}</Callout
        >
      {/if}
      {#if row.media}
        <p class="text-ui text-ink-muted">
          {mediaLine(row.media)}
        </p>
      {/if}

      <dl class="grid grid-cols-2 gap-x-4 gap-y-3 tabular-nums">
        {#if !finished}
          <div class="min-w-0">
            <dt class="text-small text-ink-muted">Speed</dt>
            <dd class="text-ui text-ink">{formatSpeed(row.speed) || 'None right now'}</dd>
          </div>
          <div class="min-w-0">
            <dt class="text-small text-ink-muted">Time left</dt>
            <dd class="text-ui text-ink">
              {row.eta_secs !== null && row.speed > 0 ? formatEta(row.eta_secs) : 'Not known yet'}
            </dd>
          </div>
          <div class="col-span-2 min-w-0">
            <dt class="text-small text-ink-muted">Connections</dt>
            <dd class="text-ui text-ink">
              {row.connections > 0 ? `${String(row.connections)} in use` : 'None open'}
            </dd>
          </div>
        {/if}
        <div class="col-span-2 min-w-0">
          <dt class="text-small text-ink-muted">Queue</dt>
          <dd class="text-ui text-ink">
            {win.queueName(row.queue)}
            <span class="text-small text-ink-muted">· {reasonPhrase(row.queue_reason)}</span>
          </dd>
        </div>
        <div class="col-span-2 min-w-0">
          <dt class="text-small text-ink-muted">Saved to</dt>
          <dd class="text-ui text-ink wrap-anywhere">{folderOf(row.path)}</dd>
        </div>
      </dl>

      {#if asking === 'cancel'}
        <div class={ASK} role="group" aria-labelledby="{uid}-cancel" data-asking>
          <p id="{uid}-cancel" class="text-body text-ink">
            Cancel this download? It stops, the part already downloaded is deleted, and it leaves
            the list.
          </p>
          <div class="flex flex-wrap justify-end gap-2">
            <Button {@attach focusNow} onclick={stopAsking}>Keep downloading</Button>
            <Button variant="danger" onclick={() => void win.progress.cancelDownload()}
              >Cancel download</Button
            >
          </div>
        </div>
      {:else if asking === 'link'}
        <form
          class={ASK}
          data-asking
          onsubmit={(event) => {
            event.preventDefault();
            void saveLink();
          }}
        >
          <Field
            label="Fresh link"
            description="Start the download again on the page it came from, copy the new link and paste it here. Everything already downloaded is kept."
            error={linkError}
          >
            {#snippet children(control: FieldControl)}
              <Input
                {...control}
                type="url"
                placeholder="https://"
                spellcheck="false"
                autocomplete="off"
                value={link}
                onchange={(text: string) => {
                  link = text;
                  linkError = null;
                }}
                {@attach focusNow}
              />
            {/snippet}
          </Field>
          <div class="flex flex-wrap justify-end gap-2">
            <Button variant="ghost" onclick={stopAsking}>Cancel</Button>
            <Button variant="primary" type="submit">Use this link</Button>
          </div>
        </form>
      {/if}

      {#if finished && !staysOpen && !hovered && !focused && !opening && asking === null}
        <p class="text-small text-ink-muted">This window closes by itself in a few seconds.</p>
      {/if}
    </div>

    {#snippet footer()}
      {#if finished}
        <Button variant="ghost" onclick={() => win.close()}>Close</Button>
        <Button icon={FolderOpen} onclick={() => void win.progress.openFolder()}
          >Show in folder</Button
        >
        <Button variant="primary" onclick={() => void open()}>Open</Button>
      {:else}
        <Button variant="ghost" onclick={() => win.close()}>Hide</Button>
        <Button aria-expanded={asking === 'cancel'} onclick={() => ask('cancel')}>Cancel</Button>
        {#if main}
          <Button variant="primary" onclick={() => void main.act()}>{main.label}</Button>
        {/if}
      {/if}
    {/snippet}
  </WindowFrame>
</div>
