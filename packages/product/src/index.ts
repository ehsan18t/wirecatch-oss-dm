// Generated from config/product.toml by `pnpm product:generate`. Do not edit: change config/product.toml and run it again.

/** The product name people see. */
export const PRODUCT_NAME = 'Wirecatch';

/** The lowercase product name used in file names. */
export const PRODUCT_SLUG = 'wirecatch';

/** The release version of the app and the extension. */
export const PRODUCT_VERSION = '0.1.0';

/** The license, as an SPDX expression. */
export const LICENSE = 'GPL-3.0-only';

/** Who wrote it, for the copyright line. */
export const AUTHORS = 'Wirecatch contributors';

/** One line for store listings and package managers. */
export const SUMMARY = 'A fast download manager that just works';

/** A few sentences for installers. */
export const DESCRIPTION =
  'Wirecatch downloads files from the web quickly and reliably, catches downloads from your browser, and keeps queues, schedules and speed limits simple.';

/** The legal name that publishes the installers. */
export const PUBLISHER = 'TODO-PRODUCT-PUBLISHER-LEGAL-NAME';

/** The source repository. */
export const REPOSITORY_URL = 'https://github.com/ehsan18t/wirecatch-oss-dm';

/** The product website. */
export const WEBSITE_URL = 'https://TODO-PRODUCT-WEBSITE.invalid/';

/** Where people ask for help. */
export const SUPPORT_EMAIL = 'TODO-PRODUCT-SUPPORT-EMAIL@placeholder.invalid';

/** Where vulnerabilities are reported privately. */
export const SECURITY_EMAIL = 'TODO-PRODUCT-SECURITY-EMAIL@placeholder.invalid';

/** The desktop app executable, without `.exe`. */
export const APP_EXECUTABLE = 'wirecatch-app';

/** The browser bridge executable, without `.exe`; it lives next to the app (D19). */
export const BRIDGE_EXECUTABLE = 'wirecatch-bridge';

/** The command-line downloader executable, without `.exe`. */
export const CLI_EXECUTABLE = 'wirecatch';

/** The download list, in the data folder. */
export const DATABASE_FILE = 'wirecatch.db';

/** The native messaging host name the bridge registers and the extension connects to. */
export const NATIVE_HOST_NAME = 'app.wirecatch.bridge';

/** The description in the bridge's native messaging manifests. */
export const NATIVE_HOST_DESCRIPTION = 'Wirecatch browser bridge';

/** The one Chromium extension id every bridge trusts (D22). */
export const CHROMIUM_EXTENSION_ID = 'chdgnmpjoipbgfeldicipccjjghmagki';

/** The origin Chromium passes for the extension, in the exact form it uses. */
export const CHROMIUM_EXTENSION_ORIGIN = 'chrome-extension://chdgnmpjoipbgfeldicipccjjghmagki/';

/** The public key that gives unpacked Chromium builds the extension id (D22). */
export const CHROMIUM_PUBLIC_KEY =
  'MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAtR5Ub/hMTwq2GZpy7c277pRnrpRRjEf+4yn6ojwohj61eU+YBFp1lQAh3TMMR3hJ5V2JSQe7avxce9BgJJBnr39ASQh7HSvxv6zPTXBfXINF0MpKeOcbTbOHjiDvZH0ch8UKcX3P5ut0TqqfWTUSyAVVI56jocY0bjHbpwYHZNJ2DLC5dG6gwJGL1KuuJnywcNnoks29bp9RyXKU8epD1GOxnIK/mW4tnahjszCQjOXB1gwblcAwM+X0eZgPM1No4oqGKyIXgzMZV1LfEhm2A8hffzcHjBQBG1BqfMi77mY8GXR4a/5Dfi+8hqOQlP/jrQ1u/nQylPM8p4V3En84NwIDAQAB';

/** The Firefox add-on id. */
export const FIREFOX_EXTENSION_ID = 'extension@wirecatch.app';

/** The extension's one line in browser stores. */
export const EXTENSION_DESCRIPTION = 'Sends your downloads to the Wirecatch download manager.';
