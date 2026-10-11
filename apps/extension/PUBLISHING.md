# Publishing the extension

## One Chromium id everywhere

The Chromium extension id is `chdgnmpjoipbgfeldicipccjjghmagki` (`chromium_id` in `config/extension-ids.json`). It follows from the public key in `chromium_public_key`, and it is the only Chromium id any Wirecatch bridge trusts, in development and release builds alike (D22). So the Chrome Web Store copy must have this id too, or Chrome refuses to connect it to Wirecatch.

Every Chromium build (`pnpm dev`, `pnpm build`, `pnpm build:dev`) puts the public key in the manifest's `key` field, which gives an unpacked load this id. The one exception is the store package, because the Chrome Web Store does not accept a manifest with a `key` field.

## The private key

The private key that belongs to `chromium_public_key` is kept by the maintainer in a private place outside the repository (for example a password manager). Anyone holding it can sign a packed extension with our id, so:

- Never commit it. The repository ignores `*.pem` files as a safety net, not as a reason to keep one in a checkout.
- Never put it in CI, in `config/`, or in this folder.
- Use it only for the first Chrome Web Store upload, as described below.

## Chrome Web Store

1. Build the package: `pnpm build:store`. It builds `.output/chrome-mv3-store` and zips it to `.output/wirecatchextension-<version>-chrome-store.zip`. It is the same as `pnpm build` without the `key` field.
2. First upload only: add the private key to the root of the zip, named `key.pem`, for example by copying it into an empty temporary folder as `key.pem` and running `zip -j .output/wirecatchextension-<version>-chrome-store.zip <that folder>/key.pem`. Upload that zip as a new item, then delete the temporary copy and that zip. The Chrome Web Store uses the key to give the item our id instead of a new one.
3. Before publishing, check that the item's id in the developer dashboard is `chdgnmpjoipbgfeldicipccjjghmagki`. An item's id never changes, so if it shows another id, remove that item and upload again with the key.
4. Every later update: upload the zip from step 1 as it is, without `key.pem`.

Other Chromium stores, such as Microsoft Edge Add-ons, give their own ids, which Wirecatch does not trust. Edge users install the extension from the Chrome Web Store.

## Firefox

Firefox builds use the add-on id `extension@wirecatch.app` (`firefox_id`) from `browser_specific_settings.gecko` and never carry a `key`. `pnpm zip` makes the Firefox package and its sources zip for addons.mozilla.org.
