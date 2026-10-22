// A video's quality window with output formats (D24): Save as, audio quality, subtitles and
// chapters, the notes that say whether a choice converts, and the last choices as the start.

import { questionId } from '@wirecatch/protocol';
import { render, screen, within } from '@testing-library/svelte';
import userEvent, { type UserEvent } from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import type { MockBackend } from '$lib/backend/mock';
import { mockBackend, settle, windowState } from '$lib/testing/setup';
import DownloadWindow from './DownloadWindow.svelte';

/** The sample backend with the media add-on installed and one video page asking for a quality. */
async function asking(change?: (b: MockBackend) => Promise<void>) {
  const b = mockBackend({ empty: true });
  await b.installAddon('media');
  b.tick(4500);
  await change?.(b);
  b.simulateMediaChoice();
  const [choice] = await b.pendingMediaChoices();
  const { win, host } = windowState(b, { question: choice?.id ?? questionId(0) });
  await win.init();
  render(DownloadWindow, { props: { win } });
  await settle();
  return { b, win, host, user: userEvent.setup() };
}

const saveAs = () => screen.getByRole('combobox', { name: 'Save as' });
const audioQuality = () => screen.getByRole('combobox', { name: 'Audio quality' });

const quality = () => screen.getByRole('combobox', { name: 'Quality' });
const subtitles = () => screen.getByRole('button', { name: 'Subtitles' });

/** The subtitle languages offered, each with whether it is picked, read from the list and closed again. */
async function subtitleStates(user: UserEvent): Promise<[string, string | null][]> {
  await user.click(subtitles());
  const states = screen
    .getAllByRole('option')
    .map((o): [string, string | null] => [o.textContent.trim(), o.getAttribute('aria-selected')]);
  await user.keyboard('{Escape}');
  return states;
}

/** Ticks or unticks the subtitle languages named, in the list, and closes it. */
async function tickSubtitles(user: UserEvent, ...names: string[]) {
  await user.click(subtitles());
  for (const name of names) await user.click(screen.getByRole('option', { name }));
  await user.keyboard('{Escape}');
  await settle();
}

/** The options a menu offers, read from its list and closed again. */
async function optionTexts(user: UserEvent, menu: HTMLElement): Promise<string[]> {
  await user.click(menu);
  const texts = within(screen.getByRole('listbox'))
    .getAllByRole('option')
    .map((o) => o.textContent.trim());
  await user.keyboard('{Escape}');
  return texts;
}

/** Picks the option named `name` from a menu, as the pointer does. */
async function choose(user: UserEvent, menu: HTMLElement, name: string | RegExp) {
  await user.click(menu);
  await user.click(within(screen.getByRole('listbox')).getByRole('option', { name }));
  await settle();
}

