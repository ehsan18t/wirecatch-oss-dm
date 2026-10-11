// Types for sign-firefox.mjs, which runs under plain Node.

/** The environment variables web-ext reads the AMO JWT issuer and secret from. */
export const CREDENTIAL_VARS: readonly string[];

/** The credential variables that are missing or empty. */
export function missingCredentials(env: Record<string, string | undefined>): string[];

/** The web-ext arguments that sign the built Firefox package. */
export function signArgs(paths: {
  sourceDir: string;
  artifactsDir: string;
  sourcesZip: string | null;
}): string[];
