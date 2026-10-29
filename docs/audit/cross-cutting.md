# Cross-cutting audit: identity inventory and seams

> This audit is a snapshot: its paths and line numbers name the code as it was when the audit was made (it was recorded in commit 0bd101a). Some files have since moved or been split, so a path that no longer exists, or a line that no longer matches, refers to the code of that time; `git log -- <path>` shows the commit that moved or removed it.

Audited against the committed tree at `HEAD` (9e1580d, branch `refactor/code-quality`). Note: while this audit ran, the working tree gained uncommitted edits to every `Cargo.toml` (a new `[workspace.lints]` table plus `[lints] workspace = true` in each crate) and a new untracked `clippy.toml`. Findings below describe `HEAD`; XC-22 says what the in-progress change already covers.

Line numbers are for `HEAD`. "Prod" means non-test code; test-only occurrences are listed because a rename has to touch them too.

## Part 1: identity inventory

Runtimes column: R = Rust build (const or `include_str!`), T = TypeScript build (Vite/WXT define or TS module), J = JSON/other manifest (tauri conf, WXT manifest, NSIS, shell, package.json, Cargo.toml metadata), D = docs only.

`config/extension-ids.json` today holds 4 values (`chromium_id`, `chromium_public_key`, `firefox_id`, `native_host_name`). Readers: `crates/bridge/src/config.rs:13` (`include_str!`, parsed at runtime with `deny_unknown_fields`, ignores the public key), `crates/e2e/src/bridge.rs:27` (`include_str!`, untyped `serde_json::Value`), `apps/extension/wxt.config.ts:13-25` (`readFileSync`, checks 3 fields are non-empty strings, injects `__NATIVE_HOST_NAME__` at :42, `key` at :50, gecko id at :75), `apps/extension/tests/manifest.test.ts:14` (`readFileSync`). Everything else in the table below is hardcoded where it is used. Note for the `product.toml` design: the bridge's `deny_unknown_fields` (config.rs:18-19) makes adding any field to that JSON a compile-and-test break by design, so a generated file should replace the hand-parsed JSON rather than extend it.

