# Cross flow: security-update-signatures

## Files read

Whole:

crates/updates/src/manifest.rs
crates/updates/src/release.rs
crates/core/src/manager/app_update.rs
crates/shell/src/updates.rs
crates/desktop/src/app_update.rs
crates/desktop/packaging/windows/installer.iss
crates/desktop/PACKAGING.md
scripts/release/manifest.mjs
scripts/release/updater-key.mjs
.github/workflows/release.yml

In part, for the code the flow calls:

crates/core/src/manager/mod.rs (`ManagerConfig` and its builders)
crates/core/src/headers.rs (`download_options`)
crates/desktop/src/backend.rs (`config`)
crates/desktop/src/lifecycle.rs (`quit`, `start_again`)
crates/desktop/src/main.rs (start, endpoint hand-over)
crates/desktop/src/main_window/notices_bar.rs, settings/general.rs (the update buttons)
crates/shell/src/api.rs (`check_app_update`, `fetch_app_update`)
crates/shell/src/appimage.rs, autostart.rs (`program`), bridge.rs (registration), host.rs (`DEV_BUILD`, `data_dir`)
crates/bridge/src/app.rs (`spawn_detached`)
crates/addons/src/archive.rs (`sha256_file`), manifest.rs (`Sha256Digest::from_hex`)
scripts/product/read.mjs (key format), scripts/gate.mjs (`publish` group)
gpui-pre-windows 0.3.10 `platform.rs` (`open_url`, `open_target`)
contracts/01, 02, 12, 29; FEATURES.md; docs/DECISIONS.md D111

Left out: `scripts/package.mjs` (relied on contract 29), and the engine's source (relied on contracts 01 and 02). Nothing was run on Linux. No update was run; the only checks run were two below, named in their findings or points.

## Findings

Points checked, with no finding:

- **Signing:** the signature covers the exact bytes uploaded; `verify_strict` runs before parsing; the workflow requires the tag to equal the product version and runs the placeholder check before signing.
- **Key:** compile-time constant. The placeholder happens to be valid base64url of 32 bytes, but no private key for it is known, and `no_updates` stops such a build first.
- **Downgrade:** `newest` only takes a version strictly above the compile-time `PRODUCT_VERSION`, and the signed version must equal the tag, so a signed older manifest cannot be replayed under a newer tag. Pre-release versions stay out of the stable channel even if GitHub's flag is changed, because the signed version carries the pre-release part.
- **Installer:** size and SHA-256 come from the signed manifest and are held in memory from check to install. The URL and the size in the list are unsigned, but the hash is checked on the finished file, including one resumed from an earlier part, and a mismatch deletes it.
- **Entry points:** no pipe, bridge or protocol command reaches `check_app_update`, `fetch_app_update` or `install`.
- **Fetching:** a cap of 4 MiB on metadata, no local network, and no https to http redirect. A GitHub asset URL answers 302 then 200 with `Accept: application/vnd.github+json` (checked with curl against a public release).
- **Restart:** a new AppImage that starts before the old copy has quit is handled by `start_again_later`.

### Xsecurity-update-signatures-1 [P1] The shipped app is never told where its releases are, so it never finds an update

- Where: `crates/desktop/src/backend.rs` `config`; `crates/core/src/manager/mod.rs` `with_official_app_releases`; `crates/core/src/manager/app_update.rs` `check_app_update`
- Category: broken feature
- Failure: a release build with a real key passes `no_updates`, checks after a minute and daily, and `check_app_update` returns `Ok(None)` on its first line because `app_releases` is `None`. The stage becomes `Current`, and Settings says "You have the newest version." whatever is published. No installed copy ever updates, so a security fix cannot reach anyone, and copies need a manual reinstall to get the corrected build. The placeholder key hides this today; it appears the moment the key is set.
- Evidence: `config` is `ManagerConfig::new(...).with_official_suffix_list()`. A search of `crates`, `apps` and `scripts` finds `with_official_app_releases` only at its definition and in a doc link, and nothing else assigns `app_releases`; `ManagerConfig::new` sets it to `None`. Tests use `with_app_releases`, so all pass. The same cause leaves `with_official_proxy_test` without a caller, so the proxy's Test connection has no URL in the shipped app (`proxy_test_url` doc: "a test then finds nothing to reach").
- Confidence: certain

### Xsecurity-update-signatures-2 [P1] A stray APPIMAGE variable makes the update overwrite another program

- Where: `crates/shell/src/updates.rs` `install_kind`, `replace_appimage`; same cause in `crates/shell/src/autostart.rs` `program` and `crates/shell/src/bridge.rs` registration
- Category: wrong implementation
- Failure: a `.deb` or `.rpm` install started by a process that runs from any other AppImage inherits that program's `APPIMAGE`: a browser AppImage through the bridge, or a terminal inside an editor AppImage. `install_kind` then returns `AppImage`, so "Install and restart" downloads the Wirecatch AppImage, and `replace_appimage` renames it over the path in the variable. The other program's file is destroyed and replaced by Wirecatch, which is then started. On a system package this also skips the intended page offer. The start-at-sign-in entry and the bridge's launcher script read the same variable, so they can point at the other program without any update.
- Evidence: both functions trust the variable alone and never compare it with `current_exe` or `APPDIR`. `spawn_detached` in the bridge starts the app without `env_remove` or `env_clear`, and there is none in `crates/bridge`, `crates/shell` or `crates/ipc`. The hash check proves the new file, not the place it goes. A genuine Wirecatch AppImage is unaffected because its runtime sets its own value. The update path is unreachable until finding 1 is fixed. Not run on Linux.
- Confidence: likely