describe('the quality window with output formats', () => {
  it('starts as Original and says per quality what another format means', async () => {
    const { b, host, user } = await asking();
    expect(saveAs()).toHaveTextContent('Original (WebM)');
    expect(await optionTexts(user, saveAs())).toEqual(['Original (WebM)', 'MP4', 'MKV', 'WebM']);
    // Original converts nothing, so no row says anything about it.
    expect(screen.queryByText('Converts')).toBeNull();
    expect(screen.queryByText('No conversion')).toBeNull();

    await choose(user, saveAs(), 'MP4');
    // 2160p comes only as VP9: MP4 converts it, and the window says so before it starts, on the quality itself too.
    expect(quality()).toHaveTextContent(/2160p, MP4/);
    expect(quality()).toHaveTextContent(/Converts/);
    expect(
      screen.getByText(/no MP4 version, so Wirecatch converts the video to H\.264/),
    ).toBeVisible();
    // A quality that copies says nothing about it: only a conversion is worth a word.
    const qualities = await optionTexts(user, quality());
    expect(qualities.find((q) => q.startsWith('1080p, MP4'))).not.toMatch(/Converts/);

    await choose(user, quality(), /^1080p, MP4/);
    expect(saveAs()).toHaveTextContent('MP4');
    expect(
      screen.getByText(/^No conversion: the video and audio are copied into MP4 as they are/),
    ).toBeVisible();

    // Subtitles: one field that opens a list to search and tick; human-written ones first, auto-generated ones marked, none picked at first.
    expect(subtitles()).toHaveTextContent('None');
    const offered = await subtitleStates(user);
    expect(offered.slice(0, 4)).toEqual([
      ['English', 'false'],
      ['Spanish', 'false'],
      // Two auto-generated tracks in one language are told apart by their codes.
      ['English (auto, en)', 'false'],
      ['English (auto, en-orig)', 'false'],
    ]);
    await tickSubtitles(user, 'English');
    expect(subtitles()).toHaveTextContent('English');
    expect(
      screen.getByRole('checkbox', { name: 'Save subtitles as .srt files next to the video' }),
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
    await choose(user, quality(), /^Audio only/);
    expect(await optionTexts(user, saveAs())).toEqual([
      'Original (M4A)',
      'MP3',
      'M4A',
      'Opus',
      'FLAC',
      'WAV',
    ]);
    expect(screen.queryByRole('combobox', { name: 'Audio quality' })).toBeNull();

    await choose(user, saveAs(), 'MP3');
    expect(audioQuality()).toHaveTextContent('192 kbps');
    expect(await optionTexts(user, audioQuality())).toEqual([
      '128 kbps',
      '192 kbps',
      '256 kbps',
      '320 kbps',
    ]);
    await choose(user, audioQuality(), '320 kbps');
    expect(screen.getByText(/converted to MP3 at 320 kbps/)).toBeVisible();
    expect(screen.getByText(/320 kbps is more than the 160 kbps of the download/)).toBeVisible();

    // M4A takes the AAC as it is unless a quality is picked.
    await choose(user, saveAs(), 'M4A');
    expect(audioQuality()).toHaveTextContent('256 kbps');
    await choose(user, audioQuality(), /^As downloaded/);
    expect(screen.getByText('No conversion: the audio is copied into M4A as it is.')).toBeVisible();

    await choose(user, saveAs(), 'FLAC');
    expect(
      screen.getByText(/The download's audio is Opus, which already left out some detail/),
    ).toBeVisible();
    expect(screen.queryByRole('combobox', { name: 'Audio quality' })).toBeNull();

    // Subtitles go next to an audio file.
    await tickSubtitles(user, 'Spanish');
    expect(screen.getByText('Subtitles are saved as .srt files next to the file.')).toBeVisible();
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
      await b.changeSettings({ media: settings.media });
    });
    expect(saveAs()).toHaveTextContent('MKV');
    expect(subtitles()).toHaveTextContent('English');
    const offered = await subtitleStates(user);
    // The picked one lists first, and the auto-generated English is not picked with it.
    expect(offered[0]).toEqual(['English', 'true']);
    expect(offered.find(([name]) => name === 'English (auto, en-orig)')?.[1]).toBe('false');
    expect(
      screen.getByRole('checkbox', { name: 'Save subtitles as .srt files next to the video' }),
    ).toBeChecked();
    expect(screen.getByRole('checkbox', { name: "Add the video's chapters" })).toBeChecked();

    await choose(user, quality(), /^Audio only/);
    expect(saveAs()).toHaveTextContent('Opus');
    expect(audioQuality()).toHaveTextContent('160 kbps');
  });
});

describe('the quality window, short', () => {
  it('shows the quality picked with its badges and size on one field, and folds where the file goes', async () => {
    await asking();
    // The quality picked, with what sets it apart, as each one in its list shows it.
    expect(quality()).toHaveTextContent(/^2160p, WebM\s*Best\s*1\.80 GB$/);
    const where = screen.getByRole('button', { name: /^Save to/ });
    expect(where).toHaveAttribute('aria-expanded', 'false');
    expect(where).toHaveTextContent(/Video · Main$/);
  });
});

describe('the quality window by keyboard', () => {
  it('picks a quality with the arrow keys, and downloads it on Enter in a field', async () => {
    const { b, user } = await asking();
    const choose = vi.spyOn(b, 'chooseMedia');
    quality().focus();
    // Down opens the list on the quality picked; Down again moves to the next, Enter picks it.
    await user.keyboard('{ArrowDown}{ArrowDown}{Enter}');
    await settle();
    expect(quality()).toHaveTextContent(/^1080p/);
    expect(choose).not.toHaveBeenCalled();
    // Enter on the quality opens its list rather than downloading; Enter in a text field downloads.
    screen.getByRole('textbox', { name: 'File name' }).focus();
    await user.keyboard('{Enter}');
    await settle();
    expect(choose).toHaveBeenCalledTimes(1);
    const picked = (await b.pendingMediaChoices()).length;
    expect(picked).toBe(0);
    expect(choose.mock.calls[0]?.[1].option_id).toMatch(/1080/);
  });
});
