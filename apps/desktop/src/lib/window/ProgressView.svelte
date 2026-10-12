<script lang="ts">
  // One download's progress, in its own window (D23): name, status and why it waits, progress,
  // speed, time left, connections, any error with its fix, and Pause or Resume, Cancel and Hide.
  // Once finished: Open and Show in folder, and the window closes by itself after a few seconds
  // unless the pointer is over it or it has the keyboard focus.
  import type { DownloadSummary, FixAction } from '@wirecatch/protocol';
  import Button from '$lib/components/Button.svelte';
  import ErrorFix from '$lib/components/ErrorFix.svelte';
  import CategoryIcon from '$lib/components/CategoryIcon.svelte';
  import {
    extractUrls,
    folderOf,
    formatEta,
    formatPercent,
    formatSize,
    hostOf,
    MEDIA_SOURCE_LABEL,
    PROCESSING_LABEL,
    processingFraction,
    progressbarValue,
    progressFraction,
    statusDetail,
    statusShort,
  } from '$lib/format';
  import { formatBytes, formatSpeed } from '@wirecatch/units';
  import { CLOSE_AFTER_MS, type DownloadWindowState } from './download-window.svelte';
  import WindowFrame from './WindowFrame.svelte';

  let { win, row }: { win: DownloadWindowState; row: DownloadSummary } = $props();
  const uid = $props.id();

  /** An inline question shown over the buttons: cancel, open a program, or a fresh link. */
  let asking = $state<'cancel' | 'program' | 'link' | null>(null);
  let link = $state('');
  let linkError = $state('');

  const running = $derived(
    row.status === 'downloading' ||
      row.status === 'connecting' ||
      row.status === 'queued' ||
      row.status === 'processing',
  );
  const finished = $derived(row.status === 'completed');
  // While ffmpeg converts or joins (D24), the bar shows that step when its progress is known.
  const processed = $derived(processingFraction(row));
  const fraction = $derived(processed ?? progressFraction(row.done, row.total));
  const percent = $derived(finished ? '' : formatPercent(row.done, row.total));
  const detail = $derived(statusDetail(row, win.queueName(row.queue)));
  /** "1.2 GB of 4.7 GB, 25%", or the size once finished, or what is known so far. */
  const doneText = $derived.by(() => {
    if (finished || row.total === null) return formatSize(row.done, row.total, row.status);
    const of = `${formatBytes(row.done)} of ${formatBytes(row.total)}`;
    return percent ? `${of}, ${percent}` : of;
  });
  const indeterminate = $derived(
    (row.status === 'processing' && processed === null) ||
      (fraction === null && (row.status === 'downloading' || row.status === 'connecting')),
  );

  // Closes by itself once finished, unless the person is looking at it or using it.
  let hovered = $state(false);
  let focused = $state(false);
  let root = $state<HTMLDivElement>();
  function updateFocus() {
    focused = document.hasFocus() && !!root && root.contains(document.activeElement);
  }
  $effect(() => {
    if (!finished || hovered || focused || asking !== null) return;
    const timer = setTimeout(() => win.close(), CLOSE_AFTER_MS);
    return () => clearTimeout(timer);
  });

  async function open() {
    const program = await win.isProgram();
    if (program === undefined) return;
    if (program) asking = 'program';
    else await win.openFile();
  }

  async function saveLink() {
    const found = extractUrls(link)[0];
    if (!found) {
      linkError = 'Paste the full new link, starting with https://.';
      return;
    }
    if (await win.refreshLink(found)) {
      asking = null;
      link = '';
    }
  }

  /** An inline question takes the focus when it appears. */
  function focusNow(el: HTMLElement) {
    el.focus();
  }

  function onkeydown(e: KeyboardEvent) {
    // Escape closes an inline question first, before it closes the window.
    if (e.key === 'Escape' && asking !== null) {
      e.preventDefault();
      e.stopPropagation();
      asking = null;
    }
  }
</script>

<svelte:window onfocus={updateFocus} onblur={updateFocus} />

