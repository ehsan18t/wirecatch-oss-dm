/** The protocol versions this extension speaks, and what it makes of the one the app chooses (D32). */
import {
  CALL_SINCE,
  MIN_PROTOCOL_VERSION,
  PROTOCOL_VERSION,
  type Call,
  type Hello,
  type ProtocolError,
  type Reply,
} from '@wirecatch/protocol';
import { PRODUCT_NAME } from '@wirecatch/product';
import type { Problem } from './status';

/** The protocol version that added each call, as the generated `CALL_SINCE` gives it. */
export type CallSince = Readonly<Record<Call['method'], number>>;

/** Whether a connection on protocol `version` has the call `method`, by `callSince` or the generated table. */
export function hasCall(
  callSince: CallSince | undefined,
  version: number,
  method: Call['method'],
): boolean {
  return (callSince ?? CALL_SINCE)[method] <= version;
}

/** Why a call newer than the connection's protocol version fails without being sent: the same code the app answers such a call with, so callers handle both alike. */
export const CALL_NEWER_THAN_APP: ProtocolError = {
  code: 'unsupported_call',
  message: `This version of the ${PRODUCT_NAME} app cannot do that yet. Update the ${PRODUCT_NAME} app to use it.`,
};

/** The protocol versions this extension speaks, as `hello` names them. An app from before version ranges reads only `protocol_version` and accepts only 1 there, so the oldest version goes in it. */
export const SPOKEN_VERSIONS = {
  protocol_version: MIN_PROTOCOL_VERSION,
  max_protocol_version: PROTOCOL_VERSION,
} satisfies Partial<Hello>;

/** What is wrong with the protocol version the app chose for the connection, or null when this extension speaks it. */
export function versionProblem(version: number): Problem | null {
  if (!Number.isInteger(version)) {
    return {
      kind: 'version_mismatch',
      detail: `${PRODUCT_NAME} answered with protocol version ${version}, which does not exist.`,
    };
  }
  if (version < MIN_PROTOCOL_VERSION) {
    return {
      kind: 'version_mismatch',
      detail: `${PRODUCT_NAME} speaks protocol version ${version}, and this extension needs ${MIN_PROTOCOL_VERSION} or newer. Update the ${PRODUCT_NAME} app.`,
    };
  }
  if (version > PROTOCOL_VERSION) {
    return {
      kind: 'version_mismatch',
      detail: `${PRODUCT_NAME} chose protocol version ${version}, and this extension speaks up to ${PROTOCOL_VERSION}. Update the extension.`,
    };
  }
  return null;
}

/** What is wrong with the answer to `hello`, or null when the app can be talked to. */
export function helloProblem(reply: Reply): Problem | null {
  if (reply.type !== 'hello') {
    return { kind: 'disconnected', detail: `${PRODUCT_NAME} gave an unexpected answer to hello.` };
  }
  return versionProblem(reply.data.protocol_version);
}
