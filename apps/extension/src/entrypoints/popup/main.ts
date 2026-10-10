import { mount } from 'svelte';
import { browser } from 'wxt/browser';
import App from '@/components/App.svelte';
import { POPUP_PORT_NAME, type PopupRequest } from '@/lib/messages';
import { PopupModel } from '@/lib/popup/model.svelte';
import './style.css';

/** Longest the popup waits for the background before showing itself anyway. */
const FIRST_SNAPSHOT_WAIT_MS = 300;

async function activeTabUrl(): Promise<string | null> {
  try {
    const [tab] = await browser.tabs.query({ active: true, currentWindow: true });
    return tab?.url ?? null;
  } catch {
    return null;
  }
}

function firstSnapshot(port: {
  onMessage: { addListener(listener: (message: unknown) => void): void };
}): Promise<void> {
  return new Promise((resolve) => {
    const timer = setTimeout(resolve, FIRST_SNAPSHOT_WAIT_MS);
    port.onMessage.addListener(() => {
      clearTimeout(timer);
      resolve();
    });
  });
}

async function start(): Promise<void> {
  const port = browser.runtime.connect({ name: POPUP_PORT_NAME });
  const channel = {
    postMessage: (message: PopupRequest) => port.postMessage(message),
    onMessage: port.onMessage,
    onDisconnect: port.onDisconnect,
  };
  const model = new PopupModel(channel);
  // Mount once the first state is in, so the popup opens at its final size.
  const [currentUrl] = await Promise.all([activeTabUrl(), firstSnapshot(port)]);
  const target = document.getElementById('app');
  if (target === null) return;
  mount(App, { target, props: { model, currentUrl } });
}

void start();
