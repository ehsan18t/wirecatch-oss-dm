// The popup's option lists as the app builds them, for the extension's tests and preview: written by `wirecatch-core`'s option builder (`options.sample.json`), whose test fails when they drift from it.
import SAMPLES from '../options.sample.json' with { type: 'json' };
import { isOptionList } from '../generated/validators';
import type { OptionList } from '../generated/OptionList';

function checked(value: unknown, name: string): OptionList {
  if (!isOptionList(value)) throw new Error(`The ${name} sample is not an option list.`);
  return value;
}

/** The options of a fresh install. */
export const DEFAULT_OPTIONS: OptionList = checked(SAMPLES.default, 'default');
/** Long names and every dependent row shown: a speed limit, a resolution, subtitles in the video. */
export const LONG_OPTIONS: OptionList = checked(SAMPLES.long, 'long');
