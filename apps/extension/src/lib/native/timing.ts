// How long the extension waits for the app, in milliseconds. The wait for a connection, which may include starting the app, is derived from the bridge's own start budget, which comes from crates/protocol.
import { BRIDGE_START_BUDGET_MS } from '@wirecatch/protocol';

/** How long the app may take to answer `hello` once the bridge has reached it: the app accepts connections as soon as it starts, and answers once its download list is open. */
const APP_OPEN_MS = 5_000;

/** How long connecting may take, starting the app included: the bridge's start budget, then the app's answer to `hello`. A call that may have to connect first waits this long on top of its own wait. */
export const START_WAIT_MS = BRIDGE_START_BUDGET_MS + APP_OPEN_MS;

/** How long a call waits for its answer once the app is connected. */
export const REQUEST_TIMEOUT_MS = 10_000;

/** How long to wait for the answer to `withdraw_download`, which only runs after a call already took too long. */
export const WITHDRAW_TIMEOUT_MS = 3_000;

/** Delays between reconnection attempts: quick at first (the app may be starting), then one every 30 seconds. */
export const RECONNECT_BACKOFF_MS = [1_000, 2_000, 4_000, 8_000, 15_000, 30_000] as const;
