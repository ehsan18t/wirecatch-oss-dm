# Publishing the extension

## One Chromium id everywhere

The Chromium extension id is `chdgnmpjoipbgfeldicipccjjghmagki` (`chromium_id` in `config/product.toml`). It follows from the public key in `chromium_public_key`, and it is the only Chromium id any Wirecatch bridge trusts, in development and release builds alike (D22). So the Chrome Web Store copy must have this id too, or Chrome refuses to connect it to Wirecatch.

Every Chromium build (`pnpm dev`, `pnpm build`, `pnpm build:dev`) puts the public key in the manifest's `key` field, which gives an unpacked load this id. The one exception is the store package, because the Chrome Web Store does not accept a manifest with a `key` field.

## The private key

The private key that belongs to `chromium_public_key` is kept by the maintainer in a private place outside the repository (for example a password manager). Anyone holding it can sign a packed extension with our id, so:

- Never commit it. The repository ignores `*.pem` files as a safety net, not as a reason to keep one in a checkout.
- Never put it in CI, in `config/`, or in this folder.
- Use it only for the first Chrome Web Store upload, as described below.

## Chrome Web Store

1. Build the package: `pnpm build:store`. It first runs `node scripts/gate.mjs release-build-extension` from the repository root, which warns while `config/product.toml` still holds a placeholder, so the package is for testing only (D49), and fails while the Public Suffix List in `tldts` is too old to ship (D35): update `tldts` to its latest release before every release, which the check needs the registry to confirm. `pnpm sign:firefox` runs `publish-extension`, which fails on a placeholder too (D29). It builds `.output/chrome-mv3-store` and zips it to `.output/wirecatchextension-<version>-chrome-store.zip`. It is the same as `pnpm build` without the `key` field.
2. First upload only: add the private key to the root of the zip, named `key.pem`, for example by copying it into an empty temporary folder as `key.pem` and running `zip -j .output/wirecatchextension-<version>-chrome-store.zip <that folder>/key.pem`. Upload that zip as a new item, then delete the temporary copy and that zip. The Chrome Web Store uses the key to give the item our id instead of a new one.
3. Before publishing, check that the item's id in the developer dashboard is `chdgnmpjoipbgfeldicipccjjghmagki`. An item's id never changes, so if it shows another id, remove that item and upload again with the key.
4. Every later update: upload the zip from step 1 as it is, without `key.pem`.

Other Chromium stores, such as Microsoft Edge Add-ons, give their own ids, which Wirecatch does not trust. Edge users install the extension from the Chrome Web Store.

### What the listing asks for

The developer dashboard's Privacy tab asks for a single purpose, a reason for each permission, and the data the extension handles. These are the answers; the permissions themselves are in `wxt.config.ts`, and nothing here may promise more than the code does (D61).

**Single purpose.** Hands the downloads, links and videos a person picks in the browser to the Wirecatch download manager on the same computer, which downloads them.

**Permissions.**

| Permission                                  | Why                                                                                                                                                                                                                                                                                                                                                                                                          |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `downloads`                                 | To see a download as the browser starts it, pause it while Wirecatch is asked to take it, and remove the browser's copy once Wirecatch has it, or let it go on in the browser when Wirecatch cannot take it.                                                                                                                                                                                                 |
| `nativeMessaging`                           | To talk to the Wirecatch app on the same computer, which stores the settings and does the downloading. Nothing is sent anywhere else.                                                                                                                                                                                                                                                                        |
| `cookies`                                   | A download behind a sign-in needs the cookies the browser would send with it, so Wirecatch downloads with the same access. They are read only for the download being handed over and go only to the Wirecatch app.                                                                                                                                                                                           |
| `webRequest`                                | To notice video and audio a page plays (streams and files) so the popup and the button over videos can offer them, and to read the `Cookie` header the browser sent with that media, so Wirecatch sends what the browser sent. Requests are only observed, never blocked or changed.                                                                                                                         |
| `contextMenus`                              | For "Download with Wirecatch" when right-clicking a link, image, video or audio.                                                                                                                                                                                                                                                                                                                             |
| `storage`                                   | To keep, for the current browser session only, what lets a background the browser stopped pick up where it was: the settings the Wirecatch app sent, the downloads being handed over, each tab's media list with the `Cookie` header sent for each item, messages for the popup, and the extension's error log (secrets left out). The browser clears it when it closes, and content scripts cannot read it. |
| Host access to all `http` and `https` sites | Downloads and videos come from any site: the cookies and requests above are read for whichever site a download or video comes from, and the button over videos runs on any page.                                                                                                                                                                                                                             |

