<script lang="ts">
  // The layout of a download window: a title, a body that scrolls, and buttons at the bottom
  // that always stay in view. The first `[data-autofocus]` element takes the focus when a view
  // appears, so Enter does what the main button says. The frame reports the height its content
  // needs, so the window is only as tall as what it shows (see `fit.ts`).
  import type { Snippet } from 'svelte';
  import { onMount } from 'svelte';
  import { fitReporter, getFitWindow, neededHeight } from './fit';

  let {
    title,
    description,
    children,
    footer,
  }: {
    title: string;
    description?: string;
    children: Snippet;
    footer?: Snippet;
  } = $props();

  const uid = $props.id();
  let el = $state<HTMLElement>();
  let head = $state<HTMLElement>();
  let body = $state<HTMLElement>();
  let content = $state<HTMLElement>();
  let foot = $state<HTMLElement>();
  const fit = getFitWindow();

  // Measures again whenever the title, the content or the buttons change size (a view that
  // switches from asking to progress, an error that appears, a longer file name).
  $effect(() => {
    if (fit === null || !head || !body || !content) return;
    const report = fitReporter(fit);
    const measure = () => {
      if (!head || !body || !content) return;
      const style = getComputedStyle(body);
      report(
        neededHeight({
          head: head.getBoundingClientRect().height,
          content: content.getBoundingClientRect().height,
          padding: parseFloat(style.paddingTop) + parseFloat(style.paddingBottom),
          foot: foot?.getBoundingClientRect().height ?? 0,
        }),
      );
    };
    const observer = new ResizeObserver(measure);
    for (const part of [head, content, foot]) if (part) observer.observe(part);
    measure();
    return () => observer.disconnect();
  });

  onMount(() => {
    const target =
      el?.querySelector<HTMLElement>('[data-autofocus]') ??
      el?.querySelector<HTMLElement>('input, select, button');
    target?.focus();
  });
</script>

<div
  role="group"
  class="frame"
  bind:this={el}
  aria-labelledby="{uid}-title"
  aria-describedby={description ? `${uid}-desc` : undefined}
>
  <div class="head" bind:this={head}>
    <h1 id="{uid}-title">{title}</h1>
    {#if description}
      <p id="{uid}-desc" class="description">{description}</p>
    {/if}
  </div>
  <div class="body" bind:this={body}>
    <div bind:this={content}>
      {@render children()}
    </div>
  </div>
  {#if footer}
    <div class="foot" bind:this={foot}>{@render footer()}</div>
  {/if}
</div>

<style>
  .frame {
    display: flex;
    flex-direction: column;
    height: 100%;
    min-height: 0;
    background: var(--surface);
    color: var(--ink);
  }

  .head {
    padding: 16px 20px 0;
  }

  h1 {
    margin: 0;
    font-size: var(--text-lg);
    font-weight: 620;
    line-height: 1.3;
    overflow-wrap: anywhere;
  }

  .description {
    margin: 4px 0 0;
    color: var(--ink-2);
    font-size: var(--text-sm);
    line-height: 1.5;
    overflow-wrap: anywhere;
  }

  .body {
    flex: 1;
    min-height: 0;
    padding: 14px 20px 8px;
    overflow-y: auto;
  }

  .foot {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: 8px;
    padding: 12px 20px 16px;
    border-top: 1px solid var(--line);
  }
</style>
