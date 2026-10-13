<script lang="ts">
  import { normalizeSite } from '@wirecatch/protocol';

  interface Props {
    sites: readonly string[];
    disabled?: boolean;
    onadd: (site: string) => void;
    onremove: (site: string) => void;
  }

  let { sites, disabled = false, onadd, onremove }: Props = $props();

  let draft = $state('');
  let error = $state<string | null>(null);

  function add(event: SubmitEvent): void {
    event.preventDefault();
    const site = normalizeSite(draft);
    if (site === null) {
      error = 'Type a site such as example.com.';
      return;
    }
    error = null;
    draft = '';
    if (!sites.includes(site)) onadd(site);
  }

  function remove(site: string): void {
    onremove(site);
  }
</script>

<section class="sites" aria-labelledby="sites-title">
  <h2 id="sites-title" class="field-label">Sites to ignore</h2>
  <p id="sites-help" class="help">
    Downloads from these sites (and their subdomains) stay in the browser.
  </p>

  {#if sites.length === 0}
    <p class="empty">No sites ignored.</p>
  {:else}
    <ul class="list" aria-labelledby="sites-title">
      {#each sites as site (site)}
        <li>
          <span class="site">{site}</span>
          <button
            type="button"
            class="link-button"
            {disabled}
            aria-label="Stop ignoring {site}"
            onclick={() => remove(site)}
          >
            Remove
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <form class="add" onsubmit={add}>
    <label for="site-input" class="visually-hidden">Site to ignore</label>
    <input
      id="site-input"
      type="text"
      inputmode="url"
      autocomplete="off"
      spellcheck="false"
      placeholder="example.com"
      bind:value={draft}
      {disabled}
      aria-describedby={error === null ? 'sites-help' : 'site-error'}
      aria-invalid={error !== null}
    />
    <button type="submit" class="button" {disabled}>Add</button>
  </form>
  {#if error !== null}
    <p id="site-error" class="error-text" role="alert">{error}</p>
  {/if}
</section>

<style>
  .sites {
    padding: 8px 0;
  }

  h2 {
    margin: 0;
    font-size: inherit;
  }

  .empty {
    margin: 6px 0;
    color: var(--muted);
  }

  .list {
    list-style: none;
    margin: 6px 0;
    padding: 0;
    max-height: 112px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }

  .list li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 4px 4px 4px 10px;
  }

  .list li + li {
    border-top: 1px solid var(--border);
  }

  .site {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .add {
    display: flex;
    gap: 8px;
  }

  .add input {
    flex: 1;
    min-width: 0;
  }
</style>