**Remote code.** None: every script ships in the package.

**Data use.** Declare _Authentication information_ (the cookies of a download's or a video's site) and _Web history_ (the addresses of downloads, of the pages they come from, and of media on a page). Both go only to the Wirecatch app on the same computer, to download what the person asked for, and are kept in the browser's session storage until it closes. Certify that the data is not sold, not used or transferred for purposes unrelated to the single purpose, and not used to determine creditworthiness or for lending. Nothing is collected from private windows.

**Privacy policy.** Give the public address of `PRIVACY.md` at the root of the repository (on the website once it exists, or in the GitHub repository until then).

## Firefox

Firefox builds use the add-on id `extension@wirecatch.app` (`firefox_id`) from `browser_specific_settings.gecko` and never carry a `key`. Every bridge lists that id in `allowed_extensions`. `pnpm build:firefox` builds `.output/firefox-mv3`; `pnpm zip` also makes the Firefox package and its sources zip.

### Signing for self-distribution

Release and Beta Firefox only install add-ons signed by Mozilla. Signing on the unlisted channel gives a signed `.xpi` that we host ourselves (for example on the Wirecatch website or GitHub Releases) and that release Firefox installs permanently. It is not shown on addons.mozilla.org.

1. Create an account on [addons.mozilla.org](https://addons.mozilla.org) (a Mozilla account), then make API credentials under Tools, Manage API Keys (<https://addons.mozilla.org/developers/addon/api/key/>). The JWT issuer is the API key and the JWT secret is the API secret. Keep both in a password manager, never in the repository or in a config file.
2. Run `pnpm sign:firefox` with the credentials in the environment, for example `WEB_EXT_API_KEY=... WEB_EXT_API_SECRET=... pnpm sign:firefox`. It builds the Firefox package and its sources zip, sends both to addons.mozilla.org on the unlisted channel, and saves the signed `.xpi` in `.output/signed`. web-ext reads the credentials from those variables itself, and config file discovery is turned off, so no file can supply them. Automatic signing usually takes a few minutes; Mozilla may still review unlisted versions later.
3. Raise `version` in `package.json` before each new submission: addons.mozilla.org refuses a version number it has already signed.

The first signing creates the add-on on addons.mozilla.org under `extension@wirecatch.app`, which reserves that id for our account so nobody else can claim it. D21 lists this as an action only the maintainer can take; do it soon, even before the first release.

addons.mozilla.org asks for the source code of bundled add-ons, which is why the sources zip is uploaded. That zip holds only `apps/extension`; the build also needs `packages/product` (generated from `config/product.toml`), `packages/protocol` and the workspace lockfile, so if a reviewer asks how to rebuild, point them to the tagged release in the GitHub repository and the build steps in `CONTRIBUTING.md`.

Self-distributed copies do not update by themselves unless the manifest gets an `update_url`; until then people install each new `.xpi` from our download page.

### Listing on addons.mozilla.org later

To list Wirecatch publicly, submit a version on the listed channel from the add-on's page in the Developer Hub, or with `web-ext sign --channel=listed` and an `--amo-metadata` file that has the listing details (name, summary, categories, license). Listed versions are reviewed by Mozilla and then hosted and updated by addons.mozilla.org. The add-on keeps the same id, so the bridge needs no change.

### Testing unsigned builds

- Any Firefox: open `about:debugging#/runtime/this-firefox`, choose Load Temporary Add-on, and pick `.output/firefox-mv3/manifest.json` (or run `pnpm dev:firefox`). It keeps the `extension@wirecatch.app` id, so it talks to an installed Wirecatch, and it is removed when Firefox restarts.
- To keep an unsigned build installed: Firefox Developer Edition, Nightly, ESR or an unbranded build, with `xpinstall.signatures.required` set to `false` in `about:config`, then install the zip from `pnpm zip`. Release and Beta Firefox ignore that setting.