### Xsecurity-update-signatures-3 [P2] A release that fails the signed list falls through to the unchecked release page

- Where: `crates/core/src/manager/app_update.rs` `check_app_update`; `crates/desktop/src/app_update.rs` `install`, `install_label`
- Category: wrong implementation
- Failure: on Windows or an AppImage, where the app can install, a signed release whose installer asset is missing or whose listed size differs from the manifest gets `download: None`. The app gives no warning and offers the same "Open release page" button as for a system package, with the banner text "Install it with your package manager." Someone who can change release assets, the case the signature exists for, replaces the setup with a different-sized file; the app then sends the person to download that file by hand, with no check at all.
- Evidence: the `and_then` chain yields `None` on `release.asset` missing and on `asset.size != file.size`, with no error or log. D111.4 reserves the page for a system package, and D111.3 says only that such an installer is not used. A manifest with no entry for this system has the same outcome, which is fine for a release that does not ship one, but the code cannot tell the cases apart from a tampered asset.
- Confidence: certain

### Xsecurity-update-signatures-4 [P2] The unsigned release page address goes to the system's shell with no check

- Where: `crates/core/src/manager/app_update.rs` `check_app_update` (`page`); `crates/desktop/src/main_window/notices_bar.rs`, `settings/general.rs`, `crates/desktop/src/app_update.rs` `install` (`cx.open_url`)
- Category: wrong implementation
- Failure: `page` is `html_url` from the unsigned list and is kept once a signature passes, against D111.3 ("Nothing about a release is trusted before its manifest's signature matches"). An answer forged for the API, paired with any genuine signed manifest of a newer version, can set it to a local path or a UNC program path; "What's new" or "Open release page" then runs `ShellExecuteW("open", target)`. It needs a forged TLS answer, so a certificate the system trusts, and a click, which is why this is not rated higher. GitHub itself always answers with its own https page.
- Evidence: `html_url` is a plain `String`, never parsed or compared with the repository. gpui-pre-windows `open_url` passes the text unchanged to `open_target`, which calls `ShellExecuteW`. Not run.
- Confidence: likely

### Xsecurity-update-signatures-5 [P2] The app quits before the administrator prompt is answered, so a refusal loses the failure path

- Where: `crates/shell/src/updates.rs` `run_setup`; `crates/desktop/src/app_update.rs` `install`
- Category: wrong implementation
- Failure: the person answers No at the prompt, or the setup fails after starting. The app has already taken `ShellExecuteW`'s success as "started", set `Installing` and quit, stopping its downloads. Nothing is installed, nothing restarts, and no message appears. `InstallFailed` with Try again can only appear when the shell cannot start the file at all.
- Evidence: the built `target/packages/Wirecatch-0.1.0-windows-x64-setup.exe` and Inno Setup's `SetupLdr.e32` and `SetupLdr.e64` carry `requestedExecutionLevel level="asInvoker"`, so the prompt comes from the setup asking for elevation after it has started, not from the shell. `run_setup`'s comment says the shell "asks for an administrator's approval", which is not what this file does. Refusing the prompt was not run.
- Confidence: likely

## Suggestions

### Xsecurity-update-signatures-S1 Refuse a manifest that is older than the newest one seen

- Today: a signed manifest has no date, and the app keeps no record of versions seen. Whoever controls the list can hide newer releases, or offer a real but older release that is still above the installed version, for as long as they control it.
- Better: add a signing date with a maximum age, or save the highest signed version seen and refuse less, as TUF-style updaters do.
- Value: a known-vulnerable intermediate release cannot be pushed to someone once a fix is out.

### Xsecurity-update-signatures-S2 Close the gap between the hash check and the elevated setup

- Today: the hash is read, the file handle is closed, and the setup is started by path from the user-writable `updates` folder. It then elevates itself after the prompt, and that prompt can wait indefinitely. A program running as the same user can swap the file in that gap and receive the approval. No privilege boundary is crossed without a prompt, so this is not a finding.
- Better: hold the file open without write or delete sharing from the hash until the app exits, and verify the code signature once a certificate exists (D19).
- Value: the approval a person gives applies to the file that was checked.

### Xsecurity-update-signatures-S3 Sign in a small job of its own, behind a protected environment

- Today: `UPDATER_PRIVATE_KEY` is a plain repository secret, read by a workflow file taken from whichever commit a `v*` tag points at. The signing job has `contents: write` and runs `pnpm install` and `rustup` before signing, so code they leave behind runs with the key in reach.
- Better: use a GitHub environment with required reviewers, and a job that holds only the secret, the downloaded artifacts and `manifest.mjs`, with the checks done in an earlier job.
- Value: a stolen push credential or a bad dependency does not become a signed update, and a lost key has no recovery short of a reinstall by hand.

### Xsecurity-update-signatures-S4 Check the signature and the platform list before drafting

- Today: `manifest.mjs` never verifies its signature against `public_key` in `config/product.toml`, and it fails only when no installer matches. A wrong secret is discovered after publishing, when every copy quietly logs a refused update.
- Better: verify with the public key in the script, and require all eight expected installers.
- Value: a release that no installed copy would accept can never be published by mistake.

### Xsecurity-update-signatures-S5 Let an unknown field or kind leave older copies able to update

- Today: `deny_unknown_fields` and closed enums make any new manifest field, or a new `kind`, unreadable to every copy already installed. `check_app_update` also considers only the single newest release, so one release with a bad or missing manifest stops updates for everyone, with only a log line on the quiet check.
- Better: ignore fields and entries not understood, since the bytes are already proven signed, and keep the strict check at the signature.
- Value: the format can grow without stranding installed copies.
