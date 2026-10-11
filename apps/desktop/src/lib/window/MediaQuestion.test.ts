// A video's quality window with output formats (D24): Save as, audio quality, subtitles and
// chapters, the notes that say whether a choice converts, and the last choices as the start.

import { render, screen, within } from '@testing-library/svelte';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MockBackend } from '$lib/backend/mock';
import { settle } from '$lib/testing/setup';
import DownloadWindow from './DownloadWindow.svelte';
import { DownloadWindowState } from './download-window.svelte';

const NOON = new Date(2026, 9, 10, 12, 0).getTime();

let cleanup: (() => void) | undefined;
afterEach(() => {
  cleanup?.();
  cleanup = undefined;
});

/** The sample backend with the media add-on installed and one video page asking for a quality. */
async function asking(change?: (b: MockBackend) => Promise<void>) {
  const b = new MockBackend({ autoTick: false, now: () => NOON, empty: true });
  await b.installAddon('media');
  b.tick(4500);
  await change?.(b);
  b.simulateMediaChoice();
  const [choice] = await b.pendingMediaChoices();
  const host = { close: vi.fn(), becameDownload: vi.fn() };
  const win = new DownloadWindowState(b, { question: choice?.id ?? -1 }, host);
  await win.init();
  const view = render(DownloadWindow, { props: { win } });
  cleanup = () => {
    win.dispose();
    view.unmount();
  };
  await settle();
  return { b, win, host, user: userEvent.setup() };
}

const saveAs = () => screen.getByRole('combobox', { name: 'Save as' });

/** The row a quality's radio button sits in. */
function rowOf(radio: HTMLElement): HTMLElement {
  const row = radio.closest('label');
  if (!row) throw new Error('the quality has no row');
  return row;
}
const optionTexts = (select: HTMLElement) =>
  within(select)
    .getAllByRole('option')
    .map((o) => o.textContent?.trim());

