// A button whose label (its `.truncate` text) is cut short, by a long translation or a narrow window, offers the label in full in a tooltip.
import { textIfCut, tooltip } from '@wirecatch/ui';
import type { Attachment } from 'svelte/attachments';

/** The tooltip of a button with a cut label; quiet while `reason` says why the button is off, as the button's own tooltip does then. */
export function wholeLabel(reason: () => string | null): Attachment<HTMLElement> {
  return tooltip(() => (reason() === null ? labelIfCut : null));
}

function labelIfCut(button: HTMLElement): string | null {
  const label = button.querySelector('.truncate');
  return label === null ? null : textIfCut(label.textContent, () => label)(button);
}
