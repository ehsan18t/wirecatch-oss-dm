/** What the trigger must carry: spread it onto the button that opens the popover. */
export interface PopoverTriggerProps {
  'aria-haspopup': 'dialog';
  'aria-expanded': boolean;
  'aria-controls': string;
  onclick: () => void;
  [key: symbol]: (node: HTMLElement) => void;
}