describe('the quality window with output formats', () => {
  it('starts as Original and says per quality what another format means', async () => {
    const { b, host, user } = await asking();
    expect(saveAs()).toHaveValue('original');
    expect(optionTexts(saveAs())).toEqual(['Original (WebM)', 'MP4', 'MKV', 'WebM']);
    // Original converts nothing, so no row says anything about it.
    expect(screen.queryByText('Converts')).toBeNull();
    expect(screen.queryByText('No conversion')).toBeNull();

    await user.selectOptions(saveAs(), 'mp4');
    await settle();
    // 2160p comes only as VP9: MP4 converts it, and the window says so before it starts.
    const uhd = screen.getByRole('radio', { name: /2160p, MP4/ });
    expect(uhd).toBeChecked();
    expect(within(rowOf(uhd)).getByText('Converts')).toBeInTheDocument();
    expect(
      screen.getByText(/no MP4 version, so Wirecatch converts the video to H\.264/),
    ).toBeVisible();
    const fullHd = screen.getByRole('radio', { name: /1080p, MP4/ });
    expect(within(rowOf(fullHd)).getByText('No conversion')).toBeInTheDocument();

    await user.click(fullHd);
    await settle();
    expect(saveAs()).toHaveValue('mp4');
    expect(
      screen.getByText(/^No conversion: the video and audio are copied into MP4 as they are/),
    ).toBeVisible();

    // Subtitles: human-written ones first, auto-generated ones marked, none ticked at first.
    const english = screen.getByRole('checkbox', { name: 'English' });
    const auto = screen.getByRole('checkbox', { name: 'English, auto-generated' });
    expect(screen.getByRole('checkbox', { name: 'Spanish' })).not.toBeChecked();
    expect(english).not.toBeChecked();
    expect(auto).not.toBeChecked();
    await user.click(english);
    await settle();
    expect(
      screen.getByRole('checkbox', { name: 'Save them as .srt files next to the video instead' }),
    ).not.toBeChecked();
    const chapters = screen.getByRole('checkbox', { name: "Add the video's chapters" });
    await user.click(chapters);
    await settle();

    await user.click(screen.getByRole('button', { name: 'Download now' }));
    await settle();
    expect(host.becameDownload).toHaveBeenCalledTimes(1);
    const [row] = await b.listDownloads();
    expect(row?.file_name.endsWith('.mp4')).toBe(true);
    // The next video window starts from these choices.
    const remembered = (await b.getSettings()).media.video;
    expect(remembered.format).toBe('mp4');
    expect(remembered.subtitle_languages).toEqual(['en']);
    expect(remembered.chapters).toBe(true);
    expect(remembered.auto_subtitles).toBe(false);
  });

  it('offers audio formats, an audio quality and notes for audio only', async () => {
    const { user } = await asking();
    await user.click(screen.getByRole('radio', { name: /Audio only/ }));
    await settle();
    expect(optionTexts(saveAs())).toEqual(['Original (M4A)', 'MP3', 'M4A', 'Opus', 'FLAC', 'WAV']);
    expect(screen.queryByRole('combobox', { name: 'Audio quality' })).toBeNull();

    await user.selectOptions(saveAs(), 'mp3');
    await settle();
    const quality = screen.getByRole('combobox', { name: 'Audio quality' });
    expect(quality).toHaveValue('192');
    expect(optionTexts(quality)).toEqual(['128 kbps', '192 kbps', '256 kbps', '320 kbps']);
    await user.selectOptions(quality, '320');
    await settle();
    expect(screen.getByText(/converted to MP3 at 320 kbps/)).toBeVisible();
    expect(screen.getByText(/320 kbps is more than the 160 kbps of the download/)).toBeVisible();

    // M4A takes the AAC as it is unless a quality is picked.
    await user.selectOptions(saveAs(), 'm4a');
    await settle();
    expect(screen.getByRole('combobox', { name: 'Audio quality' })).toHaveValue('256');
    await user.selectOptions(screen.getByRole('combobox', { name: 'Audio quality' }), '');
    await settle();
    expect(screen.getByText('No conversion: the audio is copied into M4A as it is.')).toBeVisible();

    await user.selectOptions(saveAs(), 'flac');
    await settle();
    expect(
      screen.getByText(/The download's audio is Opus, which already left out some detail/),
    ).toBeVisible();
    expect(screen.queryByRole('combobox', { name: 'Audio quality' })).toBeNull();

    // Subtitles go next to an audio file.
    await user.click(screen.getByRole('checkbox', { name: 'Spanish' }));
    await settle();
    expect(screen.getByText('They are saved as .srt files next to the file.')).toBeVisible();
    expect(screen.queryByRole('checkbox', { name: "Add the video's chapters" })).toBeNull();
  });

  it('starts from the last choices, kept apart for videos and audio only', async () => {
    const { user } = await asking(async (b) => {
      const settings = await b.getSettings();
      settings.media.video = {
        format: 'mkv',
        audio_kbps: null,
        subtitle_languages: ['en'],
        auto_subtitles: false,
        subtitle_files: true,
        chapters: true,
      };
      settings.media.audio = {
        ...settings.media.video,
        format: 'opus',
        audio_kbps: 160,
        chapters: false,
      };
      await b.changeSettings([{ path: ['media'], value: settings.media }]);
    });
    expect(saveAs()).toHaveValue('mkv');
    expect(screen.getByRole('checkbox', { name: 'English' })).toBeChecked();
    expect(screen.getByRole('checkbox', { name: 'English, auto-generated' })).not.toBeChecked();
    expect(
      screen.getByRole('checkbox', { name: 'Save them as .srt files next to the video instead' }),
    ).toBeChecked();
    expect(screen.getByRole('checkbox', { name: "Add the video's chapters" })).toBeChecked();

    await user.click(screen.getByRole('radio', { name: /Audio only/ }));
    await settle();
    expect(saveAs()).toHaveValue('opus');
    expect(screen.getByRole('combobox', { name: 'Audio quality' })).toHaveValue('160');
  });
});
