// The popup's option lists and the values the extension's checks use, as the app builds them, for the extension's tests and preview: written by `wirecatch-core`'s option builder (`options.sample.json`), whose test fails when they drift from it.
import SAMPLES from '../options.sample.json' with { type: 'json' };
import { isOptionList, isShellConfig } from '../generated/validators';
import type { OptionList } from '../generated/OptionList';
import type { ShellConfig } from '../generated/ShellConfig';

function checked(value: unknown, name: string): OptionList {
  if (!isOptionList(value)) throw new Error(`The ${name} sample is not an option list.`);
  return value;
}

/** The values the extension's checks use, as a fresh install has them. */
export const DEFAULT_SHELL_CONFIG: ShellConfig = (() => {
  if (!isShellConfig(SAMPLES.shell_config))
    throw new Error('The shell config sample does not read.');
  return SAMPLES.shell_config;
})();

/** The options of a fresh install. */
export const DEFAULT_OPTIONS: OptionList = checked(SAMPLES.default, 'default');
/** Long names and every dependent row shown: a speed limit, a resolution, subtitles in the video. */
export const LONG_OPTIONS: OptionList = checked(SAMPLES.long, 'long');
