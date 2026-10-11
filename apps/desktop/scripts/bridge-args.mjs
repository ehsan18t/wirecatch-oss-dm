// The cargo arguments that build the browser bridge for bundling.
//
// A development build trusts the unpacked development extension as well (the bridge's
// `dev-extension` feature); a release build must never: it trusts only the store extensions.

/**
 * @param {boolean} release
 * @returns {string[]}
 */
export function bridgeCargoArgs(release) {
  return release
    ? ['build', '-p', 'wirecatch-bridge', '--release']
    : ['build', '-p', 'wirecatch-bridge', '--features', 'dev-extension'];
}
