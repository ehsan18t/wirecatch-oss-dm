// Handling the core's events as a table: one handler per event type. A table that leaves a
// type out does not compile, so a new event is never dropped by a `switch` without a case.

import type { UiEvent } from '@wirecatch/protocol';

export type UiEventType = UiEvent['type'];

/** The core's event of one type. */
export type UiEventOf<K extends UiEventType> = Extract<UiEvent, { type: K }>;

/** What to do with each type of event. */
export type UiEventHandlers = { [K in UiEventType]: (event: UiEventOf<K>) => void };

/** Runs the handler for an event's type. */
export function dispatch<K extends UiEventType>(
  handlers: UiEventHandlers,
  event: UiEventOf<K> & { type: K },
): void {
  const handler: (event: UiEventOf<K>) => void = handlers[event.type];
  handler(event);
}

/** For an event a handler table has nothing to do with. */
export function ignore(): void {
  // Nothing to keep from this event.
}

/**
 * Events that arrive while a window loads. They are held until it has loaded and then handled in
 * order, so none is lost and none is applied to lists that are not there yet.
 */
export class HeldEvents {
  #held: UiEvent[] | null = [];
  readonly #handle: (event: UiEvent) => void;

  constructor(handle: (event: UiEvent) => void) {
    this.#handle = handle;
  }

  /** Takes one event: held while loading, handled at once after. */
  readonly receive = (event: UiEvent): void => {
    if (this.#held) this.#held.push(event);
    else this.#handle(event);
  };

  /** Handles the held events, and from now on each event as it comes. */
  release(): void {
    const held = this.#held ?? [];
    this.#held = null;
    for (const event of held) this.#handle(event);
  }

  /** Lets the held events go unhandled, and handles each event from now on as it comes. */
  drop(): void {
    this.#held = null;
  }
}
