// The cargo arguments that build the browser bridge for bundling.
//
// No bridge feature changes which extensions it trusts (D22), so debug and release builds differ
// only in the profile. Its one feature, test-overrides, is for tests and must never ship.

/**
 * @param {boolean} release
 * @returns {string[]}
 */
export function bridgeCargoArgs(release) {
  const args = ['build', '-p', 'wirecatch-bridge'];
  return release ? [...args, '--release'] : args;
}