| # | Value | Kind | Every file:line where it appears | Runtimes |
|---|---|---|---|---|
| 1 | `Wirecatch` | product / display name | Identity uses: `apps/desktop/src-tauri/tauri.conf.json:3` (productName), `:18` (main window title), `:51` (NSIS startMenuFolder); `apps/desktop/src-tauri/src/lib.rs:126` (`RELEASE_AUTOSTART_NAME`, the Windows Run value and Linux autostart file name), `:270` (dialog title); `apps/desktop/src-tauri/windows/hooks.nsh:18,19` (Run and StartupApproved values deleted on uninstall); `apps/desktop/src-tauri/src/download_windows.rs:467` (fallback window title); `apps/desktop/src-tauri/src/host.rs:171` (Windows data folder); `crates/bridge/src/install.rs:415` (`%LOCALAPPDATA%\Wirecatch\NativeMessagingHosts`); `apps/extension/wxt.config.ts:52` (manifest name), `:62` (action title), `:68` (command description); `apps/desktop/src/lib/backend/tauri.ts:243,250` ("Wirecatch settings" file filter); `apps/desktop/src-tauri/src/activity.rs:132-134` (tray tooltip); `apps/desktop/src-tauri/src/tray.rs:46` ("Open Wirecatch"). UI and error copy: 209 prod string literals in 65 files (apps/desktop 64 in 24 files, apps/extension 67 in 16, crates/core 35 in 13, crates/bridge 27 in 4, crates/ipc 8 in 3, crates/streams 5 in 2, crates/engine 3 in 3: `fetch.rs:868`, `transport.rs:246`, `download.rs:153`), plus 106 in tests. Tests: `apps/desktop/src-tauri/src/lib.rs:420`, `host.rs:236-237`, `tests/shell.rs:35`, `apps/desktop/src/lib/backend/mock*.ts`. Docs: README, DECISIONS D9, PACKAGING, PUBLISHING. | R, T, J, D |
| 2 | `Wirecatch Dev` | dev-build display name | `apps/desktop/src-tauri/src/host.rs:26` (`DEV_NAME`, used for title, data folder, autostart entry); `apps/desktop/src-tauri/examples/send_capture.rs:66` | R |
| 3 | `wirecatch` | lowercase product slug | Linux data folder `apps/desktop/src-tauri/src/host.rs:173`; ipc runtime subfolder `crates/ipc/src/paths.rs:16` (`RUNTIME_SUBDIR`); CLI binary `crates/cli/Cargo.toml:10`; DB writer thread `wirecatch-db` `crates/core/src/store.rs:222`; Tauri derives the deb/rpm package name from productName; tests `crates/ipc/src/paths.rs:120`, `crates/ipc/src/unix.rs:328`, `crates/bridge/src/install.rs:1339`, `crates/core/src/folders.rs:496`; docs `apps/desktop/PACKAGING.md:38` | R, J, D |
| 4 | `wirecatch-dev` | Linux dev data folder | `apps/desktop/src-tauri/src/host.rs:174` | R |
| 5 | `app.wirecatch.desktop` | Tauri app identifier | `apps/desktop/src-tauri/tauri.conf.json:6`; tests `apps/desktop/src-tauri/src/lib.rs:419,425` | J (R reads it via `tauri::generate_context!`) |
| 6 | `app.wirecatch.desktop.dev` | dev app identifier | `apps/desktop/src-tauri/src/host.rs:23` (`DEV_IDENTIFIER`) | R |
| 7 | `app.wirecatch.bridge` | native messaging host name | `config/extension-ids.json:6` (read by bridge `config.rs:13`, e2e `bridge.rs:27`, `wxt.config.ts:42` as `__NATIVE_HOST_NAME__`, declared in `apps/extension/src/env.d.ts:1`); test literals `crates/bridge/src/config.rs:118,146`, `crates/bridge/src/install.rs:1128-1129,1131,1140,1151-1163,1215-1216,1257-1261,1386-1397,1470-1475,1566`, `crates/bridge/src/caller.rs:110`, `crates/bridge/tests/bridge.rs:245,265`, `crates/e2e/src/extension.rs:49`, `apps/extension/tests/helpers.test.ts:149-190`; comment `apps/extension/src/lib/native/status.ts:44` | R, T, J |
| 8 | `chdgnmpjoipbgfeldicipccjjghmagki` | Chromium extension id | `config/extension-ids.json:3` (read by bridge and e2e); hardcoded in tests `crates/bridge/src/caller.rs:91,96,126-128`, `crates/bridge/src/config.rs:119,123,129`, `crates/bridge/src/install.rs:1091`, `crates/bridge/tests/bridge.rs:28,270`, `apps/desktop/src-tauri/tests/bridge.rs:24`; docs `apps/extension/PUBLISHING.md:5,21`, `docs/DECISIONS.md:131` | R, D |
| 9 | `MIIBIjANBgkq...` (`chromium_public_key`) | extension signing public key | `config/extension-ids.json:4` (read by `wxt.config.ts:50`, `apps/extension/tests/manifest.test.ts:14,32`; deliberately ignored by `crates/bridge/src/config.rs:24-26`) | T, J |
| 10 | `extension@wirecatch.app` | Firefox add-on id | `config/extension-ids.json:5` (read by bridge, e2e, `wxt.config.ts:75`); tests `crates/bridge/src/caller.rs:111,116,131,133,134`, `crates/bridge/src/config.rs:120`, `crates/bridge/src/install.rs:1144,1402`, `crates/bridge/tests/bridge.rs:29,273`; docs `apps/extension/PUBLISHING.md:28,38,50`, `docs/DECISIONS.md:127` | R, T, J, D |
| 11 | `wirecatch.app` | product domain (not yet registered, D21) | embedded in #5, #6, #7 (reverse DNS) and #10; `docs/DECISIONS.md:57,127` | D (implicit in R/J) |
| 12 | `\\.\pipe\wirecatch-app-<SID>` | Windows IPC pipe | `crates/ipc/src/paths.rs:12` (`PIPE_PREFIX`, `pub(crate)`), rewritten by string match in `apps/desktop/src-tauri/src/host.rs:192`; doc `crates/ipc/src/lib.rs:14`; tests `crates/ipc/src/paths.rs:111`, `crates/ipc/src/windows.rs:651`, `crates/ipc/tests/frames.rs:145`, `apps/desktop/src-tauri/src/host.rs:293`; `docs/DECISIONS.md:88` | R |
| 13 | `\\.\pipe\wirecatch-app-dev-<SID>` | dev pipe | derived by `replacen` in `apps/desktop/src-tauri/src/host.rs:192`; doc `:185`; test `:296` | R |
| 14 | `$XDG_RUNTIME_DIR/wirecatch/app.sock` | Linux IPC socket | `crates/ipc/src/paths.rs:16,20` (`SOCKET_NAME`); matched by suffix in `apps/desktop/src-tauri/src/host.rs:189`; doc `crates/ipc/src/lib.rs:25`; tests `crates/ipc/src/paths.rs:121,129`, `crates/ipc/tests/frames.rs:29,148`, `crates/bridge/src/app.rs:368`, `crates/bridge/tests/bridge.rs:65`, `apps/desktop/src-tauri/tests/bridge.rs:45`, `crates/e2e/src/app.rs:156`, `apps/desktop/src-tauri/src/host.rs:298` | R |
| 15 | `app-dev.sock` | dev socket | `apps/desktop/src-tauri/src/host.rs:190`; test `:301` | R |
| 16 | `app.lock` | Linux listener lock file | `crates/ipc/src/paths.rs:24` | R |
| 17 | `/tmp/wirecatch-<uid>` | Linux socket fallback folder | `crates/ipc/src/paths.rs:41`; doc `crates/ipc/src/lib.rs:26`; tests `crates/ipc/src/paths.rs:128,147,160,185` | R |
| 18 | `wirecatch.db` | database file | `crates/core/src/store.rs:24` (`DATABASE_FILE`); tests `crates/core/src/owner_only.rs:229,250`, `apps/desktop/src-tauri/src/host.rs:239-278`; stale wrong name `downloads.db` in `crates/bridge/src/install.rs:1111` | R |
| 19 | `%LOCALAPPDATA%\Wirecatch\NativeMessagingHosts` | bridge manifest folder (Windows) | `crates/bridge/src/install.rs:415-416`; doc `:7` | R |
| 20 | `<data dir>/bin` (`~/.local/share/wirecatch/bin`) | AppImage private bridge folder | `apps/desktop/src-tauri/src/lib.rs:307`; docs `apps/desktop/PACKAGING.md:38` | R, D |
| 21 | `wirecatch-app` | app executable | `apps/desktop/src-tauri/Cargo.toml:17` ([[bin]]), `apps/desktop/src-tauri/tauri.conf.json:4` (mainBinaryName), `crates/bridge/src/app.rs:24-28` (`APP_FILE_NAME`), `apps/desktop/src-tauri/src/appimage.rs:19` (`LAUNCHER_FILE_NAME`); tests `apps/desktop/src-tauri/src/bridge.rs:211-213`, `lifecycle.rs:291`, `window.rs:186-189,195`, `autostart.rs:145`, `crates/bridge/src/shell_parent.rs:207,213`; comment strings `apps/desktop/src/lib/state/app.svelte.ts:340`, `crates/e2e/Cargo.toml:10` | R, J |
| 22 | `wirecatch-bridge` | bridge executable | `crates/bridge/Cargo.toml:11`; `apps/desktop/src-tauri/src/bridge.rs:13-17` (`BRIDGE_FILE_NAME`); `apps/desktop/src-tauri/tauri.bundle.conf.json:4` (externalBin); `apps/desktop/scripts/prepare-bridge.mjs:39,46`; `apps/desktop/src-tauri/windows/hooks.nsh:12,13`; `apps/desktop/src-tauri/linux/prerm.sh:14`; `crates/bridge/src/main.rs:27,80`; `crates/e2e/src/bridge.rs:61-95`; `apps/desktop/src-tauri/tests/bridge.rs:55-78`; `crates/bridge/tests/bridge.rs:25` | R, J |
| 23 | `wirecatch-e2e-app` | test app executable | `crates/e2e/Cargo.toml:14-15`; `crates/e2e/tests/e2e.rs:405` | R |
| 24 | `Wirecatch contributors` | publisher / copyright holder | `apps/desktop/src-tauri/tauri.conf.json:33,34`. No `authors` in any Cargo.toml or package.json. | J |
| 25 | `GPL-3.0-only` | license | `Cargo.toml:9`; `package.json:4`; `apps/desktop/package.json:5`; `apps/extension/package.json:5`; `packages/protocol/package.json:5`; `apps/desktop/src-tauri/tauri.conf.json:34,35`; `LICENSE`, `crates/engine/LICENSE` | J |
| 26 | `A fast download manager that just works` | short description | `apps/desktop/src-tauri/tauri.conf.json:38` | J |
| 27 | `Wirecatch downloads files from the web quickly...` | long description | `apps/desktop/src-tauri/tauri.conf.json:39` | J |
| 28 | `Sends your downloads to the Wirecatch download manager.` | extension store description | `apps/extension/wxt.config.ts:53` | T/J |
| 29 | `Wirecatch browser bridge` | native host manifest description | `crates/bridge/src/install.rs:53` | R |
| 30 | per-package descriptions | package metadata | `crates/*/Cargo.toml:3` (8 crates), `apps/desktop/src-tauri/Cargo.toml:3`, `apps/desktop/package.json:6`, `apps/extension/package.json:6`, `packages/protocol/package.json:6` | J |
| 31 | `0.1.0` | product version | `Cargo.toml:7` (workspace), `:13,14` (path dep versions), `crates/core/Cargo.toml:13`, `crates/bridge/Cargo.toml:29`, `apps/desktop/src-tauri/tauri.conf.json:5`, `apps/desktop/package.json:3`, `apps/extension/package.json:3` (WXT manifest version), `packages/protocol/package.json:3`. Read at runtime via `env!("CARGO_PKG_VERSION")` in `crates/engine/src/download.rs:153`, `crates/core/src/profile.rs:38`, `crates/core/src/server.rs:138,164`, and `browser.runtime.getManifest().version` in `apps/extension/src/entrypoints/background.ts:133` | R, T, J |
| 32 | `PROTOCOL_VERSION = 1` | wire protocol version | `crates/protocol/src/lib.rs:21`; regex-scraped by `packages/protocol/scripts/write-index.mjs:16` into `packages/protocol/src/index.ts:3` | R, T |
| 33 | `UNKNOWN_REQUEST_ID = 0` | wire constant | `crates/protocol/src/wire.rs:19`; scraped by `write-index.mjs:19` into `index.ts:5` | R, T |
| 34 | `1024 * 1024` (1 MiB frame cap) | wire limit | `crates/protocol/src/frame.rs:14` (`MAX_FRAME_LEN`); copied by hand in `apps/extension/src/lib/native/client.ts:15` (`MAX_MESSAGE_BYTES`) and in copy `:290`; `docs/DECISIONS.md:88` | R, T |
| 35 | `SETTINGS_FILE_FORMAT_VERSION = 2` | settings file format | `crates/core/src/profile.rs:17` | R |
| 36 | `Wirecatch/<CARGO_PKG_VERSION>` | HTTP User-Agent | `crates/engine/src/download.rs:153` (uses the engine crate version, not the app version) | R |
| 37 | `WirecatchE2E/1.0` | test User-Agent | `crates/e2e/src/app.rs:265` | R |
| 38 | `wirecatch://event` | Tauri event name | `apps/desktop/src-tauri/src/lib.rs:122`; `apps/desktop/src/lib/backend/types.ts:229`; `apps/desktop/src-tauri/capabilities/default.json:4`, `download-windows.json:4` (descriptions) | R, T |
| 39 | `wirecatch://resync` | Tauri event name | `apps/desktop/src-tauri/src/lib.rs:141`; `apps/desktop/src/lib/backend/types.ts:233` | R, T |
| 40 | `wirecatch://links` | Tauri event name | `apps/desktop/src-tauri/src/window.rs:15`; `apps/desktop/src/lib/backend/types.ts:234` | R, T |
| 41 | `.wcpart` | partial download extension | `crates/engine/src/storage.rs:8` (`PART_EXT`); `.gitignore:2`; tests `crates/core/src/naming.rs:251-284`, `crates/e2e/src/app.rs:104` | R |
| 42 | `.wcstate` | resume state extension | `crates/engine/src/storage.rs:10` (`STATE_EXT`); `.gitignore:3-4` | R |
| 43 | `wirecatch-stream 1` | on-disk stream workdir marker | `crates/streams/src/workdir.rs:43` | R |
| 44 | `wirecatch-settings.json` | settings export default file name | `apps/desktop/src/lib/state/app.svelte.ts:845`; mock `apps/desktop/src/lib/backend/mock.ts:1440` | T |
| 45 | `wirecatch.inFlight`, `wirecatch.media.v1`, `wirecatch.cache.v1`, `wirecatch.mediaButton.v1` | extension persisted storage keys | `apps/extension/src/entrypoints/background.ts:20,21`; `apps/extension/src/lib/cache.ts:5`; `apps/extension/src/lib/media/messages.ts:31` | T |
| 46 | `wirecatch-download`, `wirecatch-popup`, `wirecatch-media`, `wirecatch:media-check`, `wirecatch:media-announce`, `wirecatch:media-download` | extension context-menu id, port name, DOM id, message types | `apps/extension/src/entrypoints/background.ts:19`; `apps/extension/src/lib/messages.ts:10`; `apps/extension/src/lib/media/button.ts:257`; `apps/extension/src/lib/media/messages.ts:6,12,41` | T |
| 47 | `wirecatch.hints.dismissed`, `wirecatch.fit-window` | desktop localStorage keys | `apps/desktop/src/lib/state/hints.svelte.ts:17`; `apps/desktop/src/lib/window/fit.ts:6`; tests `apps/desktop/e2e/helpers.ts:13,17` | T |
| 48 | `application/x-wirecatch-downloads` | drag-and-drop MIME type | `apps/desktop/src/lib/dnd.ts:5` | T |
| 49 | `WIRECATCH_DATA_DIR` | env var (data folder override) | `apps/desktop/src-tauri/src/host.rs:14` | R |
| 50 | `WIRECATCH_LOG` | env var (log filter) | `crates/cli/src/main.rs:55`; `crates/core/examples/headless.rs:26` (desktop uses `RUST_LOG` instead, `apps/desktop/src-tauri/src/lib.rs:144`) | R |
| 51 | `WIRECATCH_BRIDGE_TEST_ENDPOINT`, `WIRECATCH_BRIDGE_TEST_LAUNCH`, `WIRECATCH_E2E_BRIDGE`, `WIRECATCH_TEST_FAKE_APP_ENDPOINT`, `WIRECATCH_TEST_FAKE_APP_MODE` | test env vars | `crates/bridge/src/app.rs:46,48`; `crates/bridge/tests/bridge.rs:26,27,32,33`; `apps/desktop/src-tauri/tests/bridge.rs:27,28`; `crates/e2e/src/bridge.rs:17,20,24` | R |
| 52 | `WIRECATCH_TEST_FFMPEG`, `WIRECATCH_REQUIRE_FFMPEG` | test env vars | `.github/workflows/ci.yml:59,60`; `scripts/linux-test.sh:39`; `crates/core/src/media/convert.rs:358,362`; `crates/core/src/media/merge.rs:240,244`; `crates/core/tests/manager/media_support.rs:262`; `crates/streams/tests/ffmpeg.rs:38,59`; `CONTRIBUTING.md:39` | R, J, D |
| 53 | `@wirecatch/*`, `wirecatch-*`, `wirecatch-workspace` | package namespaces | `package.json:2,10`; `apps/*/package.json:2`; `packages/protocol/package.json:2,12`; every `crates/*/Cargo.toml:2`; `apps/desktop/src-tauri/Cargo.toml:2,13` (`wirecatch_desktop_lib`) | J |
| 54 | Firefox `140.0`, Chrome `116` | browser minimums | `apps/extension/wxt.config.ts:77,85` | T/J |
| 55 | `1420` / `1421` | dev server and HMR ports | `apps/desktop/src-tauri/tauri.conf.json:9`; `apps/desktop/vite.config.ts:13,16` | J, T |
| - | update endpoint and updater public key | updater URL | none yet; placeholder only in `apps/desktop/PACKAGING.md:51-52` (`https://github.com/<org>/wirecatch/releases/latest/download/latest.json`); `tauri-plugin-updater` is compiled in (`apps/desktop/src-tauri/Cargo.toml:31`) | (future J) |
| - | homepage / repository URL | metadata | none anywhere; `crates/engine/Cargo.toml:11` says they come later | (future J) |
| - | store listing URLs (Chrome Web Store, AMO) | store URLs | none; `apps/extension/src/lib/native/status.ts:108` tells users to install "from the Wirecatch website or store" with no link | (future T) |
| - | contact / security email | email | none; `CONTRIBUTING.md:53` asks for private vulnerability reports with no channel | (future D) |