<!-- The window's root watches the pointer and the focus inside it, so a finished download's
     window does not close under the person, and takes Escape from its inline questions first;
     the controls inside are the interactive elements. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="progress-window"
  bind:this={root}
  onmouseenter={() => (hovered = true)}
  onmouseleave={() => (hovered = false)}
  onfocusin={updateFocus}
  onfocusout={() => setTimeout(updateFocus)}
  {onkeydown}
>
  <WindowFrame title={row.file_name}>
    <div class="content">
      <div class="status-line">
        <span class="icon"><CategoryIcon category={row.category} size="lg" /></span>
        <p class="status {row.status}" role="status">{statusShort(row)}</p>
      </div>
      {#if detail}<p class="detail">{detail}</p>{/if}
      {#if row.media}
        <p class="detail">{row.media.quality}, {MEDIA_SOURCE_LABEL[row.media.source]}.</p>
      {/if}

      {#if row.status === 'failed' && row.error}
        <ErrorFix
          {row}
          onfix={(fix: FixAction) => win.fix(fix, () => (asking = 'link'))}
          onretry={() => win.retry()}
        />
      {/if}

      <section class="numbers" aria-label="Progress">
        <div
          class="wire {row.status}"
          role="progressbar"
          aria-label={row.status === 'processing' && row.processing
            ? PROCESSING_LABEL[row.processing.step]
            : 'Downloaded'}
          {...progressbarValue(fraction, finished)}
        >
          {#if indeterminate}
            <span class="fill unknown"></span>
          {:else}
            <span class="fill" style:transform="scaleX({finished ? 1 : (fraction ?? 0)})"></span>
          {/if}
        </div>
        <dl class="facts num">
          <div>
            <dt>Done</dt>
            <dd>{doneText}</dd>
          </div>
          {#if !finished}
            <div>
              <dt>Speed</dt>
              <dd>{formatSpeed(row.speed) || 'None right now'}</dd>
            </div>
            <div>
              <dt>Time left</dt>
              <dd>
                {row.eta_secs !== null && row.speed > 0 ? formatEta(row.eta_secs) : 'Not known yet'}
              </dd>
            </div>
            <div>
              <dt>Connections</dt>
              <dd>{row.connections > 0 ? `${row.connections} in use` : 'None open'}</dd>
            </div>
          {/if}
          <div class="wide">
            <dt>Saved to</dt>
            <dd class="path">{folderOf(row.path)}</dd>
          </div>
        </dl>
      </section>

      {#if win.error}<p class="error" role="alert">{win.error}</p>{/if}

      {#if asking === 'cancel'}
        <div class="ask" role="group" aria-labelledby="{uid}-cancel">
          <p id="{uid}-cancel">
            Cancel this download? It stops, the part already downloaded is deleted, and it leaves
            the list.
          </p>
          <div class="ask-actions">
            <Button {@attach focusNow} onclick={() => (asking = null)}>Keep downloading</Button>
            <Button variant="danger" onclick={() => win.cancelDownload()}
              >Cancel and delete the partial file</Button
            >
          </div>
        </div>
      {:else if asking === 'program'}
        <div class="ask" role="group" aria-labelledby="{uid}-program">
          <p id="{uid}-program">
            {row.file_name} can run code on this computer. Only open it if you trust {hostOf(
              row.url,
            )}.
          </p>
          <div class="ask-actions">
            <Button {@attach focusNow} onclick={() => (asking = null)}>Do not open</Button>
            <Button
              variant="danger"
              onclick={async () => {
                asking = null;
                await win.openFile(true);
              }}>Open it</Button
            >
          </div>
        </div>
      {:else if asking === 'link'}
        <form
          class="ask"
          onsubmit={(e) => {
            e.preventDefault();
            void saveLink();
          }}
        >
          <label class="form-stack">
            <span class="form-label">Fresh link</span>
            <span class="form-help hint"
              >Start the download again on the page it came from, copy the new link and paste it
              here. Everything already downloaded is kept.</span
            >
            <input
              class="field"
              type="url"
              placeholder="https://"
              aria-invalid={linkError ? 'true' : undefined}
              {@attach focusNow}
              bind:value={link}
              oninput={() => (linkError = '')}
            />
          </label>
          {#if linkError}<p class="error" role="alert">{linkError}</p>{/if}
          <div class="ask-actions">
            <Button onclick={() => (asking = null)}>Cancel</Button>
            <Button variant="primary" type="submit">Use this link</Button>
          </div>
        </form>
      {/if}

      {#if finished && !hovered && !focused && asking === null}
        <p class="closing">This window closes by itself in a few seconds.</p>
      {/if}
    </div>

    {#snippet footer()}
      {#if finished}
        <Button onclick={() => win.close()}>Close</Button>
        <Button onclick={() => win.openFolder()}>Show in folder</Button>
        <Button variant="primary" onclick={open}>Open</Button>
      {:else}
        <Button onclick={() => win.close()}>Hide</Button>
        <Button disabled={asking === 'cancel'} onclick={() => (asking = 'cancel')}>Cancel</Button>
        {#if running}
          <Button variant="primary" onclick={() => win.pause()}>Pause</Button>
        {:else if row.status === 'failed' && !row.error}
          <Button variant="primary" onclick={() => win.retry()}>Try again</Button>
        {:else if row.status !== 'failed'}
          <Button variant="primary" onclick={() => win.resume()}
            >{row.status === 'cancelled' ? 'Start over' : 'Resume'}</Button
          >
        {/if}
      {/if}
    {/snippet}
  </WindowFrame>
</div>

<style>
  .progress-window {
    height: 100%;
  }

  .content {
    display: grid;
    gap: 12px;
  }

  .status-line {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--ink-2);
  }

  .status {
    margin: 0;
    font-size: var(--text-sm);
    font-weight: 600;
  }

  .status.downloading,
  .status.connecting {
    color: var(--accent-text);
  }

  .status.paused {
    color: var(--warn);
  }

  .status.failed {
    color: var(--danger);
  }

  .detail {
    margin: -4px 0 0;
    padding: 8px 10px;
    border-left: 3px solid var(--wire-done);
    border-radius: var(--radius-s);
    background: var(--surface-2);
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.45;
  }

  .wire {
    position: relative;
    height: 6px;
    overflow: hidden;
    border-radius: 3px;
    background: var(--wire-track);
  }

  .fill {
    position: absolute;
    inset: 0;
    background: var(--accent);
    transform-origin: left center;
    transition: transform 240ms linear;
  }

  .fill.unknown {
    width: 30%;
    animation: slide 1.4s var(--ease) infinite;
  }

  @keyframes slide {
    from {
      transform: translateX(-100%);
    }
    to {
      transform: translateX(340%);
    }
  }

  .wire.paused .fill {
    background: var(--warn-wire);
  }

  .wire.failed .fill {
    background: var(--danger);
  }

  .wire.queued .fill {
    background: var(--wire-done);
  }

  .facts {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 10px 16px;
    margin: 12px 0 0;
  }

  .facts .wide {
    grid-column: 1 / -1;
  }

  dt {
    color: var(--ink-2);
    font-size: var(--text-xs);
    font-weight: 600;
  }

  dd {
    margin: 2px 0 0;
    font-size: var(--text-sm);
  }

  .path {
    overflow-wrap: anywhere;
  }

  .ask {
    display: grid;
    gap: 10px;
    padding: 12px;
    border: 1px solid var(--line-strong);
    border-radius: var(--radius-m);
    background: var(--surface-2);
  }

  .ask p {
    margin: 0;
    line-height: 1.5;
  }

  .ask-actions {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
  }

  .hint {
    line-height: 1.45;
  }

  .error {
    margin: 0;
    color: var(--danger);
    font-size: var(--text-sm);
  }

  .closing {
    margin: 0;
    color: var(--ink-2);
    font-size: var(--text-xs);
  }
</style>
