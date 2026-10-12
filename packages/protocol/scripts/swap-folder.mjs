// Replaces a folder with a new one so that a reader sees the old folder or the new one, never a half-written one, and a failure leaves the old one in place.
//
// The new files are written to `<folder>.next` beside it, on the same disk, and swapped in with two renames: the old folder to `<folder>.previous`, then the new one into its place. When the second rename fails, the first is undone before the error is reported; a run killed between the two is undone by the next run.
import { existsSync, renameSync, rmSync } from 'node:fs';

/** Error codes Windows gives when another program (an editor, a virus scanner, a file watcher) has a folder open. */
const LOCKED = new Set(['EPERM', 'EBUSY', 'EACCES']);

export const nextFolder = (folder) => `${folder}.next`;
const previousFolder = (folder) => `${folder}.previous`;

/** `error` in plain words when the folder was locked by another program. */
function explained(error, folder) {
  if (!(error instanceof Error) || !LOCKED.has(error.code)) return error;
  return new Error(
    `${folder}, or the new files beside it, are open in another program (${error.code}). Close editors, file watchers or virus scans using them and run the command again; the folder is unchanged.`,
    { cause: error },
  );
}

/** After a run that stopped between its two renames, puts the old folder back where it belongs. */
export function recoverInterruptedSwap(folder) {
  if (!existsSync(folder) && existsSync(previousFolder(folder)))
    renameSync(previousFolder(folder), folder);
}

/** Moves the old folder back after a failed swap; when that fails too, says where the old files are. */
function undo(previous, folder, rename, cause) {
  try {
    rename(previous, folder);
  } catch (error) {
    throw new Error(
      `${folder} could not be replaced (${cause instanceof Error ? cause.message : String(cause)}), and moving the old files back failed too (${error instanceof Error ? error.message : String(error)}). They are in ${previous}; the next run of the command puts them back.`,
      { cause: error },
    );
  }
}

/** Puts `<folder>.next` in the place of `folder`. When the second rename fails, the old folder is moved back before the error is reported. `rename` is the file system's, replaced in tests. */
export function swapIn(folder, { rename = renameSync } = {}) {
  const previous = previousFolder(folder);
  rmSync(previous, { recursive: true, force: true });
  const hadFolder = existsSync(folder);
  try {
    if (hadFolder) rename(folder, previous);
    rename(nextFolder(folder), folder);
  } catch (error) {
    if (hadFolder && !existsSync(folder)) undo(previous, folder, rename, error);
    throw explained(error, folder);
  }
}

/** Removes the old folder once the new one is in place; a failure leaves only an ignored folder that the next run removes, so it is reported and not fatal. */
export function removePrevious(folder) {
  try {
    rmSync(previousFolder(folder), { recursive: true, force: true });
  } catch (error) {
    console.warn(
      `The old ${previousFolder(folder)} could not be removed (${error instanceof Error ? error.message : String(error)}); the next run removes it.`,
    );
  }
}