Distinct identity values found: 55 (rows 1 to 55), plus 4 that are referenced but do not exist yet.

## Part 2: findings on seams

## XC-1 Product identity is hardcoded across 3 languages and 5 manifest formats
- severity: high
- category: magic-value
- location: config/extension-ids.json:1-7, apps/desktop/src-tauri/tauri.conf.json:3-6, apps/desktop/src-tauri/src/host.rs:14-26, apps/desktop/src-tauri/src/lib.rs:122-141, crates/ipc/src/paths.rs:12-24, crates/bridge/src/app.rs:24-28, crates/bridge/src/install.rs:53, crates/bridge/src/install.rs:415, crates/core/src/store.rs:24, apps/extension/wxt.config.ts:52-68, apps/desktop/src-tauri/windows/hooks.nsh:12-19, apps/desktop/src-tauri/linux/prerm.sh:14
- problem: Only 4 of the 55 identity values in Part 1 have a single source (`config/extension-ids.json`); the rest are literals repeated per use. Renaming, white-labelling or adding a second channel (beta, enterprise) means hand-editing Rust consts, tauri JSON, WXT config, NSIS, a shell script and TS modules, with no compile-time link between them. Persisted names (storage keys #45, localStorage #47, data folder, db file, `.wcpart`) silently orphan user data if one copy changes and another does not.
- fix: Add `config/product.toml` holding every row of Part 1 (names, identifiers, ids, host name, pipe/socket stems, data folder names per platform and channel, db file name, executable names, publisher, descriptions, version, URLs, storage-key prefix). Generate from it: a `wirecatch-product` Rust crate (`build.rs` emitting `pub const`s, or a `product.rs` committed and drift-checked), a `packages/product` TS module, `config/extension-ids.json`-equivalent data for WXT, and the JSON/NSIS/sh fragments (tauri `productName`, `identifier`, `mainBinaryName`, `version`, `externalBin`, `startMenuFolder`, NSIS defines, `prerm.sh` path). Add a CI step that regenerates and fails on diff, the same way `packages/protocol` is checked. Leave UI copy literals alone for now (they belong to a future i18n pass), but move every identity use listed in row 1 to the generated constant.
- cross-package: yes (protocol, core, ipc, bridge, engine, desktop, extension, config)

## XC-2 Product version lives in 9 places with no drift check
- severity: medium
- category: magic-value
- location: Cargo.toml:7, Cargo.toml:13-14, crates/core/Cargo.toml:13, crates/bridge/Cargo.toml:29, apps/desktop/src-tauri/tauri.conf.json:5, apps/desktop/package.json:3, apps/extension/package.json:3, packages/protocol/package.json:3
- problem: A release bump must touch every file by hand. `tauri.conf.json:5` overrides the Cargo version that Tauri would otherwise inherit, so the installer version and `CARGO_PKG_VERSION` (sent as `app_version` in hello replies, `crates/core/src/server.rs:138,164`) can disagree. The extension version comes from its own package.json. Nothing fails when they drift.
- fix: Make `product.toml` (XC-1) the version source; generate the package.json versions and drop `version` from `tauri.conf.json` so Tauri reads `Cargo.toml`. Add a CI check that all versions match. Use `cargo release` or a small `scripts/bump-version` that rewrites them together.
- cross-package: yes (workspace, desktop, extension, protocol)

## XC-3 Desktop derives the dev endpoint by string-rewriting ipc's private naming
- severity: medium
- category: design
- location: apps/desktop/src-tauri/src/host.rs:187-195, crates/ipc/src/paths.rs:12, crates/ipc/src/paths.rs:20
- problem: `dev_endpoint` checks `strip_suffix("app.sock")` and otherwise does `replacen("wirecatch-app-", "wirecatch-app-dev-", 1)`. Both strings are `pub(crate)` details of `wirecatch-ipc`. If ipc renames its prefix or socket name, `replacen` becomes a no-op and debug builds serve the release endpoint, so a dev copy fights an installed Wirecatch for the pipe (exactly what D13/host.rs:16-19 tries to prevent). The unit test at host.rs:293-301 hardcodes the same strings, so it would keep passing on its own literals rather than catch the drift.
- fix: Give `wirecatch-ipc` a public `endpoint_for(channel: Channel)` (or `endpoint_with_suffix(&str)`) that builds both names from its own constants, and call that from `host::endpoint()`. Delete the string rewrite.
- cross-package: yes (ipc, desktop)

## XC-4 Data folder name is defined separately in the desktop and the bridge
- severity: medium
- category: duplication
- location: apps/desktop/src-tauri/src/host.rs:169-176, crates/bridge/src/install.rs:415-416, crates/bridge/src/install.rs:1110-1111
- problem: The app's data folder is chosen in `host::data_dir_for` (`Wirecatch`, `Wirecatch Dev`, `wirecatch`, `wirecatch-dev`), while the bridge writes its Windows manifests to a hardcoded `%LOCALAPPDATA%\Wirecatch\NativeMessagingHosts`. A debug app (data in `Wirecatch Dev`) and a release bridge share one folder, and a rename of either side silently splits them. The bridge test even guards a stale `downloads.db` next to it, while the real database is `wirecatch.db` (`crates/core/src/store.rs:24`), so it no longer tests what it claims.
- fix: Put the per-platform, per-channel data folder name in the generated product constants (XC-1) and use it from both `host.rs` and `install.rs`. Make the bridge test use `wirecatch_core::DATABASE_FILE` or the generated constant.
- cross-package: yes (desktop, bridge, core)

## XC-5 App and bridge executable names are duplicated in 6 formats
- severity: medium
- category: duplication
- location: apps/desktop/src-tauri/Cargo.toml:17, apps/desktop/src-tauri/tauri.conf.json:4, crates/bridge/src/app.rs:24-28, apps/desktop/src-tauri/src/appimage.rs:19, crates/bridge/Cargo.toml:11, apps/desktop/src-tauri/src/bridge.rs:13-17, apps/desktop/src-tauri/tauri.bundle.conf.json:4, apps/desktop/scripts/prepare-bridge.mjs:39, apps/desktop/scripts/prepare-bridge.mjs:46, apps/desktop/src-tauri/windows/hooks.nsh:12-13, apps/desktop/src-tauri/linux/prerm.sh:14
- problem: The bridge starts `wirecatch-app` from its own folder and the app registers `wirecatch-bridge` next to itself (D19). Each side hardcodes the other's file name, and the bundler, sidecar script, NSIS hook and Debian/RPM hook each repeat them. A rename in Cargo.toml compiles fine and ships a bridge that cannot start the app or an uninstaller that leaves browser registrations behind.
- fix: Generate both names from `product.toml` (Rust consts for `app.rs`, `bridge.rs`, `appimage.rs`; a JSON fragment for `tauri.conf.json`/`tauri.bundle.conf.json`; NSIS `!define` and a sourced shell variable for the hooks; an import in `prepare-bridge.mjs`). Add a test that the Cargo `[[bin]]` names equal the generated constants.
- cross-package: yes (bridge, desktop)

## XC-6 Nothing checks that chromium_id follows from chromium_public_key
- severity: medium
- category: security
- location: config/extension-ids.json:3-4, crates/bridge/src/config.rs:24-26, crates/bridge/src/config.rs:113-125, apps/extension/tests/manifest.test.ts:14-32
- problem: The bridge trusts `chromium_id`; the extension build ships `chromium_public_key`. D22 depends on the id being the SHA-256-derived id of that key, but the bridge ignores the key and the extension test only checks the key is copied into the manifest. Editing one field without the other produces release bridges that trust an id no build carries (every Chromium user broken), or an id derived from a key someone else holds. Only the manual step in `apps/extension/PUBLISHING.md:21` would notice.
- fix: Add a unit test (Rust in the bridge, or TS in the extension, or both) that base64-decodes `chromium_public_key`, takes SHA-256, maps the first 16 bytes to `a`-`p`, and asserts it equals `chromium_id`. Run it in CI.
- cross-package: yes (config, bridge, extension)

## XC-7 Four separate parsers for config/extension-ids.json, with different validation
- severity: medium
- category: duplication
- location: crates/bridge/src/config.rs:13-64, crates/e2e/src/bridge.rs:27-46, apps/extension/wxt.config.ts:7-25, apps/extension/tests/manifest.test.ts:14, apps/desktop/src-tauri/tests/bridge.rs:24, crates/bridge/tests/bridge.rs:28-29
- problem: The bridge validates id formats at runtime (a bad id surfaces only when a browser starts the bridge, not at build time), the extension only checks non-empty strings, e2e reads an untyped JSON value, and two test suites skip the file and hardcode the origin and Firefox id. The `chrome-extension://<id>/` origin format is built in `config.rs:78-80`, `e2e/src/bridge.rs:32`, and written out by hand in three test constants.
- fix: Replace runtime parsing with generated, validated constants (XC-1): the generator validates formats once and fails the build; Rust gets `const CHROMIUM_ID`, `const FIREFOX_ID`, `const NATIVE_HOST_NAME`, plus one `chromium_origin()` helper; TS gets the same constants. Tests import them instead of repeating literals.
- cross-package: yes (bridge, e2e, desktop, extension)

## XC-8 The Tauri webview contract is hand-mirrored, not generated from crates/protocol
- severity: high
- category: design
- location: apps/desktop/src/lib/backend/types.ts:24-52, apps/desktop/src/lib/backend/types.ts:175-235, apps/desktop/src-tauri/src/questions.rs:54-67, crates/core/src/manager/media.rs:44-64, apps/desktop/src/lib/backend/tauri.test.ts:63-70, apps/desktop/src-tauri/src/lib.rs:122, apps/desktop/src-tauri/src/lib.rs:141, apps/desktop/src-tauri/src/window.rs:15, apps/desktop/src/lib/backend/tauri.ts:95-99
- problem: D15 says every type crossing a process boundary comes from `wirecatch-protocol`. The webview to Rust boundary does not: `Question`, `MediaReading` and `AddonRequestItem` are serialized from `questions.rs` and `core::manager::media` and re-typed by hand in `types.ts`; the 50 command names exist three times (Rust `generate_handler!`, `COMMANDS`, and the test's `FIXED` list, compared by a regex over `lib.rs`); command argument names and shapes (`idsInOrder` relying on Tauri's camelCase to `ids_in_order` mapping) and return types are not checked at all; the three event names are duplicated Rust and TS. A Rust-side rename or new enum variant compiles and type-checks on both sides and fails only at runtime.
- fix: Either move these types into `crates/protocol` with `#[derive(TS)]` (and export the event-name and command-name constants through the generator), or adopt `tauri-specta`/`specta` to generate typed command bindings and event payloads into `packages/protocol`. Delete `FIXED` and the regex test once the binding is generated, and include the bindings in the existing "generated types are committed" CI check.
- cross-package: yes (protocol, core, desktop)

## XC-9 The 1 MiB frame limit is copied into the extension by hand
- severity: low
- category: duplication
- location: crates/protocol/src/frame.rs:14, apps/extension/src/lib/native/client.ts:15, apps/extension/src/lib/native/client.ts:290, packages/protocol/scripts/write-index.mjs:14-28
- problem: `MAX_MESSAGE_BYTES` must equal `MAX_FRAME_LEN` or oversized requests either get rejected by the bridge (dropping the port, which the client check exists to avoid) or get refused needlessly. `write-index.mjs` already exports two protocol constants but not this one.
- fix: Export `MAX_FRAME_LEN` from `@wirecatch/protocol` (see XC-10 for how) and import it in `client.ts`; build the "over 1 MiB" message from it.
- cross-package: yes (protocol, extension)

## XC-10 Protocol codegen scrapes Rust source with regex and never cleans its output
- severity: medium
- category: build
- location: packages/protocol/scripts/write-index.mjs:14-20, packages/protocol/package.json:11-13, .cargo/config.toml:2-3, .github/workflows/ci.yml:198-210
- problem: Constants reach TS by matching `pub const PROTOCOL_VERSION: u32 = (\d+);` against `lib.rs`; changing the type, adding a doc attribute on the same line, or writing `1_u32` breaks generation or, worse, silently matches a different line. Generation never deletes `src/generated`, so a removed or renamed Rust type leaves a stale `.ts` file that stays committed and exported, and the CI drift check (`git status --porcelain`) cannot see it because the stale file is unchanged (today the 60 generated files match the 60 `TS` derives exactly, so this is latent). `TS_RS_EXPORT_DIR` in `.cargo/config.toml` makes every `cargo test --workspace` rewrite the generated folder without rewriting `index.ts`. `packages/protocol` has no `check`, `lint` or prettier step, and `write-index.mjs` uses a different quote style from both apps.
- fix: Emit constants from Rust instead of scraping: a `#[test]` in `crates/protocol` (next to the ts-rs export) that writes `src/constants.ts` from the real values. Clear `src/generated` before exporting (in the `generate` script). Add `check` (`tsc --noEmit`) and `lint` scripts to `packages/protocol` so `pnpm check`/`pnpm lint` cover it.
- cross-package: yes (protocol, desktop, extension)

## XC-11 Audio quality choices and defaults are duplicated between core and the desktop UI
- severity: medium
- category: duplication
- location: crates/core/src/media/output.rs:152-168, apps/desktop/src/lib/media-output.ts:28-43
- problem: `KBPS_CHOICES`, `DEFAULT_KBPS` and the lossless set in TS restate `AudioCodec::kbps_choices` and `default_kbps` in Rust; the TS comment says "the core accepts these". If core changes a list, the UI offers values core rejects, and no test spans the two.
- fix: Send the choices and default with each `OutputFit` (it is already a generated protocol type), or export them as generated constants. Remove the TS tables.
- cross-package: yes (core, protocol, desktop)

## XC-12 Byte formatting and URL helpers exist three times with different behavior
- severity: low
- category: duplication
- location: apps/desktop/src/lib/format.ts:19-30, apps/desktop/src-tauri/src/activity.rs:106-125, apps/extension/src/lib/size.ts:34-45, apps/desktop/src/lib/format.ts:234-240, apps/extension/src/lib/sites.ts:30-37, apps/desktop/src/lib/format.ts:275-284, apps/extension/src/lib/file-types.ts:31-44
- problem: The same size reads `512 B` / `1.50 KB` in the desktop and tray but `512 bytes` / `1.5 KB` in the extension popup. `hostOf` returns `host` (with port) in the desktop and lower-cased `hostname` or null in the extension; `fileNameFromUrl` falls back to the whole URL on a malformed `%` escape in the desktop but to the raw segment in the extension. Users see inconsistent output and fixes land in one copy.
- fix: Create a small `packages/shared` (or `packages/format`) TS package used by both apps for formatting and URL helpers; keep the Rust tray formatter but pin it to the same cases with a shared table of expected outputs tested on both sides.
- cross-package: yes (desktop, extension)

## XC-13 Test helpers and test env var names are copied, and one copy can skip silently in CI
- severity: medium
- category: duplication
- location: crates/core/src/media/convert.rs:355-369, crates/core/src/media/merge.rs:237-251, crates/core/tests/manager/media_support.rs:260-265, crates/core/tests/manager/media.rs:792-796, crates/core/tests/manager/media_output.rs:430-434, crates/streams/tests/ffmpeg.rs:30-62, crates/e2e/src/bridge.rs:49-96, apps/desktop/src-tauri/tests/bridge.rs:52-78, crates/bridge/src/app.rs:46-48, crates/bridge/tests/bridge.rs:26-27, apps/desktop/src-tauri/tests/bridge.rs:27-28, crates/e2e/src/bridge.rs:17-24
- problem: `real_ffmpeg()` exists four times. The copy in `core/tests/manager/media_support.rs` ignores `WIRECATCH_REQUIRE_FFMPEG`, so the two core integration tests that join and convert real media print "skipped" and pass even on the CI leg that sets the requirement; `CONTRIBUTING.md:39` promises they cannot be skipped there. "Build the bridge with test-overrides" is implemented twice (e2e and desktop tests) with different arguments, both spawning a nested `cargo build` without `--locked` from inside a test. The bridge's test env var names are retyped in four crates.
- fix: Put `real_ffmpeg()` (with the REQUIRE behavior) in one test-support module, for example a `wirecatch-test-support` dev-only crate or `wirecatch-streams`' `test-util` feature, and use it everywhere. Make the desktop bridge test use `wirecatch_e2e::bridge::bridge_exe()` (add `wirecatch-e2e` as a dev-dependency) and pass `--locked` in the nested build. Export the env var names as `pub const` from the bridge (behind `test-overrides`) or from the shared test crate.
- cross-package: yes (core, streams, bridge, e2e, desktop)

## XC-14 The published engine carries Wirecatch product identity
- severity: medium
- category: design
- location: crates/engine/src/download.rs:153, crates/engine/src/fetch.rs:868, crates/engine/src/transport.rs:246, crates/engine/src/storage.rs:8-10
- problem: `wirecatch-engine` is a crates.io library, but its default User-Agent is `Wirecatch/<engine version>` and two user-facing errors say "which Wirecatch blocks" and "which Wirecatch cannot download from". Third-party users of the crate impersonate the app and show its name to their users; the app itself sends the engine crate version rather than the app version. The branded `.wcpart`/`.wcstate` extensions are fine as engine defaults but are not overridable.
- fix: Make the engine default UA `wirecatch-engine/<version>` (or require callers to set it), word engine errors without the product name, and let `wirecatch-core` set `user_agent` from the generated product name and app version. Consider exposing the part/state extensions as options with the current values as defaults.
- cross-package: yes (engine, core)

## XC-15 Shipped desktop builds log nowhere, and log filters differ per binary
- severity: high
- category: design
- location: apps/desktop/src-tauri/src/main.rs:2, apps/desktop/src-tauri/src/lib.rs:143-150, crates/cli/src/main.rs:52-58, crates/core/examples/headless.rs:26, apps/desktop/src-tauri/src/bridge.rs:82-83
- problem: Release Windows builds use `windows_subsystem = "windows"`, so stderr does not exist, and `init_logging` writes only to stderr. Every `tracing::error!` in core and the shell (for example "could not open the download manager" in `host.rs:49`) is lost in the field, which leaves support with nothing to read for a commercial product. Users are told to "see its tray icon for details" instead of a log. The desktop reads `RUST_LOG` while the CLI and headless example read `WIRECATCH_LOG`.
- fix: Add a rolling file appender (`tracing-appender`) under the data folder's `logs/`, keep stderr for debug builds, and expose "Open log folder" in settings or the tray. Use one env var (`WIRECATCH_LOG`) in every binary, defined once in the generated product constants. Write the convention into CLAUDE.md (XC-32).
- cross-package: yes (desktop, cli, core, bridge)

## XC-16 CI never builds, lints or tests the engine's http3 feature
- severity: high
- category: ci
- location: .github/workflows/ci.yml:86, .github/workflows/ci.yml:93, .github/workflows/ci.yml:104-108, crates/engine/Cargo.toml:43-47, crates/engine/src/lib.rs:216-217, crates/cli/Cargo.toml:22-25
- problem: No workspace member enables `wirecatch-engine/http3` by default and no CI command passes `--features http3` or `--all-features`. The 20 `cfg(feature = "http3")` sites, `src/http3.rs`, `src/http3/tests.rs` and the `bench_http3` example are never compiled in CI, so they can rot unnoticed in the crate that is published to crates.io (and docs.rs builds default features only, so the `http3` API docs are not rendered either).
- fix: Add `cargo clippy -p wirecatch-engine --all-targets --features http3 -- -D warnings` and `cargo test -p wirecatch-engine --features http3` to the Rust job (or use `cargo hack --each-feature` for the engine and cli). Add `[package.metadata.docs.rs] all-features = true` to the engine.
- cross-package: yes (engine, cli, ci)

## XC-17 CI clippy does not match the documented gate
- severity: medium
- category: ci
- location: .github/workflows/ci.yml:85-86, CLAUDE.md (Commands), CONTRIBUTING.md:18, docs/DECISIONS.md:101, crates/bridge/Cargo.toml:17-20, crates/bridge/src/app.rs:36-48
- problem: CLAUDE.md and CONTRIBUTING run clippy with `--features wirecatch-bridge/test-overrides`; CI runs it without. Because the bridge's `bridge` test target has `required-features = ["test-overrides"]`, CI clippy skips `crates/bridge/tests/bridge.rs` and all `cfg(feature = "test-overrides")` code, so lint failures there pass CI and fail locally. D18 states a third variant (no features, per touched crate).
- fix: Make CI run exactly the documented command. Keep one canonical gate list (a `just`/`cargo xtask ci` or `pnpm gate` script) that CI, CLAUDE.md, CONTRIBUTING and `scripts/linux-test.sh` all call, so they cannot diverge again.
- cross-package: yes (bridge, ci, docs)

## XC-18 Desktop Playwright suites (a11y, layout, performance) are not in CI
- severity: medium
- category: ci
- location: apps/desktop/package.json:21-22, apps/desktop/e2e/a11y.spec.ts, apps/desktop/e2e/layout.spec.ts, apps/desktop/e2e/performance.spec.ts, .github/workflows/ci.yml:212-222
- problem: The suites that check D3 commitments (10,000-row list stays smooth, layout at 960x600 to 1600x1000, accessibility) run only when someone types `pnpm e2e`. `pnpm test` is Vitest only, so regressions in those areas merge green.
- fix: Add a Linux job that installs Playwright browsers (`pnpm exec playwright install --with-deps chromium`), builds the mock (`pnpm --filter @wirecatch/desktop build:mock`) and runs `e2e` and `e2e:perf`, uploading the report on failure. Keep performance thresholds generous enough for shared runners or run perf on a schedule.
- cross-package: yes (desktop, ci)

## XC-19 Rust toolchain pins drift between CI, scripts and manifests
- severity: medium
- category: ci
- location: .github/workflows/ci.yml:24, .github/workflows/audit.yml:15, scripts/linux-test.sh:13, scripts/linux-test.sh:41-42, Cargo.toml:10, apps/desktop/src-tauri/Cargo.toml:8, .github/workflows/ci.yml:110-126
- problem: CI pins 1.99.0 in two workflow files that must be bumped together; `scripts/linux-test.sh` uses `rust:1.90` while CONTRIBUTING says it runs "the same way CI does" (it also skips the desktop crate in clippy and omits `--locked`); local developers get whatever rustup has because there is no `rust-toolchain.toml`. Every crate inherits `rust-version = 1.88` but only the engine is checked at that version, so the claim is unverified for protocol, ipc, streams and cli.
- fix: Add `rust-toolchain.toml` (channel `1.99.0`, components rustfmt and clippy) and have both workflows and the Docker script read it (dtolnay action reads it when `toolchain` is omitted; Docker can use `rust:<version>` derived from it). Either check every non-`publish = false` crate at MSRV or set `rust-version` only on the engine.
- cross-package: yes (ci, scripts, all crates)

## XC-20 No release or packaging workflow
- severity: medium
- category: ci
- location: .github/workflows/ci.yml, apps/desktop/PACKAGING.md:40-55, apps/extension/PUBLISHING.md, apps/desktop/package.json:14-15, apps/extension/package.json:13-16
- problem: Installers (`pnpm bundle`), updater signatures, Chrome Web Store zips and AMO signing are produced on a maintainer's machine. CI never runs `tauri build`, so packaging breakage (NSIS hooks, externalBin naming, deb/rpm scripts) is found at release time, and releases are not reproducible or attributable to a commit.
- fix: Add a tag-triggered `release.yml` that builds NSIS/deb/rpm/AppImage on the matrix, builds and zips both extension targets, signs with secrets (`TAURI_SIGNING_PRIVATE_KEY`, AMO JWT), and publishes the GitHub Release plus `latest.json`. Add a PR-time `tauri build --debug --bundles nsis,deb` smoke job (no signing) so packaging regressions surface before release.
- cross-package: yes (desktop, extension, bridge, ci)

## XC-21 Redundant and duplicated CI work
- severity: low
- category: ci
- location: .github/workflows/ci.yml:44-60, .github/workflows/ci.yml:85-96, .github/workflows/ci.yml:161-210
- problem: The Linux Rust job runs `apt-get update` twice (lines 47 and 57). Clippy (no features) and tests (with `test-overrides`) resolve different feature sets, so the bridge graph builds twice. The JS job installs Rust and compiles `wirecatch-protocol` tests on both Windows and Ubuntu only to regenerate types, though the drift check is OS-independent.
- fix: Merge the apt steps; run clippy and tests with the same feature set (XC-17); run protocol generation and the drift check once (Ubuntu) and let the Windows JS leg skip Rust.
- cross-package: yes (ci)

## XC-22 No workspace lint policy at HEAD; documented Rust rules are unenforced
- severity: high
- category: tooling
- location: Cargo.toml:1-39, crates/protocol/src/lib.rs, apps/desktop/src-tauri/src/lib.rs, crates/core/src/lib.rs:10, crates/engine/src/lib.rs:209, crates/streams/src/lib.rs:82, crates/ipc/src/lib.rs:35, crates/e2e/src/lib.rs:14, crates/bridge/src/main.rs:13
- problem: At HEAD there is no `[workspace.lints]`, no `clippy.toml`, no `rustfmt.toml`. `missing_docs` is `deny` in engine and streams, `warn` in core, ipc and e2e, and absent from `wirecatch-protocol` (the crate whose public types define every contract) and the desktop crate, contradicting "doc comment on every public item". Nothing enforces "no unwrap/expect on outside data" (no `clippy::unwrap_used`/`expect_used`), `dbg!`, `todo!`, or print statements outside the bridge. The uncommitted working-tree change adds `[workspace.lints]` (missing_docs, pedantic, unwrap_used, expect_used, allow_attributes) and `clippy.toml` with test allowances, which covers most of this once committed and the resulting warnings are fixed.
- fix: Land the in-progress `[workspace.lints]` with every crate opting in, remove the per-crate `#![warn/deny(missing_docs)]` attributes so there is one policy, add `clippy::await_holding_lock`, `clippy::large_futures` and a `disallowed-methods` list in `clippy.toml` (`std::thread::sleep`, `std::fs::*` in async modules) to make "no blocking calls on async tasks" partly checkable, and add a `rustfmt.toml` (even if it only pins `edition` and `style_edition`) so formatting does not depend on the toolchain default.
- cross-package: yes (all crates)

## XC-23 ESLint and Prettier configs differ between the two apps
- severity: medium
- category: tooling
- location: apps/desktop/eslint.config.js:13, apps/desktop/eslint.config.js:22, apps/desktop/eslint.config.js:25-35, apps/extension/eslint.config.js:11, apps/desktop/package.json:18, apps/extension/package.json:18, apps/desktop/.prettierrc:3, apps/extension/.prettierrc.json:3, packages/protocol/scripts/write-index.mjs:3
- problem: The extension uses `strictTypeChecked` with `projectService` for all files and `--max-warnings 0`; the desktop uses non-type-aware `strict`, enables `projectService` only for `.svelte` files, turns off `no-undef`, and has no `--max-warnings`, so `no-floating-promises`, `no-misused-promises` and `restrict-template-expressions` are not enforced in the desktop's plain `.ts` (backend, state) and warnings pass. Prettier `printWidth` is 100 in one app and 110 in the other. `packages/protocol` and root files are not formatted or linted by anything.
- fix: Add a root `eslint.config.base.js` (type-checked strict, shared rules) and a root `.prettierrc` that both apps extend; make every `lint` script use `--max-warnings 0`; add `packages/protocol` to lint and format. Optionally add `.editorconfig` at the root.
- cross-package: yes (desktop, extension, protocol)

## XC-24 The no-em-dash house rule is enforced by scattered one-off tests
- severity: low
- category: tooling
- location: apps/desktop/src-tauri/src/lib.rs:413, apps/desktop/src-tauri/src/bridge.rs:289, apps/desktop/src-tauri/src/tray.rs:189, crates/core/src/error.rs:203, apps/desktop/src/lib/backend/mock.test.ts:50, apps/desktop/src/lib/format.test.ts:180, apps/desktop/src/lib/media-output.test.ts:188, apps/desktop/src/lib/settings-schema.test.ts:22, apps/desktop/src/lib/state/app.svelte.test.ts:197, apps/extension/tests/helpers.test.ts:216
- problem: Ten hand-written assertions each guard one message family; every other string (engine errors, bridge output, extension UI, docs, commit messages) is unchecked. The rule is in CLAUDE.md and CONTRIBUTING but not mechanically enforced.
- fix: Add one CI step (`git grep -nP '\x{2014}' -- ':!*.lock' ':!*test*'` failing on any hit) and a `commit-msg` check in the same gate script; keep only tests that check generated or formatted text.
- cross-package: yes (all)

## XC-25 Workspace dependency hygiene: dead entry, inline duplicates, split versions
- severity: medium
- category: dependencies
- location: Cargo.toml:12-33, Cargo.toml:23, Cargo.toml:32, apps/desktop/src-tauri/Cargo.toml:32-33, apps/desktop/src-tauri/Cargo.toml:46, apps/desktop/src-tauri/Cargo.toml:59, crates/core/Cargo.toml:13, crates/core/Cargo.toml:53, crates/e2e/Cargo.toml:19-20, crates/e2e/Cargo.toml:26, crates/bridge/Cargo.toml:29, crates/engine/Cargo.toml:21, crates/engine/Cargo.toml:32, crates/engine/Cargo.toml:65-66, docs/PLAN.md (Rules: Dependencies)
- problem: `workspace.dependencies.reqwest` (with `rustls-tls-native-roots`) is used by no crate, while the engine declares its own `reqwest` with `rustls-tls-no-provider`; anyone who opts into the workspace entry pulls a second TLS root setup into the engine's graph. Internal crates `wirecatch-core`, `wirecatch-ipc` and `wirecatch-streams` are path dependencies repeated inline (some with `version = "0.1.0"`, some without). `tempfile = "3.27.0"` is declared three times, `tokio-rustls` twice in the engine alone, `tracing-subscriber` inline in the engine with different features. `windows-sys` is `0.59` in the workspace and `0.61.2` in the desktop, so two first-party copies compile (Cargo.lock has five windows-sys versions overall, most from Tauri). Workspace entries use loose majors (`"1"`) while crate-local entries use exact patches, so MSRV-affecting minimums are inconsistent. PLAN.md's rule "dependencies go in the package's own Cargo.toml" is the origin of this.
- fix: Delete the unused `reqwest` entry; add `wirecatch-core`, `wirecatch-ipc`, `wirecatch-streams`, `tempfile`, `tokio-rustls`, `rustls`, `rcgen`, `tauri*` and similar shared crates to `[workspace.dependencies]` with full minimum versions; move the workspace `windows-sys` to the version Tauri uses (0.61) so only Tauri's older transitive copies remain. Add `cargo-machete` or `cargo-udeps` and `cargo deny check bans` (multiple-versions warn list) to CI, and change the PLAN.md rule.
- cross-package: yes (workspace, engine, core, bridge, e2e, desktop)

## XC-26 Unused dependency in the CLI
- severity: low
- category: dependencies
- location: crates/cli/Cargo.toml:19, crates/cli/src/main.rs:9
- problem: `tokio-util` is declared but never referenced; `CancellationToken` comes from the engine's re-export.
- fix: Remove the dependency; a `cargo machete` CI step (XC-25) keeps it from coming back.
- cross-package: no

## XC-27 The two apps run different major versions of their JS toolchain
- severity: medium
- category: dependencies
- location: apps/desktop/package.json:34, apps/desktop/package.json:38, apps/desktop/package.json:45, apps/desktop/package.json:54, apps/desktop/package.json:57, apps/extension/package.json:27-28, apps/extension/package.json:34, apps/extension/package.json:43, apps/extension/package.json:45, pnpm-workspace.yaml:1-3
- problem: In one pnpm workspace the desktop uses ESLint 9.39 / `@eslint/js` 9, TypeScript 5.9, Vitest 4.1, `@sveltejs/vite-plugin-svelte` 6.2, while the extension uses ESLint 10.12, TypeScript 6.0, Vitest 5.0, vite-plugin-svelte 7.3. Shared code (XC-12) cannot move between them without type or lint differences, config files cannot be shared (XC-23), and Dependabot opens separate upgrade work per app.
- fix: Align both apps on the same majors and declare shared tools once with pnpm catalogs (`catalog:` in `pnpm-workspace.yaml`); have Dependabot group the catalog.
- cross-package: yes (desktop, extension)

## XC-28 Tauri Rust and JS package pairs can drift apart
- severity: medium
- category: dependencies
- location: apps/desktop/src-tauri/Cargo.toml:24-31, apps/desktop/package.json:28-29, apps/desktop/package.json:39, .github/dependabot.yml:20-39
- problem: `tauri 2.12.3` must stay in step with `@tauri-apps/api 2.12.3`, and `tauri-plugin-dialog 2.8.2` with `@tauri-apps/plugin-dialog 2.8.2`. Dependabot updates the cargo and npm sides in different grouped PRs, and nothing checks the pairs, so a merge can leave the webview API and the Rust runtime on different minors (Tauri warns about this only at build time on a developer machine).
- fix: Add a small test or CI script that reads both manifests and asserts each pair matches on major.minor; configure Dependabot to ignore the npm side of the pairs and bump them together (or bump both from one script).
- cross-package: yes (desktop, ci)

## XC-29 No license compliance or npm advisory gate
- severity: medium
- category: security
- location: .github/workflows/ci.yml:128-159, .github/workflows/audit.yml:1-30, .cargo/audit.toml:1-10, Cargo.toml:9
- problem: Only `cargo audit` runs. Nothing checks that the 648 crates and the npm tree are compatible with GPL-3.0-only (or with whatever license the commercial edition uses), nothing blocks banned or duplicate crates, and npm packages get no advisory check at all beyond Dependabot alerts. For a product moving to commercial distribution, license obligations (attribution notices in installers, copyleft compatibility) are untracked.
- fix: Add `cargo deny check licenses advisories bans sources` with a `deny.toml` (allow-list of licenses, replaces `cargo audit` and `.cargo/audit.toml` ignores), `pnpm audit --prod` (or `osv-scanner`) in CI, and generate a third-party notices file (`cargo about`, `license-checker`) bundled with the installers and extension.
- cross-package: yes (all)

## XC-30 Crate metadata and publish boundaries are incomplete
- severity: low
- category: build
- location: Cargo.toml:6-10, crates/ipc/Cargo.toml:1-7, crates/streams/Cargo.toml:1-7, crates/cli/Cargo.toml:1-7, crates/protocol/Cargo.toml:1-7, crates/engine/Cargo.toml:11, crates/bridge/src/config.rs:13, crates/e2e/src/bridge.rs:27
- problem: `[workspace.package]` has no `authors`, `repository` or `homepage`. `wirecatch-ipc`, `wirecatch-streams`, `wirecatch-cli` and `wirecatch-protocol` have no `publish = false`, so `cargo publish` at the workspace root would try to publish crates that were never meant to be public and have no README. The bridge and e2e reach outside their package with `include_str!("../../../config/...")`, which works only inside this repo and blocks packaging those crates.
- fix: Add `authors`, `repository`, `homepage` to `[workspace.package]` from `product.toml`; set `publish = false` on every crate except the engine (and protocol only if it is meant to be published); replace the `include_str!` of a file outside the package with the generated constants crate (XC-1).
- cross-package: yes (workspace, bridge, e2e, ipc, streams, cli, protocol)

## XC-31 CLAUDE.md, DECISIONS and PLAN disagree with the repo
- severity: medium
- category: docs
- location: CLAUDE.md (Layout, Commands, Rules), docs/DECISIONS.md:70-82, docs/DECISIONS.md:99-106, docs/PLAN.md (Rules every package follows), CONTRIBUTING.md:40, README.md:24-31
- problem: CLAUDE.md's Layout and D12's table omit `crates/ipc`, `crates/streams` and `crates/e2e` (README lists them). CLAUDE.md's Commands use `cargo fmt --all` (writes) where the gate is `--check`, and omit gates CI actually runs (engine docs with `-D warnings`, `cargo package`, MSRV check, `cargo audit`, generated-types drift). D18 item 1 gives a third clippy command. PLAN.md's "dependencies go in the package's own Cargo.toml" contradicts good workspace hygiene (XC-25). CONTRIBUTING says `scripts/linux-test.sh` matches CI, which it does not (XC-19).
- fix: Make one gate script the single source (XC-17) and have CLAUDE.md, CONTRIBUTING and D18 point to it instead of restating commands; update the layout lists; change the PLAN dependency rule to "shared dependencies go in `[workspace.dependencies]`".
- cross-package: yes (docs)

## XC-32 Engineering standards that are missing or not enforceable
- severity: medium
- category: docs
- location: CLAUDE.md (Rules), CONTRIBUTING.md:43-53
- problem: Standards a staff-level codebase needs are absent: where constants and identity live (only `extension-ids.json` is mentioned, in CONTRIBUTING and PLAN); a rule that webview to Rust contracts are generated (XC-8); logging conventions (target, level, file location, env var; XC-15); error-type conventions beyond "thiserror in libraries" (user-facing message type vs internal error, how engine errors map to core `user_error`); test conventions (shared helpers, no silent skips, naming); versioning and changelog policy (SemVer for the published engine, `CHANGELOG.md`, MSRV policy); a security contact (CONTRIBUTING:53 says "report privately" with no address, and there is no `SECURITY.md`); Conventional Commits are required but not checked. Several existing rules are unenforceable as written ("no blocking calls on async tasks", "no per-chunk allocations in hot paths", "errors say how to fix it") because no tool or test checks them.
- fix: Add sections to CLAUDE.md and CONTRIBUTING for identity/constants, generated contracts, logging, errors, tests and releases; add `SECURITY.md` with a reporting address and enable GitHub private vulnerability reporting; add a commit-msg check (commitlint or a small script in the gate). For the unenforceable rules, either back them with clippy `disallowed-methods`, a benchmark gate (`bench_local` thresholds), and a message-style test, or mark them as review-only.
- cross-package: yes (docs, all)
