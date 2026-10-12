<script lang="ts">
  import { onMount } from 'svelte';
  import { createBackend } from '$lib/backend';
  import Shell from '$lib/components/Shell.svelte';
  import { AppState, setApp } from '$lib/state/app.svelte';

  // The backend loads asynchronously so the mock never ships in a production build.
  const app = new AppState(createBackend);
  setApp(app);

  onMount(() => {
    // The shell opens the window hidden and shows it once its first view is drawn (after the
    // list loaded, two frames later), so it never flashes blank. Without a backend the shell
    // shows it after a moment anyway.
    const ready = () => void app.mainWindowReady();
    void app.init().finally(() => requestAnimationFrame(() => requestAnimationFrame(ready)));
    return () => app.dispose();
  });
</script>

<Shell />
