// How the sample backend starts: how many downloads, how fast it runs, and which notices it starts with.

export interface MockOptions {
  /** Total downloads. Beyond the hand-written samples, generated ones fill the list. */
  count?: number;
  /** How many generated downloads are running at once (for load testing the list). */
  active?: number;
  /** Start with no downloads at all, as on a first run. */
  empty?: boolean;
  /** Advance the simulation on a timer. Tests turn this off and call `tick` themselves. */
  autoTick?: boolean;
  tickMs?: number;
  /** Simulate one browser capture a few seconds after start. */
  autoCapture?: boolean;
  seed?: number;
  now?: () => number;
  /**
   * How long each answer takes to come back, in milliseconds, as a call to the real core does.
   * Zero (the default) answers as soon as the call returns, as the mock always has; more lets
   * answers arrive well after the events the call caused, which shows races in the interface.
   */
  latencyMs?: number;
  /** Start as the app does after setting a damaged download list aside, with its notice. */
  damagedList?: boolean;
  /** Start as the app does when it could not make its data folder private, with its notice; after the damaged list's, as the core gives them. */
  dataFolderNotPrivate?: boolean;
}
